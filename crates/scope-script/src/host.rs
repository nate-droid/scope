//! Downstream projection of the existing registry; no registration or dispatch here.
use crate::{ErrorKind, Host, Value};
use scope_ide::{
    document::session::{DocumentSession, Revision},
    tool::{ToolEffect, operations::*},
};

/// Host-owned document and expected token remain inaccessible to script code.
/// Missing policy denies invocation. This experimental adapter permits reads only.
pub struct RegistryHost<'a> {
    pub registry: &'a OperationRegistry,
    pub session: &'a mut DocumentSession,
    pub expected: Revision,
    pub policy: Option<&'a dyn Fn(OperationAuthorization<'_>) -> bool>,
    pub results: Vec<OperationResult>,
}
impl Host for RegistryHost<'_> {
    fn operation_names(&self) -> Vec<(String, String)> {
        self.registry
            .descriptors()
            .map(|(id, _)| (id.provider_id, id.action_id))
            .collect()
    }

    fn operations(&mut self) -> Result<Value, ErrorKind> {
        Ok(Value::Array(
            self.registry
                .descriptors()
                .map(|(id, d)| {
                    Value::object([
                        ("provider", Value::String(id.provider_id)),
                        ("action", Value::String(id.action_id)),
                        ("description", Value::String(d.description.clone())),
                        ("input", schema(&d.input)),
                        ("output", schema(&d.output)),
                        ("effect", Value::String(format!("{:?}", d.effect))),
                        ("execution", Value::String(format!("{:?}", d.execution))),
                        (
                            "authorization",
                            Value::Array(
                                d.authorization.iter().cloned().map(Value::String).collect(),
                            ),
                        ),
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
        let Value::Object(arguments) = arguments else {
            return Err(
                OperationError::InvalidInput(OperationValidation::InputMustBeObject).into(),
            );
        };
        if self.results.len() >= 64 {
            return Err(ErrorKind::LimitExceeded);
        }
        let result = self.registry.invoke(
            self.session,
            &self.expected,
            &OperationCall {
                operation: OperationId {
                    provider_id: provider.into(),
                    action_id: action.into(),
                },
                arguments,
            },
            |context| {
                context.descriptor.effect == ToolEffect::Read
                    && self.policy.is_some_and(|policy| policy(context))
            },
        )?;
        let retained = self.results.iter().try_fold(0usize, |total, previous| {
            Ok::<_, ErrorKind>(total + crate::value_size(&previous.value, 0)?)
        })?;
        if crate::value_size(&result.value, 0)? > 1_000_000usize.saturating_sub(retained) {
            return Err(ErrorKind::LimitExceeded);
        }
        let value = result.value.clone();
        self.results.push(result);
        Ok(value)
    }
}
fn schema(s: &OperationSchema) -> Value {
    match s {
        OperationSchema::Array(item) => {
            Value::object([("kind", "array".into()), ("items", schema(item))])
        }
        OperationSchema::Object { fields, required } => Value::object([
            ("kind", "object".into()),
            (
                "fields",
                Value::Object(fields.iter().map(|(k, v)| (k.clone(), schema(v))).collect()),
            ),
            (
                "required",
                Value::Array(required.iter().cloned().map(Value::String).collect()),
            ),
        ]),
        other => Value::String(
            match other {
                OperationSchema::Null => "null",
                OperationSchema::Boolean => "boolean",
                OperationSchema::Integer => "integer",
                OperationSchema::String => "string",
                _ => unreachable!(),
            }
            .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Interpreter, completion::ScriptCompletionProvider};
    use scope_ide::{
        document::{DocumentId, session::DocumentSnapshot},
        tool::ToolArguments,
        workspace::Workspace,
    };
    use scope_terminal::CompletionProvider;

    struct MetadataProvider(Vec<OperationDescriptor>);
    impl OperationProvider for MetadataProvider {
        fn provider_id(&self) -> &str {
            "registered_provider"
        }
        fn descriptors(&self) -> &[OperationDescriptor] {
            &self.0
        }
        fn invoke(
            &self,
            _: &str,
            _: &DocumentSnapshot,
            _: &ToolArguments,
        ) -> Result<Value, OperationFailure> {
            panic!("metadata discovery must not dispatch")
        }
    }

    #[test]
    fn completion_projects_authoritative_registry_descriptors_without_authority() {
        let mut registry = OperationRegistry::new(OperationLimits::default());
        registry
            .register(Box::new(MetadataProvider(
                ["custom_read", "custom_write"]
                    .into_iter()
                    .map(|action| OperationDescriptor {
                        action_id: action.into(),
                        description: "metadata only".into(),
                        input: OperationSchema::Object {
                            fields: Default::default(),
                            required: Default::default(),
                        },
                        output: OperationSchema::Null,
                        effect: ToolEffect::Read,
                        execution: OperationExecution::Immediate,
                        authorization: Default::default(),
                    })
                    .collect(),
            )))
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("source.txt"), "unchanged").unwrap();
        let mut session = DocumentSession::open(
            Workspace::read_only(directory.path().to_string_lossy()).unwrap(),
            DocumentId::new(1),
            "source.txt",
        )
        .unwrap();
        let revision = session.snapshot().unwrap().revision;
        let host = RegistryHost {
            registry: &registry,
            session: &mut session,
            expected: revision.clone(),
            policy: Some(&|_| panic!("completion must not request authorization")),
            results: Vec::new(),
        };
        let expected: Vec<_> = registry
            .descriptors()
            .map(|(id, _)| (id.provider_id, id.action_id))
            .collect();
        assert_eq!(host.operation_names(), expected);
        assert_eq!(expected.len(), 2);
        let interpreter = Interpreter::new();
        let mut completion = ScriptCompletionProvider::new(&interpreter, &host);
        let source = "scope.invoke(\"";
        let providers = completion.complete(source, source.len());
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].insertion, "\"registered_provider\"");
        let source = "scope.invoke(\"registered_provider\", \"";
        let actions = completion.complete(source, source.len());
        assert_eq!(
            actions
                .iter()
                .map(|c| c.insertion.as_str())
                .collect::<Vec<_>>(),
            ["\"custom_read\"", "\"custom_write\""]
        );
        assert!(host.results.is_empty());
        assert_eq!(host.session.snapshot().unwrap().revision, revision);
    }
}

/// No installed providers or authority. Useful for pure evaluation and the CLI.
pub struct NoHost;
impl Host for NoHost {
    fn operations(&mut self) -> Result<Value, ErrorKind> {
        Ok(Value::Array(vec![]))
    }
    fn invoke(&mut self, _: &str, _: &str, _: Value) -> Result<Value, ErrorKind> {
        Err(OperationError::UnknownOperation.into())
    }
}
