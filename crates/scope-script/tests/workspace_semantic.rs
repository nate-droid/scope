#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod fixture;

use fixture::{DeclarationLanguage, SOURCE};
use scope_ide::{
    document::{Document, DocumentId},
    source_edit::{
        DocumentSource, PermittedEditScope, SourceEditController, SourceId, SourceTarget,
    },
    text::{Position, TextRange},
    tool::{AgentSession, ToolCall, ToolEffect, semantic::SemanticToolProvider},
    workspace::{Workspace, WorkspaceError},
};
use scope_script::{Host, Interpreter, Value, semantic::SemanticHost};
use std::sync::atomic::Ordering;

const ACTION: &str = "language.example.declarations.describe_selection";

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    let Value::Object(fields) = value else {
        panic!("expected object: {value:?}")
    };
    fields
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {value:?}"))
}

fn workspace() -> (tempfile::TempDir, Workspace, Document) {
    let dir = tempfile::tempdir().unwrap();
    let fixture = include_str!("../examples/semantic-workspace/example.decl");
    assert_eq!(fixture, SOURCE);
    std::fs::write(dir.path().join("example.decl"), fixture).unwrap();
    let workspace = Workspace::read_only(dir.path().to_str().unwrap()).unwrap();
    let file = workspace.load_file_revision("example.decl").unwrap();
    let document = Document::from_revisioned_workspace_file(DocumentId::new(1), file);
    (dir, workspace, document)
}

fn selection() -> TextRange {
    TextRange::new(Position::new(0, 0), Position::new(0, 15))
}

fn assert_unchanged(
    dir: &tempfile::TempDir,
    workspace: &Workspace,
    document: &Document,
    before: &Document,
) {
    assert_eq!(document, before);
    let disk = workspace.load_file_revision("example.decl").unwrap();
    assert_eq!(Some(disk.revision.as_str()), before.disk_revision());
    assert_eq!(disk.contents, SOURCE);
    assert_eq!(
        std::fs::read(dir.path().join("example.decl")).unwrap(),
        SOURCE.as_bytes()
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn workspace_script_preserves_direct_semantic_result_and_source() {
    let (dir, workspace, mut document) = workspace();
    let before = document.clone();
    let service = DeclarationLanguage::default();
    let mut controller = SourceEditController::default();
    let mut source_policy_calls = 0;
    let mut call_policy_calls = 0;
    {
        let target = DocumentSource::new(&mut document, "example.declarations");
        assert_eq!(target.source_id(), SourceId::Document(before.id()));
        let provider = SemanticToolProvider::new(
            &service,
            &target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(selection()),
            |authorization| {
                source_policy_calls += 1;
                assert_eq!(authorization.source.id, SourceId::Document(before.id()));
                assert_eq!(authorization.source.text, SOURCE);
                assert_eq!(authorization.source.version, before.version());
                assert_eq!(authorization.arguments, &serde_json::json!({}));
                true
            },
        )
        .unwrap();
        let mut session = AgentSession::builder().provider(provider).build().unwrap();
        let catalog = session.catalog();
        assert_eq!(catalog.tools.len(), 1);
        let entry = &catalog.tools[0];
        assert_eq!(entry.definition.name, ACTION);
        assert_eq!(entry.definition.effect, ToolEffect::Read);
        let metadata = entry.metadata.clone().unwrap();
        assert_eq!(field(&metadata, "target").as_str(), Some("Selection"));
        assert_eq!(field(&metadata, "purpose").as_str(), Some("Exploration"));
        let call = ToolCall::new(ACTION)
            .with_argument("expected_source", SOURCE.into())
            .with_argument("expected_version", target.version().to_string().into())
            .with_argument("arguments", Value::object([] as [(&str, Value); 0]));
        let direct = session
            .invoke_with_call_policy(call.clone(), |definition, actual| {
                call_policy_calls += 1;
                assert_eq!(definition.effect, ToolEffect::Read);
                assert_eq!(actual, &call);
                true
            })
            .unwrap()
            .value;
        let mut host =
            SemanticHost::new(session, ACTION, target.text(), target.version(), Some(true));
        let discovered = host.operations().unwrap();
        assert_eq!(
            discovered,
            Value::Array(vec![Value::object([
                ("provider", "semantic".into()),
                ("action", ACTION.into()),
                ("description", entry.definition.description.clone().into()),
                ("metadata", metadata),
            ])])
        );
        let mut interpreter = Interpreter::new();
        let result = interpreter
            .eval(
                include_str!("../examples/workspace_semantic.scope"),
                &mut host,
            )
            .unwrap();
        assert_eq!(result, direct);
        let operation = interpreter.eval("operation", &mut host).unwrap();
        assert_eq!(discovered, Value::Array(vec![operation]));
        assert_eq!(field(&result, "status").as_str(), Some("read_only"));
        assert_eq!(
            field(&result, "operation_id").as_str(),
            Some("describe_selection")
        );
        assert!(!field(&result, "rationale").as_str().unwrap().is_empty());
        let value = field(&result, "value");
        assert_eq!(field(value, "name").as_str(), Some("answer"));
        assert_eq!(
            field(value, "kind").as_str(),
            Some("integer_literal_binding")
        );
        assert_eq!(field(value, "literal").as_str(), Some("42"));
        assert_eq!(field(value, "partial"), &Value::Boolean(true));
        assert_eq!(
            field(field(value, "references"), "status").as_str(),
            Some("unsupported")
        );
        assert_eq!(
            field(field(value, "inferred_type"), "status").as_str(),
            Some("unknown")
        );
        assert_eq!(
            field(field(value, "inferred_type"), "reason").as_str(),
            Some("fixture has no type analysis")
        );
        assert_eq!(
            field(value, "span"),
            &Value::object([
                ("line", 0_i64.into()),
                ("start_column", 0_i64.into()),
                ("end_column", 15_i64.into()),
            ])
        );
        let output = interpreter.take_output();
        let report = interpreter.eval("report", &mut host).unwrap();
        assert_eq!(
            report,
            Value::object([
                ("title", "Workspace declaration report".into()),
                ("status", "read_only".into()),
                ("name", "answer".into()),
                ("kind", "integer_literal_binding".into()),
                ("line", 0_i64.into()),
                ("partial", true.into()),
                ("inferred_type", "unknown".into()),
                ("references", "unsupported".into()),
            ])
        );
        assert_eq!(output, vec![report]);
    }
    assert_eq!(call_policy_calls, 1);
    assert_eq!(source_policy_calls, 2);
    assert_eq!(service.calls.load(Ordering::SeqCst), 2);
    assert_eq!(controller.list().count(), 0);
    assert_unchanged(&dir, &workspace, &document, &before);
}

#[test]
fn workspace_semantic_boundaries_do_not_mutate_sources() {
    for case in [
        "denied",
        "missing",
        "source_denied",
        "stale",
        "arguments",
        "selection",
        "partial_selection",
        "unavailable",
    ] {
        let (dir, workspace, mut document) = workspace();
        let before = document.clone();
        let service = DeclarationLanguage::default();
        let mut controller = SourceEditController::default();
        let mut policy_calls = 0;
        {
            let target = DocumentSource::new(&mut document, "example.declarations");
            assert_eq!(target.source_id(), SourceId::Document(before.id()));
            let selected = match case {
                "selection" => TextRange::new(Position::new(99, 0), Position::new(99, 1)),
                "partial_selection" => TextRange::new(Position::new(0, 0), Position::new(0, 3)),
                _ => selection(),
            };
            let provider = SemanticToolProvider::new(
                &service,
                &target,
                &mut controller,
                PermittedEditScope::Document(target.source_id()),
                Some(selected),
                |_| {
                    policy_calls += 1;
                    case != "source_denied"
                },
            )
            .unwrap();
            let session = AgentSession::builder().provider(provider).build().unwrap();
            let action = if case == "unavailable" {
                "language.missing.inspect"
            } else {
                ACTION
            };
            let mut host = SemanticHost::new(
                session,
                action,
                target.text(),
                target.version() + u64::from(case == "stale"),
                match case {
                    "missing" => None,
                    "denied" => Some(false),
                    _ => Some(true),
                },
            );
            let Value::Array(operations) = host.operations().unwrap() else {
                panic!()
            };
            assert_eq!(operations.len(), usize::from(case != "unavailable"));
            assert!(host.invoke("other", action, Value::Null).is_err());
            assert!(
                host.invoke("semantic", "language.other.inspect", Value::Null)
                    .is_err()
            );
            let arguments = if case == "arguments" {
                Value::object([("unexpected", true.into())])
            } else {
                Value::object([] as [(&str, Value); 0])
            };
            let result = host.invoke("semantic", action, arguments);
            match case {
                "denied" | "missing" | "unavailable" => assert!(result.is_err(), "{case}"),
                _ => assert_eq!(
                    field(&result.unwrap(), "status").as_str(),
                    Some(match case {
                        "source_denied" => "unauthorized",
                        "stale" => "revision_conflict",
                        _ => "invalid_arguments",
                    }),
                    "{case}"
                ),
            }
        }
        assert_eq!(
            policy_calls,
            usize::from(matches!(
                case,
                "source_denied" | "selection" | "partial_selection"
            )),
            "{case}"
        );
        assert_eq!(
            service.calls.load(Ordering::SeqCst),
            usize::from(case == "partial_selection"),
            "{case}"
        );
        assert_eq!(controller.list().count(), 0, "{case}");
        assert_unchanged(&dir, &workspace, &document, &before);
    }
}

#[test]
fn invalid_workspace_paths_never_create_a_semantic_target() {
    let (dir, workspace, document) = workspace();
    let before = document.clone();
    assert!(matches!(
        workspace.load_file_revision("missing.decl"),
        Err(WorkspaceError::ReadFile { .. })
    ));
    assert!(matches!(
        workspace.load_file_revision("../outside.decl"),
        Err(WorkspaceError::InvalidPath { .. })
    ));
    assert_unchanged(&dir, &workspace, &document, &before);
}
