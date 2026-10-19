#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod fixture;

use fixture::{DeclarationLanguage, SOURCE};
use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
    text::{Position, TextRange},
    tool::{AgentSession, ToolCall, ToolEffect, semantic::SemanticToolProvider},
};
use scope_script::{ErrorKind, Host, Interpreter, Value};
use std::sync::atomic::Ordering;

// Test-only projection of the installed session, not a second operation registry.
struct InspectionHost<'a> {
    session: AgentSession<'a>,
    expected_source: String,
    expected_version: String,
    policy_calls: usize,
}

impl Host for InspectionHost<'_> {
    fn operations(&mut self) -> Result<Value, ErrorKind> {
        Ok(Value::Array(
            self.session
                .catalog()
                .tools
                .into_iter()
                .map(|entry| {
                    Value::object([
                        ("provider", Value::from("semantic")),
                        ("action", Value::from(entry.definition.name)),
                        ("description", Value::from(entry.definition.description)),
                        ("metadata", entry.metadata.unwrap_or(Value::Null)),
                    ])
                })
                .collect(),
        ))
    }

    fn invoke(
        &mut self,
        provider: &str,
        action: &str,
        arguments: Value,
    ) -> Result<Value, ErrorKind> {
        if provider != "semantic"
            || !self
                .session
                .catalog()
                .tools
                .iter()
                .any(|entry| entry.definition.name == action)
        {
            return Err(ErrorKind::Runtime("unknown session operation".into()));
        }
        let call = ToolCall::new(action)
            .with_argument("expected_source", self.expected_source.clone().into())
            .with_argument("expected_version", self.expected_version.clone().into())
            .with_argument("arguments", arguments);
        self.session
            .invoke_with_call_policy(call, |definition, call| {
                self.policy_calls += 1;
                assert_eq!(call.name, action);
                assert_eq!(
                    call.arguments["expected_source"].as_str(),
                    Some(self.expected_source.as_str())
                );
                assert_eq!(
                    call.arguments["expected_version"].as_str(),
                    Some(self.expected_version.as_str())
                );
                definition.effect == ToolEffect::Read
            })
            .map(|result| result.value)
            .map_err(ErrorKind::Tool)
    }
}

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
        let mut host = InspectionHost {
            session,
            expected_source: target.text().to_owned(),
            expected_version: target.version().to_string(),
            policy_calls: 0,
        };
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
        assert_eq!(host.policy_calls, 1);
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
