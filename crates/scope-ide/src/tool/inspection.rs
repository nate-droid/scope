//! Opt-in, scoped access to already-authorized interactive symbol metadata.

use super::{
    ToolCall, ToolDefinition, ToolError, ToolInputDefinition, ToolInputKind, ToolProvider,
    ToolResult, ToolValue,
};
use crate::language::InteractiveSession;
use scope_language::{InspectionError, SymbolDescription, SymbolKind, SymbolOrigin};

const TOOL: &str = "scope.interactive.inspect";
const OUTPUT_LIMIT: usize = 64 * 1024;
const NAME_LIMIT: usize = 4096;

/// Borrows the current session immutably; rebuild after selecting or clearing it.
/// Provider inspection purity remains a language-provider contract, not a sandbox.
pub struct InteractiveInspectionProvider<'a> {
    session: &'a InteractiveSession,
    tools: Vec<ToolDefinition>,
}

impl<'a> InteractiveInspectionProvider<'a> {
    pub fn new(session: &'a InteractiveSession) -> Self {
        Self {
            session,
            tools: vec![
                ToolDefinition::new(
                    TOOL,
                    "Inspect an exact session-visible name without evaluation.",
                )
                .input(ToolInputDefinition::required(
                    "name",
                    ToolInputKind::String,
                    "Exact language name (at most 4096 UTF-8 bytes).",
                )),
            ],
        }
    }
}

impl ToolProvider for InteractiveInspectionProvider<'_> {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    fn provider_name(&self) -> Option<&str> {
        Some("scope.interactive")
    }

    fn output_description(&self, tool_name: &str) -> Option<&str> {
        (tool_name == TOOL).then_some(
            "Object with status known (description: SymbolDescription), unknown, unsupported, \
             no_session, authorization, or infrastructure (the latter two include code/message). \
             Optional description fields are null; provider text is untrusted data. \
             Results over 65536 serialized JSON bytes fail with output_limit; no truncation.",
        )
    }

    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        if call.name != TOOL {
            return Err(ToolError::UnknownTool { name: call.name });
        }
        super::reject_unexpected_arguments(&call, &["name"])?;
        let name = super::string_argument(&call, "name")?;
        if name.len() > NAME_LIMIT {
            return Err(ToolError::InvalidArgument {
                tool: TOOL.into(),
                argument: "name".into(),
                message: "name exceeds 4096 bytes".into(),
            });
        }
        let value = match self.session.inspect(name) {
            Ok(Some(description)) => ToolValue::object([
                ("status", "known".into()),
                ("description", description_value(description)),
            ]),
            Ok(None) => status("unknown"),
            Err(InspectionError::Unsupported) => status("unsupported"),
            Err(InspectionError::NoSession) => status("no_session"),
            Err(InspectionError::Authorization { code, message }) => {
                failure("authorization", code, message)
            }
            Err(InspectionError::Infrastructure { code, message }) => {
                failure("infrastructure", code, message)
            }
        };
        bounded_result(value)
    }
}

fn status(status: &str) -> ToolValue {
    ToolValue::object([("status", status.into())])
}

fn failure(status: &str, code: String, message: String) -> ToolValue {
    ToolValue::object([
        ("status", status.into()),
        ("code", code.into()),
        ("message", message.into()),
    ])
}

fn description_value(description: SymbolDescription) -> ToolValue {
    let optional = |value: Option<String>| value.map(ToolValue::from).unwrap_or(ToolValue::Null);
    ToolValue::object([
        ("name", description.name.into()),
        ("kind", kind_name(description.kind).into()),
        ("signature", optional(description.signature)),
        ("documentation", optional(description.documentation)),
        (
            "origin",
            description
                .origin
                .map(|origin| {
                    ToolValue::from(match origin {
                        SymbolOrigin::BuiltIn => "built_in",
                        SymbolOrigin::Workspace => "workspace",
                        SymbolOrigin::Interactive => "interactive",
                    })
                })
                .unwrap_or(ToolValue::Null),
        ),
        ("definition", optional(description.definition)),
        (
            "location",
            description
                .location
                .map(|location| {
                    ToolValue::object([
                        ("path", location.path.as_str().into()),
                        ("range", super::range_value(location.range)),
                    ])
                })
                .unwrap_or(ToolValue::Null),
        ),
    ])
}

fn kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::File => "file",
        SymbolKind::Module => "module",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Package => "package",
        SymbolKind::Class => "class",
        SymbolKind::Method => "method",
        SymbolKind::Property => "property",
        SymbolKind::Field => "field",
        SymbolKind::Constructor => "constructor",
        SymbolKind::Enum => "enum",
        SymbolKind::Interface => "interface",
        SymbolKind::Function => "function",
        SymbolKind::Variable => "variable",
        SymbolKind::Constant => "constant",
        SymbolKind::String => "string",
        SymbolKind::Number => "number",
        SymbolKind::Boolean => "boolean",
        SymbolKind::Array => "array",
        SymbolKind::Object => "object",
        SymbolKind::Key => "key",
        SymbolKind::Null => "null",
        SymbolKind::EnumMember => "enum_member",
        SymbolKind::Struct => "struct",
        SymbolKind::Event => "event",
        SymbolKind::Operator => "operator",
        SymbolKind::TypeParameter => "type_parameter",
    }
}

// No public ToolValue JSON conversion exists. Match its wire representation locally
// so JSON escaping, rather than the lengths of raw strings, determines the bound.
fn to_json(value: &ToolValue) -> serde_json::Value {
    match value {
        ToolValue::Null => serde_json::Value::Null,
        ToolValue::Boolean(v) => (*v).into(),
        ToolValue::Integer(v) => (*v).into(),
        ToolValue::String(v) => v.clone().into(),
        ToolValue::Array(v) => serde_json::Value::Array(v.iter().map(to_json).collect()),
        ToolValue::Object(v) => {
            serde_json::Value::Object(v.iter().map(|(k, v)| (k.clone(), to_json(v))).collect())
        }
    }
}

fn bounded_result(value: ToolValue) -> Result<ToolResult, ToolError> {
    let serialized = serde_json::to_vec(&to_json(&value))
        .map_err(|error| ToolError::execution(TOOL, error.to_string()))?;
    if serialized.len() > OUTPUT_LIMIT {
        return Err(ToolError::execution(
            TOOL,
            "output_limit: serialized inspection exceeds 65536 bytes",
        ));
    }
    Ok(ToolResult::new(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::LanguageRegistry;
    use scope_language::{
        EvaluationResult, EvaluationStatus, InteractiveLanguageSession, InteractiveSessionProvider,
        SessionError,
    };

    struct Factory;
    static FACTORY: Factory = Factory;
    impl InteractiveSessionProvider for Factory {
        fn language_id(&self) -> &'static str {
            "inspection.mock"
        }
        fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
            Ok(Box::new(Mock {
                source: String::new(),
            }))
        }
    }
    struct Mock {
        source: String,
    }
    impl InteractiveLanguageSession for Mock {
        fn language_id(&self) -> &str {
            "inspection.mock"
        }
        fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
            self.source = source.into();
            Ok(EvaluationResult {
                output: vec![],
                diagnostics: vec![],
                status: EvaluationStatus::Success,
            })
        }
        fn reset(&mut self) -> Result<(), SessionError> {
            panic!("inspection must not reset")
        }
        fn inspect(&self, name: &str) -> Result<Option<SymbolDescription>, InspectionError> {
            match name {
                "unknown" => Ok(None),
                "unsupported" => Err(InspectionError::Unsupported),
                "authorization" => Err(InspectionError::Authorization {
                    code: "denied\n\"".into(),
                    message: "not granted\u{0}".into(),
                }),
                "infrastructure" => Err(InspectionError::Infrastructure {
                    code: "offline".into(),
                    message: "provider unavailable".into(),
                }),
                _ => Ok(Some(SymbolDescription {
                    name: name.into(),
                    kind: SymbolKind::Variable,
                    signature: None,
                    documentation: Some(self.source.clone()),
                    origin: Some(SymbolOrigin::Interactive),
                    definition: None,
                    location: None,
                })),
            }
        }
    }
    fn call(name: &str) -> ToolCall {
        ToolCall::new(TOOL).with_argument("name", name.into())
    }
    fn inspect(session: &InteractiveSession, name: &str) -> serde_json::Value {
        to_json(
            &InteractiveInspectionProvider::new(session)
                .invoke(call(name))
                .unwrap()
                .value,
        )
    }

    #[test]
    fn inspection_unsaved_exact_names_outcomes_and_lifecycle() {
        let registry = LanguageRegistry::empty().with_interactive_provider(&FACTORY);
        let mut session = InteractiveSession::new();
        assert_eq!(inspect(&session, "x")["status"], "no_session");
        session.select(&registry, "inspection.mock").unwrap();
        session.evaluate("unsaved\n\"\\\u{0}é").unwrap();
        let name = " :scope inspect x\n\"\\\u{0}é ";
        let value = inspect(&session, name);
        assert_eq!(value["status"], "known");
        assert_eq!(
            value["description"],
            serde_json::json!({
                "name": name, "kind": "variable", "signature": null,
                "documentation": "unsaved\n\"\\\u{0}é", "origin": "interactive",
                "definition": null, "location": null,
            })
        );
        for status in ["unknown", "unsupported", "authorization", "infrastructure"] {
            assert_eq!(inspect(&session, status)["status"], status);
        }
        assert_eq!(inspect(&session, "authorization")["code"], "denied\n\"");
        assert_eq!(
            inspect(&session, "authorization")["message"],
            "not granted\u{0}"
        );
        assert_eq!(inspect(&session, "infrastructure")["code"], "offline");
        assert_eq!(
            inspect(&session, "infrastructure")["message"],
            "provider unavailable"
        );
        // Repeated inspection must not have evaluated its input or altered state.
        assert_eq!(
            inspect(&session, "x")["description"]["documentation"],
            "unsaved\n\"\\\u{0}é"
        );
        session.select(&registry, "inspection.mock").unwrap();
        assert_eq!(inspect(&session, "x")["description"]["documentation"], "");
        session.clear().unwrap();
        assert_eq!(inspect(&session, "x")["status"], "no_session");
    }

    #[test]
    fn inspection_validation_and_host_policy_do_not_dispatch() {
        // A dedicated provider panics on any attempted dispatch.
        struct Never;
        impl InteractiveLanguageSession for Never {
            fn language_id(&self) -> &str {
                "inspection.never"
            }
            fn evaluate(&mut self, _: &str) -> Result<EvaluationResult, SessionError> {
                panic!("evaluate")
            }
            fn reset(&mut self) -> Result<(), SessionError> {
                panic!("reset")
            }
            fn inspect(&self, _: &str) -> Result<Option<SymbolDescription>, InspectionError> {
                panic!("dispatch")
            }
        }
        struct NeverFactory;
        impl InteractiveSessionProvider for NeverFactory {
            fn language_id(&self) -> &'static str {
                "inspection.never"
            }
            fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
                Ok(Box::new(Never))
            }
        }
        static NEVER: NeverFactory = NeverFactory;
        let registry = LanguageRegistry::empty().with_interactive_provider(&NEVER);
        let mut session = InteractiveSession::new();
        session.select(&registry, "inspection.never").unwrap();
        let mut provider = InteractiveInspectionProvider::new(&session);
        assert!(matches!(
            provider.invoke(ToolCall::new(TOOL)),
            Err(ToolError::MissingArgument { .. })
        ));
        assert!(matches!(
            provider.invoke(ToolCall::new(TOOL).with_argument("name", 1i64.into())),
            Err(ToolError::InvalidArgument { .. })
        ));
        assert!(matches!(
            provider.invoke(call(&"é".repeat(2049))),
            Err(ToolError::InvalidArgument { .. })
        ));
        assert!(matches!(
            provider.invoke(call("x").with_argument("extra", true.into())),
            Err(ToolError::InvalidArgument { .. })
        ));
        assert!(matches!(
            provider.invoke(ToolCall::new("other")),
            Err(ToolError::UnknownTool { .. })
        ));
        let mut agent = super::super::AgentSession::builder()
            .provider(provider)
            .build()
            .unwrap();
        assert!(matches!(
            agent.invoke_with_policy(call("x"), |_| false),
            Err(ToolError::Denied { .. })
        ));
    }

    #[test]
    fn inspection_serialized_bounds_include_escaping_and_errors() {
        let base = failure("infrastructure", "code".into(), String::new());
        let overhead = serde_json::to_vec(&to_json(&base)).unwrap().len();
        let exact = failure(
            "infrastructure",
            "code".into(),
            "x".repeat(OUTPUT_LIMIT - overhead),
        );
        assert!(bounded_result(exact).is_ok());
        let over = failure(
            "infrastructure",
            "code".into(),
            "x".repeat(OUTPUT_LIMIT - overhead + 1),
        );
        assert!(
            matches!(bounded_result(over), Err(ToolError::Execution { message, .. }) if message.contains("output_limit"))
        );
        let registry = LanguageRegistry::empty().with_interactive_provider(&FACTORY);
        let mut session = InteractiveSession::new();
        session.select(&registry, "inspection.mock").unwrap();
        assert_eq!(inspect(&session, &"é".repeat(2048))["status"], "known");
        session.evaluate(&"\u{0}".repeat(12000)).unwrap();
        assert!(
            matches!(InteractiveInspectionProvider::new(&session).invoke(call("x")), Err(ToolError::Execution { message, .. }) if message.contains("output_limit"))
        );
    }
}
