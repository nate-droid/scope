#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod fixture;

use scope_ide::{
    document::{Document, DocumentId},
    source_edit::{DocumentSource, PermittedEditScope, SourceEditController, SourceTarget},
    text::{Position, TextRange},
    tool::{AgentSession, semantic::SemanticToolProvider},
    workspace::Workspace,
};
use scope_script::{ErrorKind, Interpreter, Value, semantic::SemanticHost};
use std::sync::atomic::Ordering;

const ACTION: &str = "language.example.declarations.describe_selection";

#[test]
fn e5_scripts_preserve_complete_outcomes_and_failure_boundaries() {
    check_envelopes(false);
}

#[test]
fn presentation_preserves_real_nested_semantic_envelopes_and_failure_distinctions() {
    check_envelopes(true);
}

fn check_envelopes(check_presentation: bool) {
    for case in [
        "success",
        "stale",
        "selection",
        "partial_selection",
        "source_denied",
        "denied",
        "missing",
        "unavailable",
        "no_selection",
        "arguments",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("example.decl");
        std::fs::write(&path, fixture::SOURCE).unwrap();
        let workspace = Workspace::read_only(dir.path().to_str().unwrap()).unwrap();
        let file = workspace.load_file_revision("example.decl").unwrap();
        let mut document = Document::from_revisioned_workspace_file(DocumentId::new(1), file);
        let before = document.clone();
        let service = fixture::DeclarationLanguage::default();
        let mut controller = SourceEditController::default();
        {
            let target = DocumentSource::new(&mut document, "example.declarations");
            let selection = match case {
                "no_selection" => None,
                "selection" => Some(TextRange::new(Position::new(99, 0), Position::new(99, 1))),
                "partial_selection" => {
                    Some(TextRange::new(Position::new(0, 0), Position::new(0, 3)))
                }
                _ => Some(TextRange::new(Position::new(0, 0), Position::new(0, 15))),
            };
            let provider = SemanticToolProvider::new(
                &service,
                &target,
                &mut controller,
                PermittedEditScope::Document(target.source_id()),
                selection,
                |_| case != "source_denied",
            )
            .unwrap();
            let session = AgentSession::builder().provider(provider).build().unwrap();
            let action = if case == "unavailable" {
                "language.missing.inspect"
            } else {
                ACTION
            };
            let authorization = match case {
                "denied" => Some(false),
                "missing" => None,
                _ => Some(true),
            };
            let mut host = SemanticHost::new(
                session,
                action,
                target.text(),
                target.version() + u64::from(case == "stale"),
                authorization,
            );
            let arguments = if case == "arguments" {
                "{unexpected: true}"
            } else {
                "{}"
            };
            // Print the entire envelope before any success-specific projection.
            let script = format!(
                r#"
                let catalog = scope.operations();
                print(catalog);
                let outcome = scope.invoke("semantic", "{action}", {arguments});
                let report = {{title: "Failure envelope explorer", outcome: outcome}};
                print(report);
                report
            "#
            );
            let mut interpreter = Interpreter::new();
            let evaluated = interpreter.eval(&script, &mut host);
            let output = interpreter.take_output();
            println!("{case}: {evaluated:?}; output: {output:?}");
            match case {
                "denied" | "missing" => {
                    assert!(matches!(evaluated.unwrap_err().kind, ErrorKind::Tool(_)));
                    assert_eq!(output.len(), 1);
                }
                "unavailable" => {
                    assert!(
                        matches!(evaluated.unwrap_err().kind, ErrorKind::Runtime(ref message) if message == "unavailable semantic operation")
                    );
                    assert_eq!(output, vec![Value::Array(vec![])]);
                }
                _ => {
                    let report = evaluated.unwrap();
                    assert_eq!(output.len(), 2);
                    assert_eq!(output[1], report);
                    let outcome = interpreter.eval("outcome", &mut host).unwrap();
                    if check_presentation {
                        use scope_script::presentation::{
                            FormatOptions, format_value, format_value_with, value_json,
                        };

                        let before = report.clone();
                        let json = value_json(&report);
                        let full = FormatOptions {
                            max_depth: usize::MAX,
                            max_collection_items: usize::MAX,
                            max_string_chars: usize::MAX,
                            max_nodes: usize::MAX,
                        };
                        assert_eq!(
                            serde_json::from_str::<serde_json::Value>(&format_value_with(
                                &report, &full,
                            ))
                            .unwrap(),
                            json,
                            "{case}"
                        );
                        assert_eq!(json["outcome"], value_json(&outcome), "{case}");
                        let bounded = format_value_with(
                            &report,
                            &FormatOptions {
                                max_depth: 1,
                                ..full
                            },
                        );
                        assert!(bounded.contains("<depth truncated>"), "{case}: {bounded}");
                        let _ = format_value(&report);
                        assert_eq!(report, before, "{case}");
                        assert_eq!(value_json(&report), json, "{case}");
                        assert_eq!(interpreter.eval("outcome", &mut host).unwrap(), outcome);
                        assert_eq!(interpreter.take_output(), Vec::<Value>::new());

                        if case != "success" {
                            assert_eq!(json["outcome"].get("value"), None, "{case}");
                            assert_eq!(value_json(&Value::Null), serde_json::Value::Null);
                            assert_eq!(format_value(&Value::Null), "null");
                            assert_eq!(interpreter.eval("null", &mut host).unwrap(), Value::Null);
                            let error = interpreter.eval("outcome.value", &mut host).unwrap_err();
                            assert!(matches!(error.kind, ErrorKind::Runtime(ref message)
                                if message == "missing object field"));
                            assert_eq!(value_json(&report), json, "{case}");
                        }
                    }
                    assert_eq!(
                        report,
                        Value::object([
                            ("title", "Failure envelope explorer".into()),
                            ("outcome", outcome.clone())
                        ])
                    );
                    let Value::Object(fields) = &outcome else {
                        panic!("{case}: {outcome:?}")
                    };
                    let status = match case {
                        "success" => "read_only",
                        "stale" => "revision_conflict",
                        "source_denied" => "unauthorized",
                        "no_selection" => "ambiguous_target",
                        _ => "invalid_arguments",
                    };
                    assert_eq!(
                        fields.get("status"),
                        Some(&Value::String(status.into())),
                        "{case}"
                    );
                    if case != "success" {
                        assert_eq!(
                            fields.len(),
                            if case == "arguments" { 4 } else { 3 },
                            "{case}: {outcome:?}"
                        );
                        assert_eq!(
                            fields.contains_key("invocation_schema"),
                            case == "arguments"
                        );
                        assert_eq!(
                            fields.get("operation_id"),
                            Some(&Value::String("describe_selection".into()))
                        );
                        assert!(
                            matches!(fields.get("message"), Some(Value::String(message)) if !message.is_empty())
                        );
                        assert!(!fields.contains_key("value"), "{case}: {outcome:?}");
                        let error = interpreter.eval("outcome.value", &mut host).unwrap_err();
                        assert!(
                            matches!(error.kind, ErrorKind::Runtime(ref message) if message == "missing object field")
                        );
                    }
                }
            }
        }
        assert_eq!(
            service.calls.load(Ordering::SeqCst),
            usize::from(matches!(case, "success" | "partial_selection")),
            "{case}"
        );
        assert_eq!(controller.list().count(), 0, "{case}");
        assert_eq!(document, before, "{case}");
        let after = workspace.load_file_revision("example.decl").unwrap();
        assert_eq!(Some(after.revision.as_str()), before.disk_revision());
        assert_eq!(std::fs::read(&path).unwrap(), fixture::SOURCE.as_bytes());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        // Missing files fail during host setup, before a semantic target exists.
        assert!(workspace.load_file_revision("missing.decl").is_err());
    }
}
