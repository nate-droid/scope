#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod fixture;

use fixture::{DeclarationLanguage, SOURCE};
use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
    text::{Position, TextRange},
    tool::{AgentSession, ToolEffect, semantic::SemanticToolProvider},
};
use scope_script::{Interpreter, Value};
use std::sync::atomic::Ordering;

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    let Value::Object(fields) = value else {
        panic!("expected object: {value:?}")
    };
    fields.get(key).unwrap()
}

#[test]
fn script_discovers_and_invokes_existing_selection_description() {
    let service = DeclarationLanguage::default();
    let mut buffers = ScratchBuffers::default();
    buffers.create("example.declarations", SOURCE);
    let target = buffers.selected().unwrap();
    let before = (
        target.source_id(),
        target.version(),
        target.text().to_owned(),
        target.is_dirty(),
    );
    let mut controller = SourceEditController::default();
    let mut source_policy_calls = 0;
    {
        let provider = SemanticToolProvider::new(
            &service,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(TextRange::new(Position::new(0, 0), Position::new(0, 15))),
            |authorization| {
                source_policy_calls += 1;
                assert_eq!(authorization.source.text, SOURCE);
                assert_eq!(authorization.source.version, before.1);
                assert_eq!(authorization.arguments, &serde_json::json!({}));
                true
            },
        )
        .unwrap();
        let session = AgentSession::builder().provider(provider).build().unwrap();
        let catalog = session.catalog();
        assert_eq!(catalog.tools.len(), 1);
        assert_eq!(catalog.tools[0].definition.effect, ToolEffect::Read);
        let expected_metadata = catalog.tools[0].metadata.clone().unwrap();
        let mut host = scope_script::semantic::SemanticHost::new(
            session,
            catalog.tools[0].definition.name.clone(),
            target.text(),
            target.version(),
            Some(true),
        );
        let mut interpreter = Interpreter::new();
        let result = interpreter
            .eval(
                r#"
                let operations = scope.operations();
                let operation = operations[0];
                print(operation);
                scope.invoke(operation.provider, operation.action, {})
            "#,
                &mut host,
            )
            .unwrap();
        let discovered = interpreter.take_output();
        assert_eq!(discovered.len(), 1);
        assert_eq!(field(&discovered[0], "provider").as_str(), Some("semantic"));
        assert_eq!(
            field(&discovered[0], "action").as_str(),
            Some(catalog.tools[0].definition.name.as_str())
        );
        assert_eq!(field(&discovered[0], "metadata"), &expected_metadata);
        assert_eq!(
            field(&expected_metadata, "target").as_str(),
            Some("Selection")
        );
        assert_eq!(
            field(&expected_metadata, "purpose").as_str(),
            Some("Exploration")
        );
        assert_eq!(field(&result, "status").as_str(), Some("read_only"));
        let value = field(&result, "value");
        assert_eq!(field(value, "name").as_str(), Some("answer"));
        assert_eq!(
            field(value, "kind").as_str(),
            Some("integer_literal_binding")
        );
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
        assert_eq!(field(field(value, "span"), "line"), &Value::Integer(0));
    }
    assert_eq!(source_policy_calls, 1);
    assert_eq!(service.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        (
            target.source_id(),
            target.version(),
            target.text().to_owned(),
            target.is_dirty()
        ),
        before
    );
    assert_eq!(controller.list().count(), 0);
}

#[test]
fn semantic_boundaries() {
    use scope_script::{Host, semantic::SemanticHost};
    for case in [
        "unavailable",
        "arguments",
        "stale",
        "missing",
        "denied",
        "source_denied",
    ] {
        let service = DeclarationLanguage::default();
        let mut buffers = ScratchBuffers::default();
        buffers.create("example.declarations", SOURCE);
        let target = buffers.selected().unwrap();
        let before = (
            target.source_id(),
            target.version(),
            target.text().to_owned(),
            target.is_dirty(),
        );
        let mut controller = SourceEditController::default();
        {
            let provider = SemanticToolProvider::new(
                &service,
                target,
                &mut controller,
                PermittedEditScope::Document(target.source_id()),
                Some(TextRange::new(Position::new(0, 0), Position::new(0, 15))),
                |_| case != "source_denied",
            )
            .unwrap();
            let session = AgentSession::builder().provider(provider).build().unwrap();
            let action = if case == "unavailable" {
                "language.missing.inspect"
            } else {
                "language.example.declarations.describe_selection"
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
            let discovered = host.operations().unwrap();
            if case == "unavailable" {
                assert_eq!(discovered, Value::Array(vec![]));
            } else {
                assert!(matches!(discovered, Value::Array(ref entries) if entries.len() == 1));
            }
            let arguments = if case == "arguments" {
                Value::object([("unexpected", true.into())])
            } else {
                Value::object([] as [(&str, Value); 0])
            };
            let result = host.invoke("semantic", action, arguments);
            match case {
                "unavailable" | "missing" | "denied" => assert!(result.is_err(), "{case}"),
                _ => assert_eq!(
                    field(&result.unwrap(), "status").as_str(),
                    Some(match case {
                        "arguments" => "invalid_arguments",
                        "stale" => "revision_conflict",
                        "source_denied" => "unauthorized",
                        _ => unreachable!(),
                    })
                ),
            }
            assert!(
                host.invoke("semantic", "language.other.inspect", Value::Null)
                    .is_err()
            );
        }
        assert_eq!(service.calls.load(Ordering::SeqCst), 0, "{case}");
        assert_eq!(
            (
                target.source_id(),
                target.version(),
                target.text().to_owned(),
                target.is_dirty()
            ),
            before
        );
        assert_eq!(controller.list().count(), 0);
    }
}
