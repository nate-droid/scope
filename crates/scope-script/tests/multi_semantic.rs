#[path = "../examples/support/multi_semantic.rs"]
mod fixture;
use fixture::*;
use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
    text::{Position, TextEdit, TextRange},
    tool::{AgentSession, semantic::SemanticToolProvider},
};
use scope_script::{Host, Value, runner::ScriptRunner, semantic::SemanticHost};

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    let Value::Object(fields) = value else {
        panic!("not an object: {value:?}")
    };
    &fields[key]
}

#[test]
fn two_reads_discover_metadata_combine_results_and_refresh_independent_snapshots() {
    let service = MultiLanguage::default();
    let mut runner = ScriptRunner::with_operations(&service, bindings()).unwrap();
    let mut scripts = ScratchBuffers::default();
    scripts.create("scope-script", COMBINED);
    let mut targets = ScratchBuffers::default();
    targets.create(LANGUAGE, "alpha");
    for text in ["alpha", "beta"] {
        let source = scripts.selected().unwrap();
        let target = targets.selected().unwrap();
        let before = (
            source.version(),
            source.text().to_owned(),
            source.is_dirty(),
            target.version(),
            target.text().to_owned(),
            target.is_dirty(),
        );
        runner
            .invoke_script_with_target(source, Some(target), &mut Vec::new())
            .unwrap();
        let record = runner.records.last().unwrap();
        assert_ne!(record.source.identity, record.target.identity);
        assert_eq!(record.source.text.trim(), COMBINED.trim());
        assert_eq!(record.target.text, text);
        let Value::Array(entries) = &record.catalog else {
            panic!()
        };
        assert_eq!(entries.len(), 2);
        for (entry, id) in entries.iter().zip(["echo", "measure"]) {
            assert_eq!(field(entry, "provider").as_str(), Some("semantic"));
            assert_eq!(
                field(entry, "action"),
                &Value::String(format!("language.{LANGUAGE}.{id}"))
            );
            assert_eq!(
                field(entry, "description"),
                &Value::String(format!("Fixture {id}"))
            );
            assert_eq!(
                field(field(entry, "metadata"), "target").as_str(),
                Some("Document")
            );
            assert!(format!("{:?}", field(entry, "metadata")).contains("SI7"));
        }
        assert_eq!(record.printed, vec![record.catalog.clone()]);
        assert_eq!(record.results.len(), 2);
        let combined = record.outcome.as_ref().unwrap();
        for (index, id) in ["echo", "measure"].into_iter().enumerate() {
            assert_eq!(
                field(&record.results[index], "status").as_str(),
                Some("read_only")
            );
            assert_eq!(field(combined, id), field(&record.results[index], "value"));
            assert_eq!(field(field(combined, id), "source").as_str(), Some(text));
        }
        assert_eq!(
            field(field(field(combined, "echo"), "arguments"), "label").as_str(),
            Some("demo")
        );
        assert_eq!(
            field(field(field(combined, "measure"), "arguments"), "limit"),
            &Value::Integer(3)
        );
        assert_eq!(
            before,
            (
                source.version(),
                source.text().to_owned(),
                source.is_dirty(),
                target.version(),
                target.text().to_owned(),
                target.is_dirty()
            )
        );
        if text == "alpha" {
            targets
                .selected_mut()
                .unwrap()
                .apply_edit(TextEdit::replace(
                    TextRange::new(Position::new(0, 0), Position::new(0, 5)),
                    "beta",
                ))
                .unwrap();
            scripts
                .selected_mut()
                .unwrap()
                .apply_edit(TextEdit::replace(
                    TextRange::new(Position::new(0, 0), Position::new(0, 0)),
                    " ",
                ))
                .unwrap();
        }
    }
    assert_ne!(
        runner.records[0].target.revision,
        runner.records[1].target.revision
    );
    assert_ne!(
        runner.records[0].source.revision,
        runner.records[1].source.revision
    );
    assert_eq!(runner.records[0].target.text, "alpha");
    assert_eq!(runner.records[0].source.text, COMBINED);
    assert!(targets.selected().unwrap().is_dirty());
    assert_eq!(
        *service.calls.lock().unwrap(),
        ["echo", "measure", "echo", "measure"]
    );
}

#[test]
fn constructor_rejects_duplicate_unknown_qualified_unsupported_and_empty_ids() {
    let service = MultiLanguage::default();
    for (ids, message) in [
        (vec!["echo", "echo"], "duplicate"),
        (vec!["missing"], "unknown"),
        (vec!["language.example.multi.echo"], "unknown"),
        (vec!["propose"], "unsupported"),
        (vec![], "empty"),
    ] {
        let error = ScriptRunner::with_operations(
            &service,
            ids.into_iter().map(|id| (id.into(), Some(true))),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains(message), "{error}");
    }
    assert!(service.calls.lock().unwrap().is_empty());
}

#[test]
fn hidden_denied_and_invalid_typed_arguments_never_dispatch() {
    let service = MultiLanguage::default();
    let mut targets = ScratchBuffers::default();
    targets.create(LANGUAGE, "target");
    for (id, authorization, arguments, status) in [
        ("hidden", Some(true), "{}", None),
        ("denied", None, "{}", None),
        ("denied", Some(false), "{}", None),
        ("echo", Some(true), "{label: 7}", Some("invalid_arguments")),
        (
            "measure",
            Some(true),
            "{enabled: true, limit: false}",
            Some("invalid_arguments"),
        ),
    ] {
        let selected = if id == "hidden" {
            bindings()
        } else {
            vec![(id.into(), authorization)]
        };
        let mut runner = ScriptRunner::with_operations(&service, selected).unwrap();
        let mut scripts = ScratchBuffers::default();
        scripts.create(
            "scope-script",
            format!("scope.invoke(\"semantic\", \"language.{LANGUAGE}.{id}\", {arguments})"),
        );
        runner
            .invoke_script_with_target(
                scripts.selected().unwrap(),
                targets.selected().ok(),
                &mut Vec::new(),
            )
            .unwrap();
        let record = &runner.records[0];
        if let Some(status) = status {
            assert_eq!(
                field(record.outcome.as_ref().unwrap(), "status").as_str(),
                Some(status)
            );
        } else {
            assert!(record.outcome.is_err());
            assert!(record.results.is_empty());
        }
        if id == "denied" {
            assert!(format!("{:?}", record.catalog).contains("denied"));
        }
    }
    assert!(service.calls.lock().unwrap().is_empty());
}

#[test]
fn pure_script_still_requires_a_valid_target() {
    let service = MultiLanguage::default();
    let mut runner = ScriptRunner::with_operations(&service, bindings()).unwrap();
    let mut scripts = ScratchBuffers::default();
    scripts.create("scope-script", "{answer: 42}");
    let mut targets = ScratchBuffers::default();
    targets.create("wrong-language", "target");
    for target in [None, targets.selected().ok()] {
        assert!(
            runner
                .invoke_script_with_target(scripts.selected().unwrap(), target, &mut Vec::new())
                .is_err()
        );
        assert!(runner.records.is_empty());
    }
    targets.create(LANGUAGE, "target");
    runner
        .invoke_script_with_target(
            scripts.selected().unwrap(),
            targets.selected().ok(),
            &mut Vec::new(),
        )
        .unwrap();
    assert_eq!(
        runner.records[0].outcome.as_ref().unwrap(),
        &Value::object([("answer", Value::Integer(42))])
    );
    assert!(runner.records[0].results.is_empty());
    assert!(service.calls.lock().unwrap().is_empty());
}

#[test]
fn low_level_bindings_filter_effects_reject_duplicates_and_preserve_metadata() {
    let service = MultiLanguage::default();
    let mut targets = ScratchBuffers::default();
    targets.create(LANGUAGE, "target");
    let target = targets.selected().unwrap();
    let mut controller = SourceEditController::default();
    {
        let provider = SemanticToolProvider::new(
            &service,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            None,
            |_| true,
        )
        .unwrap();
        let session = AgentSession::builder().provider(provider).build().unwrap();
        let catalog = session.catalog();
        let mut host = SemanticHost::with_bindings(
            session,
            ["echo", "measure", "propose", "missing"]
                .map(|id| (format!("language.{LANGUAGE}.{id}"), Some(true))),
            target.text(),
            target.version(),
        )
        .unwrap();
        let Value::Array(entries) = host.operations().unwrap() else {
            panic!()
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(host.operation_names().len(), 2);
        for entry in entries {
            let action = field(&entry, "action").as_str().unwrap();
            let original = catalog
                .tools
                .iter()
                .find(|tool| tool.definition.name == action)
                .unwrap();
            assert_eq!(
                field(&entry, "metadata"),
                original.metadata.as_ref().unwrap()
            );
        }
        let error = host
            .invoke(
                "semantic",
                "language.example.multi.propose",
                Value::object([] as [(&str, Value); 0]),
            )
            .unwrap_err();
        assert!(format!("{error:?}").contains("unsupported"));
        assert!(
            host.invoke("semantic", "language.example.multi.hidden", Value::Null)
                .is_err()
        );
        assert!(
            host.invoke("semantic", "language.example.multi.missing", Value::Null)
                .is_err()
        );
    }
    assert_eq!(controller.list().count(), 0);
    let session = AgentSession::builder().build().unwrap();
    assert!(
        SemanticHost::with_bindings(
            session,
            [
                ("language.example.multi.echo".into(), None),
                ("language.example.multi.echo".into(), Some(true))
            ],
            "target",
            0
        )
        .err()
        .unwrap()
        .to_string()
        .contains("duplicate")
    );
    assert!(service.calls.lock().unwrap().is_empty());
}

#[test]
fn file_backed_target_keeps_unsaved_changes_dirty_without_saving() {
    use scope_ide::{language::LanguageRegistry, workspace::Workspace};
    static SERVICE: std::sync::LazyLock<MultiLanguage> = std::sync::LazyLock::new(Default::default);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target.multi");
    std::fs::write(&path, "disk").unwrap();
    let workspace = Workspace::read_only(directory.path().to_string_lossy()).unwrap();
    let registry = LanguageRegistry::builtins().with_language(&*SERVICE);
    let mut targets = ScratchBuffers::default();
    targets.open(&workspace, &registry, "target.multi").unwrap();
    targets
        .selected_mut()
        .unwrap()
        .apply_edit(TextEdit::replace(
            TextRange::new(Position::new(0, 0), Position::new(0, 4)),
            "unsaved",
        ))
        .unwrap();
    let mut scripts = ScratchBuffers::default();
    scripts.create("scope-script", COMBINED);
    let mut runner = ScriptRunner::with_operations(&*SERVICE, bindings()).unwrap();
    let version = targets.selected().unwrap().version();
    runner
        .invoke_script_with_target(
            scripts.selected().unwrap(),
            Some(targets.selected().unwrap()),
            &mut Vec::new(),
        )
        .unwrap();
    assert!(runner.records[0].outcome.is_ok());
    assert_eq!(runner.records[0].target.text, "unsaved");
    assert_eq!(targets.selected().unwrap().version(), version);
    assert!(targets.selected().unwrap().is_dirty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "disk");
}
