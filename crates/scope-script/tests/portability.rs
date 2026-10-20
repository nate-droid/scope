#[path = "../../scope-ide/examples/state_description_support/mod.rs"]
mod fixture;

use fixture::{SOURCE, StateMachineLanguage};
use scope_ide::{
    document::{Document, DocumentId},
    source_edit::{
        DocumentSource, PermittedEditScope, SourceEditController, SourceId, SourceTarget,
    },
    text::{Position, TextRange},
    tool::{AgentSession, ToolEffect, semantic::SemanticToolProvider},
    workspace::Workspace,
};
use scope_script::{Interpreter, Value, semantic::SemanticHost};
use std::sync::atomic::Ordering;

#[test]
fn e4_workspace_state_machine_report_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("example.state");
    std::fs::write(&path, SOURCE).unwrap();
    let workspace = Workspace::read_only(dir.path().to_str().unwrap()).unwrap();
    let disk_before = workspace.load_file_revision("example.state").unwrap();
    let revision_before = disk_before.revision.clone();
    let bytes_before = std::fs::read(&path).unwrap();
    let mut document = Document::from_revisioned_workspace_file(DocumentId::new(1), disk_before);
    let before = document.clone();
    let service = StateMachineLanguage::default();
    let mut controller = SourceEditController::default();
    let mut source_policy_calls = 0;
    {
        let target = DocumentSource::new(&mut document, "example.state_machine");
        let provider = SemanticToolProvider::new(
            &service,
            &target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(TextRange::new(
                Position::new(0, 0),
                Position::new(0, SOURCE.lines().next().unwrap().len()),
            )),
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
        let session = AgentSession::builder().provider(provider).build().unwrap();
        let catalog = session.catalog();
        assert_eq!(catalog.tools.len(), 1);
        let entry = &catalog.tools[0];
        assert_eq!(
            entry.definition.name,
            "language.example.state_machine.describe_selection"
        );
        assert_eq!(entry.definition.effect, ToolEffect::Read);
        let mut host = SemanticHost::new(
            session,
            entry.definition.name.clone(),
            target.text(),
            target.version(),
            Some(true),
        );
        let mut interpreter = Interpreter::new();
        // This projection belongs to this provider's payload, not a shared semantic schema.
        let report = interpreter
            .eval(
                r#"
                let operations = scope.operations();
                let operation = operations[0];
                let result = scope.invoke(operation.provider, operation.action, {});
                let report = {
                    title: "Workspace state-machine report",
                    status: result.status,
                    transition: result.value.transition,
                    states: result.value.topology.states,
                    outgoing: result.value.topology.outgoing,
                    topology: result.value.capabilities.topology.status,
                    reachability: result.value.capabilities.reachability.status,
                    reachability_reason: result.value.capabilities.reachability.reason,
                    inferred_type: result.value.inferred_type.status
                };
                print(report);
                report
                "#,
                &mut host,
            )
            .unwrap();
        let transition = Value::object([
            ("from", "idle".into()),
            ("to", "running".into()),
            ("event", "start".into()),
        ]);
        let expected = Value::object([
            ("title", "Workspace state-machine report".into()),
            ("status", "read_only".into()),
            ("transition", transition.clone()),
            (
                "states",
                Value::Array(vec!["idle".into(), "running".into()]),
            ),
            ("outgoing", Value::Array(vec![transition])),
            ("topology", "supported".into()),
            ("reachability", "unknown".into()),
            ("reachability_reason", "runtime guards unavailable".into()),
            ("inferred_type", "unsupported".into()),
        ]);
        assert_eq!(report, expected);
        assert_eq!(interpreter.take_output(), vec![expected]);
        assert_eq!(
            interpreter.eval("operations", &mut host).unwrap(),
            Value::Array(vec![Value::object([
                ("provider", "semantic".into()),
                ("action", entry.definition.name.clone().into()),
                ("description", entry.definition.description.clone().into()),
                ("metadata", entry.metadata.clone().unwrap()),
            ])])
        );
        assert_eq!(
            interpreter.eval("result.operation_id", &mut host).unwrap(),
            "describe_selection".into()
        );
    }
    assert_eq!(source_policy_calls, 1);
    assert_eq!(service.calls.load(Ordering::SeqCst), 1);
    assert_eq!(controller.list().count(), 0);
    assert_eq!(document, before);
    assert_eq!(document.version(), before.version());
    assert_eq!(document.disk_revision(), Some(revision_before.as_str()));
    let disk_after = workspace.load_file_revision("example.state").unwrap();
    assert_eq!(disk_after.revision, revision_before);
    assert_eq!(disk_after.contents.as_bytes(), bytes_before);
    assert_eq!(std::fs::read(&path).unwrap(), bytes_before);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
