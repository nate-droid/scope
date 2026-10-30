//! Public, read-only semantic agent runs against a retained scratch buffer.
//!
//! No App, workspace, filesystem, save, or source mutation capability is supplied.
//! Language services and models are trusted host implementations, not sandboxed code.
use crate::{
    agent::{
        AgentCancellationToken, AgentError, AgentEvidence, AgentLimits, AgentRun,
        AgentSourceBinding, AgentSourceIdentity, AgentToolOutcome, AgentTurn, DecisionModel,
        ProtocolTrace, read_only_policy, run_session_traced_observed,
    },
    scratch::ScratchBuffer,
    source_edit::{
        PermittedEditScope, SourceEditController, SourceId, SourceSnapshot, SourceTarget,
    },
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
    evaluation_source_id: scope_language::SourceId,
    /// Sensitive protocol capture, bounded by `limits.max_conversation_bytes`.
    /// When truncated, the final JSONL record may be incomplete.
    pub trace_jsonl: Vec<u8>,
    /// Capture stopped at the byte cap or because the host sink failed.
    pub trace_truncated: bool,
}

impl RetainedScratchAgentRun<'_> {
    /// Compare process-local buffer identity plus tagged ID, revision, and language.
    /// The serialized typed binding remains owner-local and cannot distinguish owners;
    /// freshness additionally requires the private, non-serialized evaluation identity.
    pub fn source_is_fresh(&self, current: &ScratchBuffer) -> bool {
        &self.evaluation_source_id == current.evaluation_source_id()
            && self
                .run
                .turn
                .source_binding()
                .is_some_and(|binding| *binding == source_binding(current.snapshot()))
    }

    /// Compact presentation of authoritative lifecycle evidence only.
    /// Never displays arguments, returned values, prompts, or model claims.
    pub fn activity_summary(&self) -> String {
        let turn = &self.run.turn;
        let usage = turn.usage();
        let mut text = format!(
            "Agent activity: {:?} — {:?}\nDecisions: {}; tool attempts: {}.\n",
            turn.source_binding(),
            turn.state(),
            usage.decisions,
            usage.tool_calls,
        );
        let mut records = 0;
        for evidence in turn.evidence() {
            if let AgentEvidence::Tool { name, outcome, .. } = evidence {
                let name: String = name
                    .chars()
                    .take(120)
                    .flat_map(char::escape_default)
                    .collect();
                let status = if *outcome == AgentToolOutcome::Success {
                    "returned successfully"
                } else {
                    "failed or denied (not evidence of execution)"
                };
                text.push_str(&format!("- Tool attempt {name}: {status} ({outcome:?}).\n"));
                records += 1;
            }
        }
        if records == 0 {
            text.push_str("No tool evidence retained; no semantic operations inferred.\n");
        }
        text.push_str("Runtime diagnostic/evaluation attribution is not inferred from tool names; retained workbench diagnostics are not agent evidence.\nModel conclusion follows separately; it is not an execution record.\n");
        text
    }
}

// Document IDs here are in-memory host identities, not filesystem paths.
fn source_binding(source: SourceSnapshot<'_>) -> AgentSourceBinding {
    AgentSourceBinding {
        identity: match source.id {
            SourceId::Scratch(id) => AgentSourceIdentity::ScratchBuffer(id.to_string()),
            SourceId::Document(id) => AgentSourceIdentity::Host(id.as_u64().to_string()),
        },
        revision: source.version,
        language: source.language.to_owned(),
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
    run_retained_scratch_agent_observed(
        source,
        service,
        model,
        request,
        limits,
        config,
        &AgentCancellationToken::default(),
        &mut |_| {},
    )
}

/// Retained runner with cooperative cancellation and authoritative lifecycle snapshots.
/// The callback also observes failed/cancelled turns when no successful run is returned.
pub fn run_retained_scratch_agent_observed<'a>(
    source: &'a ScratchBuffer,
    service: &dyn LanguageService,
    model: &mut (impl DecisionModel + ?Sized),
    request: &str,
    limits: AgentLimits,
    config: RetainedScratchAgentConfig<'_>,
    cancellation: &AgentCancellationToken,
    callback: &mut dyn FnMut(&AgentTurn),
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
        run_session_traced_observed(
            model,
            &mut session,
            request,
            limits,
            read_only_policy,
            &mut trace,
            Some(source_binding(snapshot)),
            cancellation,
            callback,
        )?
    };
    Ok(RetainedScratchAgentRun {
        run,
        source: snapshot,
        evaluation_source_id: source.evaluation_source_id().clone(),
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

    fn outcome(source: &ScratchBuffer) -> RetainedScratchAgentRun<'_> {
        RetainedScratchAgentRun {
            run: AgentRun {
                turn: AgentTurn::new(
                    None,
                    Some(source_binding(source.snapshot())),
                    AgentLimits::default(),
                ),
                content: "PRIVATE_MODEL_CLAIM".into(),
                stop: AgentStop::Completed,
                decisions: 999,
                tool_calls: 999,
                malformed: 0,
            },
            source: source.snapshot(),
            evaluation_source_id: source.evaluation_source_id().clone(),
            trace_jsonl: Vec::new(),
            trace_truncated: false,
        }
    }

    #[test]
    fn trace_and_legacy_counters_are_not_lifecycle_authority() {
        let mut sources = ScratchBuffers::default();
        sources.create("test", "PRIVATE_SOURCE");
        let mut result = outcome(sources.selected().unwrap());
        let summary = result.activity_summary();
        result.trace_jsonl =
            br#"{"event":"feedback","envelope":{"kind":"tool_result","name":"FORGED","ok":true}}"#
                .to_vec();
        result.trace_truncated = true;
        assert_eq!(summary, result.activity_summary());
        assert!(summary.contains("Ready"));
        assert!(summary.contains("Decisions: 0; tool attempts: 0"));
        assert!(!summary.contains("PRIVATE"));
        assert!(!summary.contains("999"));
    }

    #[test]
    fn tool_summary_uses_typed_outcomes_and_hides_native_outputs() {
        struct Provider(Vec<ToolDefinition>);
        impl ToolProvider for Provider {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::new(ToolValue::String("PRIVATE_OUTPUT".into())))
            }
        }
        struct Model {
            name: String,
            called: bool,
        }
        impl DecisionModel for Model {
            fn decide(
                &mut self,
                _: &[crate::agent::DecisionMessage],
                _: u32,
            ) -> Result<String, String> {
                let response = if self.called {
                    serde_json::json!({"type":"complete", "content":"PRIVATE_CLAIM"})
                } else {
                    self.called = true;
                    serde_json::json!({"type":"tool", "name":self.name, "arguments":{}})
                };
                Ok(response.to_string())
            }
        }
        let mut sources = ScratchBuffers::default();
        sources.create("test", "source");
        for allowed in [false, true] {
            let name = format!("\n\"\\{}UNRETAINED_SUFFIX", "x".repeat(117));
            let mut session = AgentSession::builder()
                .provider(Provider(vec![ToolDefinition::new(&name, "inspect")]))
                .build()
                .unwrap();
            let mut result = outcome(sources.selected().unwrap());
            result.run = crate::agent::run_session(
                &mut Model {
                    name,
                    called: false,
                },
                &mut session,
                "inspect",
                AgentLimits::default(),
                |_, _| allowed,
            )
            .unwrap();
            let summary = result.activity_summary();
            assert_eq!(
                summary
                    .lines()
                    .filter(|line| line.starts_with("- Tool attempt "))
                    .count(),
                1
            );
            assert!(summary.contains(if allowed {
                "returned successfully (Success)"
            } else {
                "failed or denied (not evidence of execution) (Denied)"
            }));
            assert!(!summary.contains("PRIVATE"));
            assert!(!summary.contains("UNRETAINED_SUFFIX"));
            assert!(summary.contains("\\n\\\"\\\\"));
        }
    }

    #[test]
    fn source_freshness_rejects_independent_owners_with_identical_bindings() {
        let mut first = ScratchBuffers::default();
        let mut second = ScratchBuffers::default();
        assert_eq!(first.create("test", "same"), 0);
        assert_eq!(second.create("test", "same"), 0);
        let source = first.selected().unwrap();
        let other = second.selected().unwrap();
        assert_eq!(
            source_binding(source.snapshot()),
            source_binding(other.snapshot())
        );
        let result = outcome(source);
        assert!(result.source_is_fresh(source));
        assert!(!result.source_is_fresh(other));
    }

    #[test]
    fn source_freshness_requires_tag_revision_and_language() {
        let mut sources = ScratchBuffers::default();
        let id = sources.create("test", "original");
        let source = sources.selected().unwrap();
        let original_turn = outcome(source).run.turn;
        let original_identity = source.evaluation_source_id().clone();
        let binding = source_binding(source.snapshot());
        let mut result = outcome(source);
        assert!(result.source_is_fresh(source));
        for stale in [
            AgentSourceBinding {
                revision: binding.revision + 1,
                ..binding.clone()
            },
            AgentSourceBinding {
                language: "other".into(),
                ..binding.clone()
            },
            AgentSourceBinding {
                identity: AgentSourceIdentity::ScratchBuffer((id + 1).to_string()),
                ..binding.clone()
            },
            AgentSourceBinding {
                identity: AgentSourceIdentity::Host(id.to_string()),
                ..binding.clone()
            },
        ] {
            result.run.turn = AgentTurn::new(None, Some(stale), AgentLimits::default());
            assert!(!result.source_is_fresh(source));
        }
        drop(result);
        sources
            .selected_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), "edited "))
            .unwrap();
        let source = sources.selected().unwrap();
        assert_eq!(source.evaluation_source_id(), &original_identity);
        let mut result = outcome(source);
        assert!(result.source_is_fresh(source));
        result.run.turn = original_turn;
        assert!(!result.source_is_fresh(source));
    }

    #[test]
    fn observed_runner_retains_provenance_and_supports_cancellation() {
        struct Service;
        impl LanguageService for Service {
            fn language_id(&self) -> &'static str {
                "test"
            }
            fn language_name(&self) -> &'static str {
                "Test"
            }
            fn matches_path(&self, _: &scope_language::LanguagePath<'_>) -> bool {
                false
            }
            fn analyze(
                &self,
                _: scope_language::LanguageDocument<'_>,
            ) -> scope_language::LanguageAnalysis {
                Default::default()
            }
        }
        struct Model;
        impl DecisionModel for Model {
            fn decide(
                &mut self,
                _: &[crate::agent::DecisionMessage],
                _: u32,
            ) -> Result<String, String> {
                Ok(r#"{"type":"complete","content":"done"}"#.into())
            }
        }
        let mut sources = ScratchBuffers::default();
        sources.create("test", "unsaved");
        for cancel in [false, true] {
            let token = AgentCancellationToken::default();
            if cancel {
                token.cancel();
            }
            let mut snapshots = Vec::new();
            let mut authorize = |_: SemanticAuthorization<'_>| true;
            let result = run_retained_scratch_agent_observed(
                sources.selected().unwrap(),
                &Service,
                &mut Model,
                "inspect",
                AgentLimits::default(),
                RetainedScratchAgentConfig {
                    trace: None,
                    allowed_operation_ids: &[],
                    authorize: &mut authorize,
                },
                &token,
                &mut |turn| snapshots.push(turn.clone()),
            )
            .unwrap();
            assert_eq!(snapshots.last(), Some(&result.run.turn));
            assert_eq!(
                result.run.turn.source_binding(),
                Some(&source_binding(result.source))
            );
            assert!(result.source_is_fresh(sources.selected().unwrap()));
            assert_eq!(
                result.run.stop,
                if cancel {
                    AgentStop::Cancelled
                } else {
                    AgentStop::Completed
                }
            );
        }
    }

    #[test]
    fn authoritative_budget_state_is_not_completion() {
        let mut sources = ScratchBuffers::default();
        sources.create("test", "source");
        let mut result = outcome(sources.selected().unwrap());
        struct Model;
        impl DecisionModel for Model {
            fn decide(
                &mut self,
                _: &[crate::agent::DecisionMessage],
                _: u32,
            ) -> Result<String, String> {
                panic!("zero decision budget must not call model")
            }
        }
        let mut session = AgentSession::builder().build().unwrap();
        result.run = crate::agent::run_session(
            &mut Model,
            &mut session,
            "inspect",
            AgentLimits {
                max_decisions: 0,
                ..AgentLimits::default()
            },
            read_only_policy,
        )
        .unwrap();
        assert!(
            result
                .activity_summary()
                .contains("BudgetExhausted(DecisionBudget)")
        );
        assert!(!result.activity_summary().contains("Completed"));
    }
}
