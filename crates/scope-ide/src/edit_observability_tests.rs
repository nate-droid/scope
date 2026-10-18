use super::*;
use crate::{
    agent::DecisionMessage,
    tool::{ToolProvider, ToolResult},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

const PRIVATE: &str = "PRIVATE_SOURCE_ARGUMENT_DIAGNOSTIC_7f39";

struct Script {
    replies: VecDeque<Result<String, String>>,
    messages: Vec<Vec<DecisionMessage>>,
}
impl DecisionModel for Script {
    fn decide(&mut self, messages: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.messages.push(messages.to_vec());
        self.replies
            .pop_front()
            .expect("unexpected model invocation")
    }
}
fn script(replies: Vec<String>) -> Script {
    Script {
        replies: replies.into_iter().map(Ok).collect(),
        messages: vec![],
    }
}
fn call(name: &str) -> String {
    json!({"type":"tool", "name":name, "arguments":{"private":PRIVATE}}).to_string()
}
fn proposal() -> ToolValue {
    ToolValue::object([
        ("status", "proposal".into()),
        ("proposal_id", "1".into()),
        ("diagnostics", PRIVATE.into()),
    ])
}
struct Provider {
    definitions: Vec<ToolDefinition>,
    replies: VecDeque<ToolValue>,
    calls: Arc<Mutex<Vec<String>>>,
}
impl ToolProvider for Provider {
    fn tools(&self) -> &[ToolDefinition] {
        &self.definitions
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        self.calls.lock().unwrap().push(call.name);
        Ok(ToolResult::new(
            self.replies.pop_front().expect("unexpected dispatch"),
        ))
    }
}
fn session(replies: Vec<ToolValue>) -> (AgentSession<'static>, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(vec![]));
    let provider = Provider {
        definitions: vec![
            ToolDefinition::new("read", "inspect"),
            ToolDefinition::new("edit", "propose").effect(ToolEffect::Write),
        ],
        replies: replies.into(),
        calls: calls.clone(),
    };
    (
        AgentSession::builder().provider(provider).build().unwrap(),
        calls,
    )
}
fn caps(inspect_first: bool) -> EditCapabilities {
    EditCapabilities {
        inspect: vec!["read".into()],
        propose: vec!["edit".into()],
        inspect_first,
    }
}
fn run(
    model: &mut Script,
    session: &mut AgentSession<'_>,
    inspect: bool,
    budgets: EditBudgets,
) -> EditRun {
    run_edit_session(
        model,
        session,
        Ok(SourceId::Scratch(1)),
        PRIVATE,
        &caps(inspect),
        budgets,
        |_, _| true,
    )
}
fn assert_summary(
    run: &EditRun,
    models: usize,
    tools: usize,
    duplicates: usize,
    retries: usize,
    proposals: usize,
) {
    let summary = run.summary();
    assert_eq!(summary.outcome, run.outcome);
    assert_eq!(summary.model_invocations, models);
    assert_eq!(summary.tool_calls, tools);
    assert_eq!(summary.duplicate_calls_suppressed, duplicates);
    assert_eq!(summary.protocol_retries, retries);
    assert_eq!(summary.proposal_attempts, proposals);
    assert_eq!(
        summary.phase_transitions,
        run.trace
            .iter()
            .filter(|event| matches!(event, EditEvent::Transition { .. }))
            .count()
    );
    // Neither wall-clock duration nor unavailable backend usage is fabricated.
    let _: Option<u128> = summary.elapsed_ms;
    assert_eq!(summary.token_usage, None::<u64>);
    assert_eq!(run.compact_trace(), run.compact_trace());
    assert!(!run.compact_trace().is_empty());
    assert!(!run.compact_trace().contains(PRIVATE));
    assert!(!format!("{:?}", run.trace).contains(PRIVATE));
}

#[test]
fn summary_counts_proposal_without_application_and_preserves_phase_evidence() {
    let read = ToolValue::object([
        ("status", "read_only".into()),
        (
            "value",
            ToolValue::object([
                ("candidate", PRIVATE.into()),
                ("validation_status", "Accepted".into()),
                ("checks_passed", ToolValue::Integer(3)),
                ("checks_total", ToolValue::Integer(3)),
            ]),
        ),
    ]);
    let mut model = script(vec![
        call("read"),
        json!({"type":"complete", "content":PRIVATE}).to_string(),
        call("edit"),
    ]);
    let (mut session, calls) = session(vec![read, proposal()]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.proposal, Some(ProposalId(1)));
    assert_eq!(*calls.lock().unwrap(), vec!["read", "edit"]);
    assert!(model.replies.is_empty());
    assert_summary(&result, 3, 2, 0, 0, 1);
    assert_eq!(result.summary().phase_transitions, 4);
    for (index, phase) in [(1, "Inspect"), (2, "Propose")] {
        let context: serde_json::Value =
            serde_json::from_str(&model.messages[index][2].content).unwrap();
        assert_eq!(context["phase"], phase);
        assert_eq!(context["evidence"][0]["call"]["name"], "read");
        assert_eq!(
            context["evidence"][0]["value"]["value"]["candidate"],
            PRIVATE
        );
        assert_eq!(
            context["evidence"][0]["value"]["value"]["validation_status"],
            "Accepted"
        );
        assert_eq!(context["evidence"][0]["value"]["value"]["checks_passed"], 3);
        assert!(
            model.messages[index]
                .iter()
                .all(|message| message.role != "assistant")
        );
    }
    assert!(result.trace.iter().any(|event| matches!(event,
        EditEvent::EvidenceRetained { count } if *count > 0)));
    assert!(result.trace.iter().any(|event| matches!(event,
        EditEvent::ToolObserved { name, phase: EditPhase::Inspect } if name == "read")));
    assert!(result.trace.iter().any(|event| matches!(
        event,
        EditEvent::ToolResult {
            classification: ToolResultClassification::ReadOnly
        }
    )));
    assert!(result.trace.iter().any(|event| matches!(
        event,
        EditEvent::ToolResult {
            classification: ToolResultClassification::Proposal
        }
    )));
    // The provider exposes no apply/save operation. ProposalReady is not an apply receipt.
    assert_eq!(result.state.phase(), EditPhase::Finished);
}

#[test]
fn early_resolution_failure_has_no_model_or_tool_activity() {
    let mut model = script(vec![]);
    let (mut session, calls) = session(vec![]);
    let result = run_edit_session(
        &mut model,
        &mut session,
        Err(TargetResolutionError::NotFound),
        PRIVATE,
        &caps(false),
        EditBudgets::default(),
        |_, _| panic!("must not authorize a tool"),
    );
    assert_eq!(result.outcome, EditOutcome::MissingPrerequisite);
    assert_summary(&result, 0, 0, 0, 0, 0);
    assert_eq!(result.summary().phase_transitions, 1);
    assert!(model.messages.is_empty());
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn backend_failure_completes_invocation_without_exposing_error() {
    let mut model = script(vec![]);
    model.replies.push_back(Err(PRIVATE.into()));
    let (mut session, calls) = session(vec![]);
    let result = run(&mut model, &mut session, false, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::AgentBackendFailure);
    assert_summary(&result, 1, 0, 0, 0, 0);
    assert!(calls.lock().unwrap().is_empty());
    assert!(result.trace.iter().any(|event| matches!(
        event,
        EditEvent::ModelCompleted {
            ordinal: 1,
            phase: EditPhase::Propose,
            completion: ModelCompletion::BackendFailure
        }
    )));
}

#[test]
fn protocol_retry_counts_separately_from_proposal_correction() {
    let malformed = format!("not JSON: {PRIVATE}");
    let response = call("edit");
    let bytes = response.len();
    let mut model = script(vec![malformed, response]);
    let (mut session, calls) = session(vec![proposal()]);
    let result = run(&mut model, &mut session, false, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_summary(&result, 2, 1, 0, 1, 1);
    assert_eq!(result.corrections, 0);
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert!(
        result
            .trace
            .iter()
            .any(|event| matches!(event, EditEvent::ProtocolError { .. }))
    );
    assert!(result.trace.iter().any(|event| matches!(event,
        EditEvent::ModelCompleted { ordinal: 2, completion: ModelCompletion::Response { bytes: actual }, .. }
            if *actual == bytes)));
}

#[test]
fn duplicate_observation_does_not_inflate_dispatched_tool_count() {
    let mut model = script(vec![call("read"), call("read")]);
    let (mut session, calls) = session(vec![ToolValue::object([
        ("status", "read_only".into()),
        ("value", PRIVATE.into()),
    ])]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::NoProgress);
    assert_summary(&result, 2, 1, 1, 0, 0);
    assert_eq!(*calls.lock().unwrap(), vec!["read"]);
}

#[test]
fn exhausted_protocol_budget_reports_errors_but_only_actual_retries() {
    let mut model = script(vec![format!("malformed {PRIVATE}")]);
    let (mut session, calls) = session(vec![]);
    let mut budgets = EditBudgets::default();
    budgets.transport.max_malformed = 0;
    let result = run(&mut model, &mut session, true, budgets);
    assert_eq!(result.outcome, EditOutcome::NoProgress);
    assert_summary(&result, 1, 0, 0, 0, 0);
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(
        result
            .trace
            .iter()
            .filter(|event| matches!(event, EditEvent::ProtocolError { .. }))
            .count(),
        1
    );
}
