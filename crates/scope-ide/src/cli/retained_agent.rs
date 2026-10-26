//! Public, read-only semantic agent runs against a retained scratch buffer.
//!
//! No App, workspace, filesystem, save, or source mutation capability is supplied.
//! Language services and models are trusted host implementations, not sandboxed code.
use crate::{
    agent::{
        AgentError, AgentLimits, AgentRun, DecisionModel, ProtocolTrace, read_only_policy,
        run_session_traced,
    },
    scratch::ScratchBuffer,
    source_edit::{PermittedEditScope, SourceEditController, SourceSnapshot, SourceTarget},
    tool::{
        AgentSession, ToolCall, ToolDefinition, ToolError, ToolProvider, ToolResult, ToolValue,
        semantic::{SemanticAuthorization, SemanticToolProvider},
    },
};
use scope_language::{
    LanguageService, SemanticOperationDescription, SemanticOperationEffect,
    SemanticTargetRequirement,
};

/// Per-run host authority. IDs are language-local; an empty allowlist grants nothing.
///
/// Authorization is required for every eligible invocation, including candidate
/// evaluation declared as Read. Discovery and model output never grant authority.
pub struct RetainedScratchAgentConfig<'a> {
    /// Optional sensitive JSONL sink. Receives the same bounded bytes as capture,
    /// including events preceding a model error. Sink failures disable tracing.
    pub trace: Option<&'a mut dyn std::io::Write>,
    pub allowed_operation_ids: &'a [String],
    pub authorize: &'a mut dyn for<'b> FnMut(SemanticAuthorization<'b>) -> bool,
}

/// The unmodified agent outcome and its exact, host-accessible source attribution.
///
/// The snapshot borrows the original retained buffer, not a cloned document or a
/// disk snapshot. Budget stops remain successful returns with their original stop
/// reason and counts; callers must inspect `run.stop` before claiming completion.
#[derive(Debug)]
pub struct RetainedScratchAgentRun<'a> {
    pub run: AgentRun,
    pub source: SourceSnapshot<'a>,
    /// Sensitive protocol capture, bounded by `limits.max_conversation_bytes`.
    /// When truncated, the final JSONL record may be incomplete.
    pub trace_jsonl: Vec<u8>,
    /// Capture stopped at the byte cap or because the host sink failed.
    pub trace_truncated: bool,
}

/// Run only allowlisted Read/Document semantic operations on one retained buffer.
///
/// The immutable source borrow pins identity, revision, language, and exact text
/// throughout the run. A mismatched service language fails before asking the model.
/// Model-facing calls contain only registered language arguments; the semantic
/// provider owns source preconditions and validates arguments and results.
/// No generic tools, writes, proposals, or fallback source are exposed. Existing
/// session limits, feedback, trace behavior, and stop accounting are preserved.
pub fn run_retained_scratch_agent<'a>(
    source: &'a ScratchBuffer,
    service: &dyn LanguageService,
    model: &mut (impl DecisionModel + ?Sized),
    request: &str,
    limits: AgentLimits,
    config: RetainedScratchAgentConfig<'_>,
) -> Result<RetainedScratchAgentRun<'a>, AgentError> {
    let snapshot = source.snapshot();
    if snapshot.language != service.language_id() {
        return Err(AgentError::Tool(ToolError::Denied {
            tool: "language.semantic: scratch language does not match service".into(),
        }));
    }
    let operations: Vec<_> = service
        .semantic_operations()
        .into_iter()
        .filter(|operation| eligible(operation, config.allowed_operation_ids))
        .collect();
    let mut controller = SourceEditController::default();
    let provider = SemanticToolProvider::new(
        service,
        source,
        &mut controller,
        PermittedEditScope::Document(snapshot.id),
        None,
        |authorization: SemanticAuthorization<'_>| {
            operations
                .iter()
                .any(|operation| operation == authorization.operation)
                && authorization.source.id == snapshot.id
                && authorization.source.version == snapshot.version
                && authorization.source.language == snapshot.language
                && authorization.source.text == snapshot.text
                && (config.authorize)(authorization)
        },
    )
    .map_err(AgentError::Tool)?
    .with_projected_arguments()
    .map_err(AgentError::Tool)?;
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
    let mut session = AgentSession::builder()
        .provider(AllowedTools { provider, tools })
        .build()
        .map_err(AgentError::Tool)?;
    let mut capture = BoundedTrace {
        bytes: Vec::new(),
        limit: limits.max_conversation_bytes,
        truncated: false,
        sink: config.trace,
    };
    let run = {
        let mut trace = ProtocolTrace::new(&mut capture);
        trace.record(&serde_json::json!({
            "event": "source_binding",
            "source_id": format!("{:?}", snapshot.id),
            "version": snapshot.version,
            "language": snapshot.language,
        }));
        run_session_traced(
            model,
            &mut session,
            request,
            limits,
            read_only_policy,
            &mut trace,
        )?
    };
    Ok(RetainedScratchAgentRun {
        run,
        source: snapshot,
        trace_jsonl: capture.bytes,
        trace_truncated: capture.truncated,
    })
}

// Streaming avoids allocating an unbounded serialized event or line buffer.
// ProtocolTrace latches the first error, so no further bytes follow truncation.
struct BoundedTrace<'a> {
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
    sink: Option<&'a mut dyn std::io::Write>,
}
impl std::io::Write for BoundedTrace<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let count = bytes.len().min(self.limit.saturating_sub(self.bytes.len()));
        if count == 0 {
            self.truncated = true;
            return Err(std::io::Error::other(
                "retained agent trace byte limit reached",
            ));
        }
        self.bytes.extend_from_slice(&bytes[..count]);
        if let Some(sink) = &mut self.sink {
            if let Err(error) = sink.write_all(&bytes[..count]) {
                self.truncated = true;
                return Err(error);
            }
        }
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(sink) = &mut self.sink {
            if let Err(error) = sink.flush() {
                self.truncated = true;
                return Err(error);
            }
        }
        Ok(())
    }
}

fn eligible(operation: &SemanticOperationDescription, ids: &[String]) -> bool {
    operation.effect == SemanticOperationEffect::Read
        && operation.target == SemanticTargetRequirement::Document
        && ids.contains(&operation.id)
}

struct AllowedTools<P> {
    provider: P,
    tools: Vec<ToolDefinition>,
}
impl<P: ToolProvider> ToolProvider for AllowedTools<P> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_records_events_and_forwards_identical_bytes() {
        let mut sink = Vec::new();
        let mut capture = BoundedTrace {
            bytes: Vec::new(),
            limit: 1024,
            truncated: false,
            sink: Some(&mut sink),
        };
        {
            let mut trace = ProtocolTrace::new(&mut capture);
            trace.record(&serde_json::json!({"event":"source_binding", "version":3}));
            trace.record_presentation("exact source é");
            assert!(!trace.failed());
        }
        assert!(!capture.truncated);
        let bytes = capture.bytes;
        assert_eq!(bytes, sink);
        let events: Vec<serde_json::Value> = bytes
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(events[0]["event"], "source_binding");
        assert_eq!(events[1]["event"], "presentation");
    }

    #[test]
    fn trace_cap_latches_failure_without_unbounded_capture() {
        let mut capture = BoundedTrace {
            bytes: Vec::new(),
            limit: 7,
            truncated: false,
            sink: None,
        };
        {
            let mut trace = ProtocolTrace::new(&mut capture);
            trace.record_presentation("larger than cap");
            assert!(trace.failed());
            trace.record_presentation("not captured");
        }
        assert_eq!(capture.bytes.len(), 7);
        assert!(capture.truncated);
    }

    #[test]
    fn eligibility_requires_all_three_host_conditions() {
        let mut operation = SemanticOperationDescription {
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
    fn hidden_tools_are_neither_discovered_nor_dispatched() {
        struct NeverDispatch;
        impl ToolProvider for NeverDispatch {
            fn tools(&self) -> &[ToolDefinition] {
                &[]
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                panic!("hidden tool reached provider")
            }
        }
        let mut provider = AllowedTools {
            provider: NeverDispatch,
            tools: vec![],
        };
        assert!(provider.tools().is_empty());
        assert!(provider.metadata("hidden").is_none());
        assert!(provider.output_description("hidden").is_none());
        assert!(matches!(
            provider.invoke(ToolCall::new("hidden")),
            Err(ToolError::Denied { .. })
        ));
    }
}
