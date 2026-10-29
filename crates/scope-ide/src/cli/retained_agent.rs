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

impl RetainedScratchAgentRun<'_> {
    /// Compact presentation derived only from complete retained feedback records.
    /// Never displays arguments, returned values, prompts, or model claims.
    pub fn activity_summary(&self) -> String {
        let mut text = format!(
            "Agent activity: {:?}, committed revision {} — {:?}\nDecisions: {}; tool attempts: {}.\n",
            self.source.id,
            self.source.version,
            self.run.stop,
            self.run.decisions,
            self.run.tool_calls,
        );
        let mut records = 0;
        for line in self.trace_jsonl.split(|byte| *byte == b'\n') {
            let Ok(event) = serde_json::from_slice::<serde_json::Value>(line) else {
                continue;
            };
            if event["event"] != "feedback" || event["envelope"]["kind"] != "tool_result" {
                continue;
            }
            let feedback = &event["envelope"];
            let Some(name) = feedback["name"].as_str() else {
                continue;
            };
            // Tool names originate in model decisions, so bound and escape them.
            let name: String = name
                .chars()
                .take(120)
                .flat_map(char::escape_default)
                .collect();
            let status = if feedback["ok"] == true {
                "returned successfully"
            } else {
                "failed or denied (not evidence of execution)"
            };
            let supplied = if event["retained"] == true {
                "feedback supplied to model context"
            } else {
                "feedback not supplied to model context"
            };
            text.push_str(&format!("- Tool attempt {name}: {status}; {supplied}.\n"));
            records += 1;
        }
        if records == 0 {
            text.push_str("No tool-result records retained; no semantic operations inferred.\n");
        }
        if self.trace_truncated {
            text.push_str("Trace incomplete: operation summary may omit evidence.\n");
        }
        text.push_str("Runtime diagnostic/evaluation attribution is not inferred from tool names; retained workbench diagnostics are not agent evidence.\nModel conclusion follows separately; it is not an execution record.\n");
        text
    }
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
        if let Some(sink) = &mut self.sink
            && let Err(error) = sink.write_all(&bytes[..count])
        {
            self.truncated = true;
            return Err(error);
        }
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(sink) = &mut self.sink
            && let Err(error) = sink.flush()
        {
            self.truncated = true;
            return Err(error);
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

#[cfg(test)]
mod activity_summary_tests {
    use super::*;
    use crate::{
        agent::AgentStop,
        scratch::ScratchBuffers,
        text::{Position, TextEdit},
    };
    use serde_json::{Value, json};

    fn outcome<'a>(source: &'a ScratchBuffer, events: &[Value]) -> RetainedScratchAgentRun<'a> {
        let mut trace_jsonl = Vec::new();
        for event in events {
            serde_json::to_writer(&mut trace_jsonl, event).unwrap();
            trace_jsonl.push(b'\n');
        }
        RetainedScratchAgentRun {
            run: AgentRun {
                content: "MODEL_CLAIM: evaluation completed without diagnostics".into(),
                stop: AgentStop::Completed,
                decisions: 3,
                tool_calls: 1,
                malformed: 0,
            },
            source: source.snapshot(),
            trace_jsonl,
            trace_truncated: false,
        }
    }

    fn feedback(name: &str, ok: bool, retained: bool) -> Value {
        json!({
            "event": "feedback", "retained": retained,
            "envelope": {"kind": "tool_result", "call": 1, "name": name,
                "ok": ok, "value": "PRIVATE_RETURN_VALUE", "error": "PRIVATE_ERROR"}
        })
    }

    const DISCLAIMERS: &str = "Runtime diagnostic/evaluation attribution is not inferred from tool names; retained workbench diagnostics are not agent evidence.\nModel conclusion follows separately; it is not an execution record.\n";
    const NO_OPERATIONS: &str =
        "No tool-result records retained; no semantic operations inferred.\n";

    #[test]
    fn complete_feedback_is_evidence_not_advertising_requests_or_model_prose() {
        let mut sources = ScratchBuffers::default();
        sources.create("test", "PRIVATE_SOURCE");
        let source = sources.selected().unwrap();
        let run = outcome(
            source,
            &[
                json!({"event": "request", "tools": [{"name": "ADVERTISED_ONLY"}],
                "messages": [{"role": "user", "content": "REQUEST_ONLY"}]}),
                json!({"event": "response", "raw": "MODEL_PROSE: ran PROSE_ONLY"}),
                json!({"event": "decision", "accepted": true, "kind": "tool", "name": "DECISION_ONLY"}),
                feedback("language.test.evaluate", true, true),
                json!({"event": "outcome", "content": "OUTCOME_PROSE"}),
            ],
        );
        assert_eq!(
            run.activity_summary(),
            format!(
                "Agent activity: {:?}, committed revision 0 — Completed\nDecisions: 3; tool attempts: 1.\n- Tool attempt language.test.evaluate: returned successfully; feedback supplied to model context.\n{DISCLAIMERS}",
                source.snapshot().id,
            )
        );
    }

    #[test]
    fn no_operations_are_inferred_from_non_feedback_or_wrong_envelopes() {
        let mut sources = ScratchBuffers::default();
        sources.create("test", "source");
        let mut impostor = feedback("NOT_FEEDBACK", true, true);
        impostor["event"] = json!("request");
        let run = outcome(
            sources.selected().unwrap(),
            &[
                impostor,
                json!({"event": "feedback", "retained": true,
                "envelope": {"kind": "protocol_error", "name": "NOT_A_RESULT"}}),
                json!({"event": "feedback", "envelope": {"kind": "tool_result", "ok": true}}),
            ],
        );
        let summary = run.activity_summary();
        assert!(summary.ends_with(&format!("{NO_OPERATIONS}{DISCLAIMERS}")));
        assert!(!summary.contains("- Tool attempt"));
        let empty = outcome(sources.selected().unwrap(), &[]);
        assert!(empty.activity_summary().contains(NO_OPERATIONS));
    }

    #[test]
    fn truncated_trace_keeps_complete_feedback_and_ignores_partial_record() {
        let mut sources = ScratchBuffers::default();
        sources.create("test", "source");
        let mut run = outcome(
            sources.selected().unwrap(),
            &[feedback("complete", true, true)],
        );
        let partial = br#"{"event":"feedback","retained":true,"envelope":{"kind":"tool_result","name":"PARTIAL_ONLY","ok":true"#;
        run.trace_jsonl.extend_from_slice(partial);
        run.trace_truncated = true;
        let summary = run.activity_summary();
        assert!(summary.contains("- Tool attempt complete: returned successfully;"));
        assert!(!summary.contains("PARTIAL_ONLY"));
        assert!(!summary.contains(NO_OPERATIONS));
        assert!(summary.ends_with(&format!(
            "Trace incomplete: operation summary may omit evidence.\n{DISCLAIMERS}"
        )));
        run.trace_jsonl = partial.to_vec();
        let summary = run.activity_summary();
        assert!(summary.contains(NO_OPERATIONS));
        assert!(!summary.contains("- Tool attempt"));
    }

    #[test]
    fn successful_feedback_not_retained_in_model_context_is_still_reported() {
        let mut sources = ScratchBuffers::default();
        sources.create("test", "source");
        let run = outcome(
            sources.selected().unwrap(),
            &[feedback("inspect", true, false)],
        );
        let summary = run.activity_summary();
        assert!(summary.contains("- Tool attempt inspect: returned successfully; feedback not supplied to model context.\n"));
        assert!(!summary.contains(NO_OPERATIONS));
        assert!(!summary.contains("Trace incomplete"));
        assert!(!summary.contains("PRIVATE_RETURN_VALUE"));
    }

    #[test]
    fn failed_and_denied_feedback_never_claims_execution() {
        let mut sources = ScratchBuffers::default();
        sources.create("test", "source");
        let mut failed = feedback("failed", false, true);
        failed["envelope"]["error"] = json!("provider failed: PRIVATE_ERROR");
        let mut denied = feedback("denied", false, false);
        denied["envelope"]["error"] = json!("authorization denied: PRIVATE_ERROR");
        let run = outcome(sources.selected().unwrap(), &[failed, denied]);
        let summary = run.activity_summary();
        assert!(summary.contains("- Tool attempt failed: failed or denied (not evidence of execution); feedback supplied to model context.\n"));
        assert!(summary.contains("- Tool attempt denied: failed or denied (not evidence of execution); feedback not supplied to model context.\n"));
        assert!(!summary.contains("returned successfully"));
        assert!(!summary.contains("PRIVATE_ERROR"));
    }

    #[test]
    fn attribution_uses_actual_snapshot_identity_and_committed_revision() {
        let mut sources = ScratchBuffers::default();
        let id = sources.create("test", "original");
        sources
            .selected_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), "edited "))
            .unwrap();
        sources.create("test", "different selected source");
        let source = sources.get(id).unwrap();
        assert_eq!(source.version(), 1);
        let run = outcome(
            source,
            &[json!({"event": "source_binding", "source_id": "FORGED_ID", "version": 999})],
        );
        assert_eq!(
            run.activity_summary(),
            format!(
                "Agent activity: {:?}, committed revision 1 — Completed\nDecisions: 3; tool attempts: 1.\n{NO_OPERATIONS}{DISCLAIMERS}",
                source.snapshot().id,
            )
        );
    }

    #[test]
    fn budget_stops_and_counts_are_not_presented_as_completion() {
        let mut sources = ScratchBuffers::default();
        sources.create("test", "source");
        for stop in [
            AgentStop::DecisionBudget,
            AgentStop::ToolBudget,
            AgentStop::MalformedBudget,
            AgentStop::MessageBudget,
            AgentStop::ConversationBudget,
        ] {
            let mut run = outcome(sources.selected().unwrap(), &[]);
            let expected_stop = format!("{stop:?}");
            run.run.stop = stop;
            run.run.decisions = 7;
            run.run.tool_calls = 4;
            let summary = run.activity_summary();
            assert!(summary.contains(&format!(
                " — {expected_stop}\nDecisions: 7; tool attempts: 4.\n"
            )));
            assert!(summary.contains(NO_OPERATIONS));
            assert!(!summary.contains("Completed"));
            assert!(!summary.contains("MODEL_CLAIM"));
        }
    }

    #[test]
    fn model_controlled_names_are_bounded_and_escaped() {
        let mut sources = ScratchBuffers::default();
        sources.create("test", "source");
        let name = format!("\n\"\\{}UNRETAINED_SUFFIX", "x".repeat(117));
        let run = outcome(sources.selected().unwrap(), &[feedback(&name, true, true)]);
        let summary = run.activity_summary();
        assert!(summary.contains(&format!(
            "- Tool attempt \\n\\\"\\\\{}: returned successfully;",
            "x".repeat(117)
        )));
        assert!(!summary.contains("UNRETAINED_SUFFIX"));
        assert_eq!(
            summary
                .lines()
                .filter(|line| line.starts_with("- Tool attempt "))
                .count(),
            1
        );
    }
}
