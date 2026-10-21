//! Explicit host opt-in for document-bound, read-only terminal semantics.
use super::{App, InteractiveSession};
use crate::{
    agent::{AgentError, AgentLimits, AgentResponse, AgentStop, DecisionModel},
    source_edit::{DocumentSource, PermittedEditScope, SourceEditController, SourceTarget},
    tool::{
        AgentSession, ToolCall, ToolDefinition, ToolError, ToolProvider, ToolResult, ToolValue,
        semantic::{SemanticAuthorization, SemanticToolProvider},
    },
};
use scope_language::{SemanticOperationEffect, SemanticTargetRequirement};

/// Per-run host authority, never derived from model output or discovery metadata.
/// IDs are language-local operation IDs. An empty list grants nothing.
/// Candidate evaluation is not excluded: hosts must explicitly decide whether to
/// permit it in `authorize`, even when an operation declares a Read effect.
pub struct TerminalSemanticConfig<'a> {
    pub allowed_operation_ids: &'a [String],
    pub authorize: &'a mut dyn for<'b> FnMut(SemanticAuthorization<'b>) -> bool,
}

/// Run a terminal agent with caller-owned limits and optional semantic authority.
///
/// `None` preserves the existing generic CLI surface. `Some` replaces that surface
/// with only allowlisted Read/Document semantic tools and optional interactive
/// inspection. It never exposes generic writes, proposals, or filesystem tools.
/// The open in-memory document is borrowed for the whole run; no disk snapshot or
/// cloned document is substituted. Missing documents fail closed (no fallback).
pub fn run_terminal_agent_model(
    app: &mut App,
    interactive: Option<&InteractiveSession>,
    model: &mut impl DecisionModel,
    request: &str,
    limits: AgentLimits,
    config: Option<TerminalSemanticConfig<'_>>,
) -> Result<AgentResponse, AgentError> {
    run_terminal_agent_mode(app, interactive, model, request, limits, config, false)
}

/// Opt in to host-bound semantic arguments. The model supplies only registered
/// language arguments; the original App document remains the invocation baseline.
pub fn run_terminal_agent_model_with_projected_arguments(
    app: &mut App,
    interactive: Option<&InteractiveSession>,
    model: &mut impl DecisionModel,
    request: &str,
    limits: AgentLimits,
    config: TerminalSemanticConfig<'_>,
) -> Result<AgentResponse, AgentError> {
    run_terminal_agent_mode(app, interactive, model, request, limits, Some(config), true)
}

fn run_terminal_agent_mode(
    app: &mut App,
    interactive: Option<&InteractiveSession>,
    model: &mut impl DecisionModel,
    request: &str,
    limits: AgentLimits,
    config: Option<TerminalSemanticConfig<'_>>,
    projected: bool,
) -> Result<AgentResponse, AgentError> {
    let Some(config) = config else {
        return super::run_default_terminal_agent_model(model, app, interactive, request, limits);
    };
    let context = crate::agent::AgentContext::from_app(app).render();
    app.open_document().ok_or_else(|| {
        AgentError::Tool(ToolError::Denied {
            tool: "language.semantic: no open document".into(),
        })
    })?;
    let service = app.open_language().ok_or_else(|| {
        AgentError::Tool(ToolError::Denied {
            tool: "language.semantic: no matching registered language service".into(),
        })
    })?;
    let document = app.open_document_mut().ok_or_else(|| {
        AgentError::Tool(ToolError::Denied {
            tool: "language.semantic: no open document".into(),
        })
    })?;
    let target = DocumentSource::new(document, service.language_id());
    let snapshot = target.snapshot();
    let mut controller = SourceEditController::default();
    let operations: Vec<_> = service
        .semantic_operations()
        .into_iter()
        .filter(|operation| eligible(operation, config.allowed_operation_ids))
        .collect();
    let provider = SemanticToolProvider::new(
        service,
        &target,
        &mut controller,
        PermittedEditScope::Document(snapshot.id),
        None,
        |authorization: SemanticAuthorization<'_>| {
            operations
                .iter()
                .any(|operation| operation == authorization.operation)
                && authorization.source.id == snapshot.id
                && authorization.source.version == snapshot.version
                && authorization.source.text == snapshot.text
                && authorization.source.language == snapshot.language
                && (config.authorize)(authorization)
        },
    )
    .map_err(AgentError::Tool)?;
    // Filter discovery and dispatch, not just the outer session policy. Keep the
    // provider's metadata and schema intact, including candidate-evaluation hints.
    let tools = provider
        .tools()
        .iter()
        .filter(|tool| {
            operations.iter().any(|operation| {
                tool.name == format!("language.{}.{}", service.language_id(), operation.id)
            })
        })
        .cloned()
        .collect();
    let provider = Projection { provider, tools };
    let provider: Box<dyn ToolProvider + '_> = if projected {
        Box::new(LanguageArguments::new(provider, snapshot))
    } else {
        Box::new(provider)
    };
    let mut builder = AgentSession::builder().provider(TerminalProvider(provider));
    if let Some(interactive) = interactive {
        builder = builder.provider(crate::tool::inspection::InteractiveInspectionProvider::new(
            interactive,
        ));
    }
    let mut session = builder.build().map_err(AgentError::Tool)?;
    let request = format!(
        "User request:\n{request}\n\nActive Scope context:\n{context}\n\nHost semantic snapshot:\nexpected_version: {}\nexpected_source:\n{}",
        snapshot.version, snapshot.text,
    );
    let run = crate::agent::run_session(
        model,
        &mut session,
        &request,
        limits,
        crate::agent::read_only_policy,
    )?;
    if run.stop != AgentStop::Completed {
        return Err(AgentError::Stopped(run.stop));
    }
    Ok(AgentResponse {
        content: run.content,
    })
}

fn eligible(operation: &scope_language::SemanticOperationDescription, ids: &[String]) -> bool {
    operation.effect == SemanticOperationEffect::Read
        && operation.target == SemanticTargetRequirement::Document
        && ids.contains(&operation.id)
}

struct Projection<P> {
    provider: P,
    tools: Vec<ToolDefinition>,
}
impl<P: ToolProvider> ToolProvider for Projection<P> {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
    fn provider_name(&self) -> Option<&str> {
        self.provider.provider_name()
    }
    fn metadata(&self, name: &str) -> Option<ToolValue> {
        self.tools
            .iter()
            .any(|tool| tool.name == name)
            .then(|| self.provider.metadata(name))
            .flatten()
    }
    fn output_description(&self, name: &str) -> Option<&str> {
        self.tools
            .iter()
            .any(|tool| tool.name == name)
            .then(|| self.provider.output_description(name))
            .flatten()
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        if !self.tools.iter().any(|tool| tool.name == call.name) {
            return Err(ToolError::Denied { tool: call.name });
        }
        self.provider.invoke(call)
    }
}

struct TerminalProvider<'a>(Box<dyn ToolProvider + 'a>);
impl ToolProvider for TerminalProvider<'_> {
    fn tools(&self) -> &[ToolDefinition] {
        self.0.tools()
    }
    fn metadata(&self, name: &str) -> Option<ToolValue> {
        self.0.metadata(name)
    }
    fn provider_name(&self) -> Option<&str> {
        self.0.provider_name()
    }
    fn output_description(&self, name: &str) -> Option<&str> {
        self.0.output_description(name)
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        self.0.invoke(call)
    }
}

/// Private terminal-only decorator; authority remains in the semantic provider.
struct LanguageArguments<P> {
    provider: P,
    tools: Vec<ToolDefinition>,
    version: u64,
    source: String,
    binding: ToolValue,
}
impl<P: ToolProvider> LanguageArguments<P> {
    fn new(provider: P, snapshot: crate::source_edit::SourceSnapshot<'_>) -> Self {
        use crate::tool::{ToolInputDefinition, ToolInputKind};
        let tools = provider
            .tools()
            .iter()
            .cloned()
            .map(|mut tool| {
                let ToolValue::Object(metadata) =
                    provider.metadata(&tool.name).expect("semantic metadata")
                else {
                    unreachable!()
                };
                let ToolValue::Array(arguments) = &metadata["arguments"] else {
                    unreachable!()
                };
                tool.inputs = arguments
                    .iter()
                    .map(|argument| {
                        let ToolValue::Object(field) = argument else {
                            unreachable!()
                        };
                        let kind = match field["kind"].as_str().expect("argument kind") {
                            "String" => ToolInputKind::String,
                            "Integer" => ToolInputKind::Integer,
                            "Boolean" => ToolInputKind::Boolean,
                            "Object" => ToolInputKind::Object,
                            "Array" => ToolInputKind::Array,
                            _ => unreachable!("registered semantic argument kind"),
                        };
                        ToolInputDefinition {
                            name: field["name"].as_str().unwrap().into(),
                            description: field["description"].as_str().unwrap().into(),
                            kind,
                            required: field["required"] == ToolValue::Boolean(true),
                        }
                    })
                    .collect();
                tool
            })
            .collect();
        Self {
            provider,
            tools,
            version: snapshot.version,
            source: snapshot.text.into(),
            binding: ToolValue::object([
                ("owner", "app-document".into()),
                ("source_id", format!("{:?}", snapshot.id).into()),
                ("original_revision", snapshot.version.to_string().into()),
                ("language", snapshot.language.into()),
                ("role", "invocation_baseline".into()),
            ]),
        }
    }
}
fn argument_schema(metadata: &ToolValue) -> Option<ToolValue> {
    let ToolValue::Object(metadata) = metadata else {
        return None;
    };
    let ToolValue::Object(schema) = metadata.get("invocation_schema")? else {
        return None;
    };
    let ToolValue::Object(properties) = schema.get("properties")? else {
        return None;
    };
    properties.get("arguments").cloned()
}
impl<P: ToolProvider> ToolProvider for LanguageArguments<P> {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
    fn provider_name(&self) -> Option<&str> {
        self.provider.provider_name()
    }
    fn output_description(&self, name: &str) -> Option<&str> {
        self.tools
            .iter()
            .any(|t| t.name == name)
            .then(|| self.provider.output_description(name))
            .flatten()
    }
    fn metadata(&self, name: &str) -> Option<ToolValue> {
        if !self.tools.iter().any(|t| t.name == name) {
            return None;
        }
        let mut metadata = self.provider.metadata(name)?;
        let schema = argument_schema(&metadata)?;
        let ToolValue::Object(fields) = &mut metadata else {
            return None;
        };
        fields.insert("invocation_schema".into(), schema);
        fields.insert("host_binding".into(), self.binding.clone());
        Some(metadata)
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        if !self.tools.iter().any(|t| t.name == call.name) {
            return Err(ToolError::Denied { tool: call.name });
        }
        let schema = argument_schema(
            &self
                .provider
                .metadata(&call.name)
                .expect("semantic metadata"),
        );
        let mut result = self.provider.invoke(ToolCall {
            name: call.name,
            arguments: [
                ("expected_version".into(), self.version.to_string().into()),
                ("expected_source".into(), self.source.clone().into()),
                ("arguments".into(), ToolValue::Object(call.arguments)),
            ]
            .into_iter()
            .collect(),
        })?;
        if let ToolValue::Object(fields) = &mut result.value {
            if fields.get("status").and_then(ToolValue::as_str) == Some("invalid_arguments") {
                if let Some(message) = fields.remove("message") {
                    fields.insert(
                        "message".into(),
                        message
                            .as_str()
                            .unwrap_or("")
                            .split(". Language fields belong inside arguments.")
                            .next()
                            .unwrap_or("")
                            .to_owned()
                            .into(),
                    );
                }
                if let Some(ToolValue::String(message)) = fields.get_mut("message") {
                    message
                        .push_str(". Retry using invocation_schema with language arguments only.");
                }
                fields.insert("retry_guidance".into(), "Submit only language arguments matching invocation_schema; the host owns source preconditions.".into());
                if let Some(schema) = schema {
                    fields.insert("invocation_schema".into(), schema);
                }
            }
            fields.insert("host_binding".into(), self.binding.clone());
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "agent_semantics_tests.rs"]
mod agent_semantics_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_filter_requires_allowlist_read_and_document() {
        let mut operation = scope_language::SemanticOperationDescription {
            id: "inspect".into(),
            description: "Inspect".into(),
            arguments: vec![],
            effect: SemanticOperationEffect::Read,
            target: SemanticTargetRequirement::Document,
            constraints: None,
        };
        assert!(!eligible(&operation, &[]));
        assert!(!eligible(&operation, &["other".into()]));
        assert!(eligible(&operation, &["inspect".into()]));
        operation.effect = SemanticOperationEffect::ProposeChanges;
        assert!(!eligible(&operation, &["inspect".into()]));
    }

    #[test]
    fn projection_rejects_hidden_calls_before_dispatch() {
        struct NeverDispatch;
        impl ToolProvider for NeverDispatch {
            fn tools(&self) -> &[ToolDefinition] {
                &[]
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                panic!("hidden call reached underlying provider")
            }
        }
        let mut projection = Projection {
            provider: NeverDispatch,
            tools: vec![],
        };
        assert!(projection.tools().is_empty());
        assert!(projection.metadata("hidden").is_none());
        assert!(projection.output_description("hidden").is_none());
        assert!(matches!(
            projection.invoke(ToolCall::new("hidden")),
            Err(ToolError::Denied { .. })
        ));
    }
}
