//! Bounded edit execution with contextual semantic inventory and discovery.
use super::{
    AgentLimits, Decision, DecisionModel, PROTOCOL, parse_decision, push_message, to_json,
};
use crate::{
    proposal::ProposalId,
    source_edit::{
        AgentEditRequest, PermittedEditScope, SourceEditController, SourceId, SourceTarget,
        TargetResolutionError,
    },
    tool::{AgentSession, ToolCall, ToolDefinition, ToolEffect, ToolError, ToolValue},
};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPhase {
    Resolve,
    Inspect,
    Propose,
    Validate,
    Finished,
    Blocked,
}

/// No backwards planning edge: correction returns only to proposal generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditState {
    phase: EditPhase,
}
impl Default for EditState {
    fn default() -> Self {
        Self {
            phase: EditPhase::Resolve,
        }
    }
}
impl EditState {
    pub fn phase(&self) -> EditPhase {
        self.phase
    }
    pub fn transition(&mut self, next: EditPhase) -> Result<(), InvalidTransition> {
        use EditPhase::*;
        if !matches!(
            (self.phase, next),
            (Resolve, Inspect | Propose | Blocked)
                | (Inspect, Propose | Finished | Blocked)
                | (Propose, Validate | Blocked)
                | (Validate, Propose | Finished | Blocked)
        ) {
            return Err(InvalidTransition {
                from: self.phase,
                to: next,
            });
        }
        self.phase = next;
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: EditPhase,
    pub to: EditPhase,
}

#[derive(Debug, Clone, Copy)]
pub struct EditBudgets {
    pub inspections: usize,
    /// Revised candidate evaluations permitted after the initial evaluation.
    pub refinements: usize,
    pub proposals: usize,
    pub corrections: usize,
    pub model_invocations: usize,
    /// Suppressed duplicate calls tolerated before NoProgress; never redispatched.
    /// Default 0 stops immediately; 1 permits one feedback opportunity, subject
    /// to unchanged model/transport budgets. Shared by editing and investigation.
    pub repeated_calls: usize,
    pub transport: AgentLimits,
}
impl Default for EditBudgets {
    fn default() -> Self {
        Self {
            inspections: 3,
            refinements: 2,
            proposals: 1,
            corrections: 1,
            model_invocations: 6,
            repeated_calls: 0,
            transport: AgentLimits::default(),
        }
    }
}

/// Explicit host-selected capabilities, not all tools marked Write. Only M2
/// proposal operations belong in `propose`; never save/apply/evaluate tools.
#[derive(Debug, Default)]
pub struct EditCapabilities {
    pub inspect: Vec<String>,
    pub propose: Vec<String>,
    pub inspect_first: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOutcome {
    ProposalReady,
    InvestigationCompleted,
    InvestigationInconclusive,
    UnsupportedRequest,
    MissingPrerequisite,
    AmbiguousTarget,
    ValidationFailure,
    SemanticBlocked,
    OperationBudgetExhausted,
    ExplorationBudgetExhausted,
    RefinementBudgetExhausted,
    NoProgress,
    AuthorizationFailure,
    AgentBackendFailure,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditEvent {
    ModelCompleted {
        ordinal: usize,
        phase: EditPhase,
        completion: ModelCompletion,
    },
    ToolObserved {
        name: String,
        phase: EditPhase,
    },
    ToolResult {
        classification: ToolResultClassification,
    },
    EvidenceRetained {
        count: usize,
    },
    Discovery {
        revealed: usize,
    },
    ProtocolError {
        category: String,
    },
    Transition {
        from: EditPhase,
        to: EditPhase,
    },
    ModelInvocation(usize),
    Inspection(usize),
    CandidateEvaluation(usize),
    RefinementAttempt(usize),
    ProposalAttempt(usize),
    /// Arguments and returned source are deliberately absent from the trace.
    ToolCall {
        ordinal: usize,
        phase: EditPhase,
    },
    NewToolResult,
    NewSourceInformation,
    RepeatedCall,
    NoProgressReason {
        reason: String,
    },
    ProposalProduced(ProposalId),
    RevisedProposalProduced(ProposalId),
    ValidationFailure,
    ValidationStatus {
        status: String,
        diagnostic_count: usize,
    },
    SemanticBlocker {
        category: String,
        recoverable: bool,
    },
    ProtocolRetry {
        category: String,
        attempt: usize,
    },
    CorrectionAttempt(usize),
    ValidProposalReached(ProposalId),
    Terminated(EditOutcome),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCompletion {
    Response { bytes: usize },
    BackendFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolResultClassification {
    ReadOnly,
    Proposal,
    Blocked,
    Rejected,
    InvalidArguments,
    Unauthorized,
    Unsupported,
    MissingPrerequisite,
    InternalFailure,
    Other,
}
fn diagnostic_category(value: &str) -> String {
    match value {
        "Accepted"
        | "AcceptedWithLimitations"
        | "Rejected"
        | "NotValidated"
        | "NotEvaluated"
        | "MissingDependency"
        | "UnsupportedConstruct"
        | "UnresolvedTarget"
        | "UnsatisfiedPrecondition"
        | "InsufficientContext"
        | "dependency"
        | "unsupported"
        | "missing_prerequisite"
        | "validation"
        | "unknown" => value.into(),
        _ => "other".into(),
    }
}

impl ToolResultClassification {
    fn from_status(status: Option<&str>) -> Self {
        match status {
            Some("read_only") => Self::ReadOnly,
            Some("proposal") => Self::Proposal,
            Some("blocked") => Self::Blocked,
            Some("invalid_source" | "rejected" | "validation_failed") => Self::Rejected,
            Some("invalid_arguments") => Self::InvalidArguments,
            Some("unauthorized") => Self::Unauthorized,
            Some("unsupported_operation") => Self::Unsupported,
            Some("revision_conflict" | "missing_prerequisite" | "ambiguous_target") => {
                Self::MissingPrerequisite
            }
            Some("internal_failure") => Self::InternalFailure,
            _ => Self::Other,
        }
    }
}

/// Host-observed counts. Backend usage is unavailable through DecisionModel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSummary {
    pub outcome: EditOutcome,
    pub model_invocations: usize,
    pub tool_calls: usize,
    pub duplicate_calls_suppressed: usize,
    pub protocol_retries: usize,
    pub phase_transitions: usize,
    pub proposal_attempts: usize,
    pub inspections: usize,
    pub evaluations: usize,
    pub refinements: usize,
    pub corrections: usize,
    pub elapsed_ms: Option<u128>,
    pub token_usage: Option<u64>,
}

#[derive(Debug)]
pub struct EditRun {
    pub state: EditState,
    pub outcome: EditOutcome,
    pub proposal: Option<ProposalId>,
    pub model_invocations: usize,
    pub inspections: usize,
    pub evaluations: usize,
    pub refinements: usize,
    pub proposal_attempts: usize,
    pub corrections: usize,
    /// Structured tool diagnostics are retained for the caller, not logged.
    /// Host termination metadata (`refinement_stopped`) may augment the final
    /// result without changing its provider status or evaluation evidence.
    pub last_result: Option<ToolValue>,
    pub trace: Vec<EditEvent>,
    /// Owned snapshot retained even on pre-dispatch failures.
    pub source_snapshot: Option<ToolValue>,
    pub discovered_operations: Vec<String>,
    pub evidence: Vec<InvestigationEvidence>,
    pub conclusion: Option<String>,
    started: std::time::Instant,
    elapsed_ms: Option<u128>,
}
impl Default for EditRun {
    fn default() -> Self {
        Self {
            state: EditState::default(),
            outcome: EditOutcome::OperationBudgetExhausted,
            proposal: None,
            model_invocations: 0,
            inspections: 0,
            evaluations: 0,
            refinements: 0,
            proposal_attempts: 0,
            corrections: 0,
            last_result: None,
            trace: Vec::new(),
            source_snapshot: None,
            discovered_operations: Vec::new(),
            evidence: Vec::new(),
            conclusion: None,
            started: std::time::Instant::now(),
            elapsed_ms: None,
        }
    }
}
impl EditRun {
    pub fn summary(&self) -> EditSummary {
        let count =
            |predicate: fn(&EditEvent) -> bool| self.trace.iter().filter(|e| predicate(e)).count();
        EditSummary {
            outcome: self.outcome,
            model_invocations: count(|e| matches!(e, EditEvent::ModelInvocation(_))),
            tool_calls: count(|e| matches!(e, EditEvent::ToolCall { .. })),
            duplicate_calls_suppressed: count(|e| matches!(e, EditEvent::RepeatedCall)),
            protocol_retries: count(|e| matches!(e, EditEvent::ProtocolRetry { .. })),
            phase_transitions: count(|e| matches!(e, EditEvent::Transition { .. })),
            proposal_attempts: count(|e| matches!(e, EditEvent::ProposalAttempt(_))),
            inspections: self.inspections,
            evaluations: self.evaluations,
            refinements: self.refinements,
            corrections: self.corrections,
            elapsed_ms: self.elapsed_ms,
            token_usage: None,
        }
    }

    /// Ordered host observations only; never prints request, arguments or results.
    pub fn compact_trace(&self) -> String {
        use std::fmt::Write;
        let mut output = String::from("Agent edit (host observations)\n");
        for (index, event) in self.trace.iter().enumerate() {
            let detail = match event {
                EditEvent::Transition { from, to } => format!("{from:?} -> {to:?}"),
                EditEvent::ToolObserved { name, phase } => format!("{phase:?}: {name}"),
                EditEvent::ToolResult { classification } => format!("result: {classification:?}"),
                EditEvent::EvidenceRetained { count } => format!("evidence retained: {count}"),
                EditEvent::RepeatedCall => "duplicate call suppressed (not dispatched)".into(),
                EditEvent::ProposalProduced(id) | EditEvent::RevisedProposalProduced(id) => {
                    format!("proposal registered: #{} (not applied)", id.0)
                }
                EditEvent::Terminated(outcome) => format!("outcome: {outcome:?}"),
                _ => format!("{event:?}"),
            };
            let _ = writeln!(output, "  {:03} {detail}", index + 1);
        }
        let _ = writeln!(output, "Metrics: {:?}", self.summary());
        output
    }
    /// Forward only the bounded, context-free trace through Scope's existing gate.
    /// Logging failure does not change edit completion or grant logging authority.
    #[cfg(feature = "logging")]
    pub fn log_trace(
        &self,
        binding: &crate::logging::LoggingBinding,
    ) -> Result<crate::logging::LoggingReceipt, crate::logging::LoggingError> {
        binding.log_info(&format!("scope.edit {:?}", self.trace))
    }
    fn transition(&mut self, next: EditPhase) {
        let from = self.state.phase();
        self.state
            .transition(next)
            .expect("runner transition invariant");
        self.trace.push(EditEvent::Transition { from, to: next });
    }
    fn finish(mut self, outcome: EditOutcome) -> Self {
        self.transition(
            if matches!(
                outcome,
                EditOutcome::ProposalReady
                    | EditOutcome::InvestigationCompleted
                    | EditOutcome::InvestigationInconclusive
            ) {
                EditPhase::Finished
            } else {
                EditPhase::Blocked
            },
        );
        self.outcome = outcome;
        self.elapsed_ms = Some(self.started.elapsed().as_millis());
        self.trace.push(EditEvent::Terminated(outcome));
        self
    }
}
fn field<'a>(value: &'a ToolValue, name: &str) -> Option<&'a ToolValue> {
    match value {
        ToolValue::Object(fields) => fields.get(name),
        _ => None,
    }
}

/// The host passes the result of M1 `resolve_edit_target`, the scoped request
/// snapshot, and a session bound to that same target. Provider validation evidence
/// accompanies `proposal`; registration alone does not imply semantic validation.
/// No application or second proposal registry occurs here.
/// The session's target is read-only for this run. Thus unchanged call arguments
/// imply unchanged relevant document state. Mutable/ambient providers are excluded.
/// Synchronous native/model calls must terminate themselves (no timeout guarantee).
pub fn run_edit_session(
    model: &mut (impl DecisionModel + ?Sized),
    session: &mut AgentSession<'_>,
    resolved: Result<SourceId, TargetResolutionError>,
    request: &str,
    capabilities: &EditCapabilities,
    budgets: EditBudgets,
    mut policy: impl FnMut(&ToolDefinition, &ToolCall) -> bool,
) -> EditRun {
    run_edit_session_with_state(
        model,
        session,
        resolved,
        request,
        capabilities,
        budgets,
        EditDispatch {
            policy: &mut policy,
            relevant_state: &mut |_| ToolValue::Null,
        },
    )
}

/// Host-owned callbacks. State tokens must cover relevant document/tool revisions
/// and contain no secrets. Reading a token must not invoke inspection tools.
/// A changed token permits an otherwise identical call, within the same budgets.
pub struct EditDispatch<'a> {
    pub policy: &'a mut dyn FnMut(&ToolDefinition, &ToolCall) -> bool,
    pub relevant_state: &'a mut dyn FnMut(&ToolCall) -> ToolValue,
}

/// State-aware variant for hosts whose read-only tool information can change.
/// The host retains the same target/capability/authorization obligations as
/// `run_edit_session`; a state token is not authorization or target resolution.
pub fn run_edit_session_with_state(
    model: &mut (impl DecisionModel + ?Sized),
    session: &mut AgentSession<'_>,
    resolved: Result<SourceId, TargetResolutionError>,
    request: &str,
    capabilities: &EditCapabilities,
    budgets: EditBudgets,
    dispatch: EditDispatch<'_>,
) -> EditRun {
    run_edit_session_inventory(
        model,
        session,
        resolved,
        request,
        capabilities,
        budgets,
        dispatch,
        None,
    )
}

/// Lifecycle-local discovery uses the existing tool decision protocol, not a provider.
pub const DISCOVER_SEMANTIC_OPERATIONS: &str = "edit.discover_operations";

/// Host task intent expressed as fully qualified operation names. An empty subset
/// starts with no visible operations. Discovery only reveals eligible operations.
#[derive(Debug, Default)]
pub struct SemanticDiscoveryOptions {
    pub initial_operations: Vec<String>,
    pub page_size: usize,
}

fn compact_catalog(
    catalog: &crate::tool::SessionCatalog,
    names: &[String],
    projected: bool,
) -> serde_json::Value {
    let mut entries: Vec<_> = catalog
        .tools
        .iter()
        .filter(|entry| names.contains(&entry.definition.name))
        .collect();
    entries.sort_by(|a, b| a.definition.name.cmp(&b.definition.name));
    json!({"tools": entries.into_iter().map(|entry| {
        if let Some(metadata) = entry.metadata.as_ref().filter(|metadata| {
            let value = to_json((*metadata).clone());
            entry.definition.name.starts_with("language.")
                && ["id", "description", "arguments", "effect", "target"]
                    .iter()
                    .all(|key| value.get(*key).is_some())
        }) {
            let metadata = to_json(metadata.clone());
            let mut operation = serde_json::Map::new();
            // Whitelist protocol-relevant fields; opaque constraints are neither
            // an invocation schema nor useful bounded discovery context.
            for key in ["id", "description", "arguments", "effect", "target", "purpose", "produces_evaluation"] {
                if let Some(value) = metadata.get(key) {
                    operation.insert(key.into(), value.clone());
                }
            }

            let inputs = if projected {
                let mut single = catalog.clone();
                single.tools = vec![entry.clone()];
                to_json(single.to_value())["tools"][0]["definition"]["inputs"].clone()
            } else {
                json!([
                    {"name":"expected_version","kind":"string","required":true},
                    {"name":"expected_source","kind":"string","required":true},
                    {"name":"arguments","kind":"object","required":true}
                ])
            };
            json!({
                "name":entry.definition.name,
                "operation":operation,
                "invocation_schema":metadata.get("invocation_schema"),
                "inputs":inputs,
                "preconditions":if projected { json!({"ownership":"Host binds exact source, revision, identity, scope and selection; send only domain arguments."}) } else { json!({
                    "expected_version":"Exact decimal u64 revision of resolved target",
                    "expected_source":"Exact original source of resolved target",
                    "selection":"Host-provided selection required when operation.target is Selection"
                }) },
                "scope":"single_document",
                "effect_semantics":if metadata.get("purpose").and_then(serde_json::Value::as_str) == Some("CandidateEvaluation") && entry.definition.effect == ToolEffect::Read {
                    "Executes a disposable candidate; does not install its definitions or modify active source. Returned evidence may support a later proposal; candidate definitions are not present in the document."
                } else if entry.definition.effect == ToolEffect::Read {
                    "Returns provider evidence without proposing or applying source changes; does not imply candidate validation or evaluation"
                } else {
                    "Proposes source changes for host review; never applies them; validation and evaluation are established only by explicit provider evidence"
                },
                "result":if entry.definition.effect == ToolEffect::Read { "read_only" } else { "proposal requiring host review; never applied" }
            })
        } else {
            let mut single = catalog.clone();
            single.tools = vec![entry.clone()];
            to_json(single.to_value())["tools"][0].clone()
        }
    }).collect::<Vec<_>>()})
}

/// Level A is derived only from the canonical session inventory, never a second registry.
fn discovery_summary(catalog: &crate::tool::SessionCatalog, names: &[String]) -> serde_json::Value {
    let canonical = to_json(catalog.to_value());
    let mut tools: Vec<_> = canonical["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            names
                .iter()
                .any(|name| entry["definition"]["name"] == *name)
        })
        .map(|entry| {
            let metadata = &entry["metadata"];
            let description = metadata["description"]
                .as_str()
                .or_else(|| entry["definition"]["description"].as_str())
                .unwrap_or("");
            let truncated = description.chars().count() > 160;
            let mut short: String = description.chars().take(160).collect();
            if truncated {
                short.push('…');
            }
            json!({"name":entry["definition"]["name"], "effect":entry["definition"]["effect"],
                "target":metadata["target"], "purpose":metadata["purpose"],
                "produces_evaluation":metadata["produces_evaluation"],
                "description":short, "description_truncated":truncated})
        })
        .collect();
    tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    json!({"tools":tools})
}

fn discovery_detail(
    catalog: &crate::tool::SessionCatalog,
    allowed: &[String],
    name: &str,
) -> serde_json::Value {
    let canonical = to_json(catalog.to_value());
    let entry = canonical["tools"].as_array().unwrap().iter().find(|entry| {
        allowed.iter().any(|allowed| allowed == name) && entry["definition"]["name"] == name
    });
    match entry {
        Some(entry) => json!({"kind":"tool_result", "ok":true, "value":{"tools":[entry]}}),
        None => {
            json!({"kind":"tool_result", "ok":false, "error":"unknown_or_ineligible_operation"})
        }
    }
}

fn reveal_operations(eligible: &[String], visible: &mut Vec<String>, page_size: usize) -> usize {
    let mut hidden: Vec<_> = eligible
        .iter()
        .filter(|name| !visible.contains(name))
        .cloned()
        .collect();
    hidden.sort();
    hidden.dedup();
    let count = hidden.len().min(page_size);
    visible.extend(hidden.into_iter().take(count));
    count
}

/// Host-owned evidence, not a model-authored plan or reasoning transcript.
/// Each result remains tied to the exact invocation and host revision token.
#[derive(Default)]
struct EditContinuation {
    evidence: Vec<(ToolCall, ToolValue, ToolValue)>,
    feedback: Option<String>,
    invalidated: usize,
    /// Focus survives unrelated inspection, but never a relevant revision change.
    refinement: Option<(ToolCall, ToolValue, ToolValue)>,
}
impl EditContinuation {
    fn retain(&mut self, call: ToolCall, state: ToolValue, value: ToolValue) {
        if let Some(entry) = self
            .evidence
            .iter_mut()
            .find(|(previous, token, _)| previous == &call && token == &state)
        {
            entry.2 = value;
        } else {
            self.evidence.push((call, state, value));
        }
        self.feedback = None;
    }

    fn refresh(&mut self, relevant_state: &mut dyn FnMut(&ToolCall) -> ToolValue) {
        if self
            .refinement
            .as_ref()
            .is_some_and(|(call, state, _)| *state != relevant_state(call))
        {
            self.refinement = None;
        }
        let previous = self.evidence.len();
        self.evidence
            .retain(|(call, state, _)| *state == relevant_state(call));
        self.invalidated = previous - self.evidence.len();
        if self.invalidated > 0 {
            self.feedback = None;
        }
    }

    fn context(
        &self,
        phase: EditPhase,
        remaining_refinements: usize,
        investigation: bool,
    ) -> String {
        let guidance = self.refinement.as_ref().map(|(call, _, value)| {
            let evaluation = field(value, "evaluation");
            let outcome = evaluation.and_then(|e| field(e, "outcome")).and_then(ToolValue::as_str);
            json!({
                "outcome":outcome,
                "primary_action":if outcome == Some("Satisfied") { "complete_to_propose_then_invoke_authorized_proposal" } else { "submit_changed_candidate_using_evaluator" },
                "decision":{"type":"tool","name":call.name,"arguments":to_json(ToolValue::Object(call.arguments.clone()))},
                "decision_instruction":"The decision shows the exact evaluated invocation, not a request to repeat it. Submit changed candidate arguments through this evaluator; preserve applicable criteria and source preconditions. Additional source inspection is optional when needed. Return complete if unable to proceed.",
                "evaluation":evaluation.map(|e| to_json(e.clone())),
                "observations":to_json(value.clone()),
                "remaining_refinements":remaining_refinements,
                "inspection_scope":"Inspection reads existing source definitions, not temporary candidate definitions. Candidate execution is disposable and installs nothing. Validation evidence is retained separately from active source.",
                "evaluation_orchestration":"explicit_model_invocation",
                "missing_host_interface":"Typed argument roles binding candidate source and retained validation criteria to evaluator inputs are not available; argument names, descriptions, and JSON types are insufficient for safe reconstruction.",
                "available_actions":["submit changed candidate through the named evaluator", "inspect existing source when needed", "report inability with complete", "advance to authorized proposal only when evidence supports it"]
            })
        });
        json!({
            "candidate_refinement":guidance,
            "kind":"edit_continuation",
            "phase":format!("{phase:?}"),
            "next_action":if investigation { "inspect_or_conclude" } else if phase == EditPhase::Propose { "invoke_permitted_proposal_tool" } else if self.refinement.is_some() { "follow_candidate_refinement" } else { "inspect_or_complete_to_propose" },
            "completion":if investigation { "host_assessed_conclusion_or_uncertainty" } else if phase == EditPhase::Inspect { "advance_to_propose" } else { "unsupported_without_proposal" },
            "invalidation":if self.invalidated == 0 { serde_json::Value::Null } else {
                json!({"removed_results":self.invalidated,"instruction":"Relevant host state changed; stale evidence was removed. Inspect again if needed; original request preconditions and authorization remain unchanged."})
            },
            "evidence":self.evidence.iter().map(|(call, state, value)| json!({
                "kind":"tool_result", "ok":true,
                "call":{"name":call.name,"arguments":to_json(ToolValue::Object(call.arguments.clone()))},
                "relevant_state":({
                    let mut state = to_json(state.clone());
                    // Exact source remains in the internal comparison token. The
                    // immutable request already supplies the source to the model.
                    if let Some(fields) = state.as_object_mut() {
                        fields.remove("source");
                    }
                    state
                }),
                "value":to_json(value.clone())
            })).collect::<Vec<_>>()
        }).to_string()
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Session orchestration keeps target resolution, authorization, budgets, and discovery explicit"
)]
fn run_edit_session_inventory(
    model: &mut (impl DecisionModel + ?Sized),
    session: &mut AgentSession<'_>,
    resolved: Result<SourceId, TargetResolutionError>,
    request: &str,
    capabilities: &EditCapabilities,
    budgets: EditBudgets,
    dispatch: EditDispatch<'_>,
    discovery: Option<&SemanticDiscoveryOptions>,
) -> EditRun {
    run_objective_session_inventory(
        model,
        session,
        resolved,
        request,
        capabilities,
        budgets,
        dispatch,
        discovery,
        None,
        false,
        false,
    )
}

fn run_objective_session_inventory(
    model: &mut (impl DecisionModel + ?Sized),
    session: &mut AgentSession<'_>,
    resolved: Result<SourceId, TargetResolutionError>,
    request: &str,
    capabilities: &EditCapabilities,
    budgets: EditBudgets,
    dispatch: EditDispatch<'_>,
    discovery: Option<&SemanticDiscoveryOptions>,
    mut completion: Option<&mut CompletionPolicy<'_>>,
    projected_arguments: bool,
    compact_discovery: bool,
) -> EditRun {
    use EditOutcome::*;
    let mut run = EditRun::default();
    match resolved {
        Err(TargetResolutionError::NotFound) => return run.finish(MissingPrerequisite),
        Err(TargetResolutionError::Ambiguous { .. }) => return run.finish(AmbiguousTarget),
        Ok(_) => {}
    }
    let catalog = session.catalog();
    // Check effect and registration before invoking any backend. Discovery does
    // not authorize tools; the explicit policy is still checked at dispatch.
    for (names, effect) in [
        (&capabilities.inspect, ToolEffect::Read),
        (&capabilities.propose, ToolEffect::Write),
    ] {
        if names.iter().any(|name| {
            catalog
                .lookup(name)
                .is_none_or(|entry| entry.definition.effect != effect)
        }) {
            return run.finish(MissingPrerequisite);
        }
    }
    if completion.is_none() && capabilities.propose.is_empty() {
        return run.finish(UnsupportedRequest);
    }
    if request.trim().is_empty() {
        return run.finish(MissingPrerequisite);
    }
    run.transition(
        if completion.is_some() || (capabilities.inspect_first && !capabilities.inspect.is_empty())
        {
            EditPhase::Inspect
        } else {
            EditPhase::Propose
        },
    );
    let mut messages = Vec::new();
    let system = if projected_arguments {
        let objective = if completion.is_some() {
            "This is a bounded read-only investigation. Discover and invoke permitted read operations, inspect evidence, then complete with an evidence-backed conclusion or explicit uncertainty. Never propose, apply or save. Completion is assessed by the host, not proof of semantic correctness."
        } else {
            "This is a bounded edit task. During Inspect use read tools or complete to advance to Propose. During Propose invoke one permitted semantic edit tool. A proposal ends execution; never apply it. Complete without a proposal means unsupported request."
        };
        format!(
            "{PROTOCOL}\n{objective} For language tools, put only operation-specific domain fields directly in the decision arguments object, matching invocation_schema. The host binds the exact source, revision, target, scope and selection. Do not send expected_version, expected_source, an arguments wrapper, or catalog metadata. Escape literal newlines inside JSON strings as \\n."
        )
    } else if completion.is_some() {
        format!(
            "{PROTOCOL}\nThis is a bounded read-only investigation. Discover and invoke permitted read operations, inspect evidence, then return complete with an evidence-backed conclusion or explicit uncertainty. Never propose, apply or save. Language tool arguments must contain only expected_version, expected_source, and arguments, copied from edit_request; operation-specific inputs belong inside arguments.arguments. Completion is assessed by the host and does not establish semantic correctness."
        )
    } else {
        format!(
            "{PROTOCOL}\nThis is a bounded edit task. During Inspect use read tools or return complete to advance to Propose. During Propose invoke one permitted semantic edit tool. For language tools the decision arguments object must contain ONLY expected_version, expected_source, and arguments. Copy expected_version and expected_source from edit_request exactly. Put language-operation fields (such as a symbol name) inside arguments.arguments, not beside expected_version. Catalog metadata (operation, effect, target, scope, inputs) is descriptive and must not be sent as invocation fields. A proposal ends execution; never apply it. Complete without a proposal means unsupported request."
        )
    };
    if push_message(&mut messages, "system", system.clone(), budgets.transport).is_err()
        || push_message(&mut messages, "user", request.into(), budgets.transport).is_err()
    {
        return run.finish(OperationBudgetExhausted);
    }
    let mut visible = discovery
        .map(|options| options.initial_operations.clone())
        .unwrap_or_else(|| {
            capabilities
                .inspect
                .iter()
                .chain(&capabilities.propose)
                .cloned()
                .collect()
        });
    if discovery.is_some_and(|options| options.page_size == 0)
        || visible.iter().any(|name| {
            !capabilities.inspect.contains(name) && !capabilities.propose.contains(name)
        })
        || catalog.lookup(DISCOVER_SEMANTIC_OPERATIONS).is_some()
    {
        return run.finish(MissingPrerequisite);
    }
    run.discovered_operations = visible.clone();
    let is_evaluation = |name: &str| {
        catalog.lookup(name).is_some_and(|entry| {
            entry.definition.effect == ToolEffect::Read
                && entry
                    .metadata
                    .as_ref()
                    .and_then(|metadata| field(metadata, "purpose"))
                    .and_then(ToolValue::as_str)
                    == Some("CandidateEvaluation")
        })
    };
    let has_evaluation = capabilities.inspect.iter().any(|name| is_evaluation(name));
    let mut refinement_pending = false;
    let mut continuation = EditContinuation::default();
    let mut discovery_calls = 0usize;
    let mut calls: Vec<(ToolCall, ToolValue)> = Vec::new();
    let mut results: Vec<ToolValue> = Vec::new();
    let mut repeated = 0;
    let mut malformed = 0usize;
    loop {
        if run.model_invocations >= budgets.model_invocations
            || run.model_invocations >= budgets.transport.max_decisions
            || budgets.transport.max_tokens_per_decision == 0
        {
            return run.finish(OperationBudgetExhausted);
        }
        let allowed = if run.state.phase() == EditPhase::Inspect {
            &capabilities.inspect
        } else {
            &capabilities.propose
        };
        let phase_visible: Vec<_> = allowed
            .iter()
            .filter(|name| visible.contains(name))
            .cloned()
            .collect();
        // Rebuild from immutable request and host evidence; never replay decisions,
        // completion prose, or malformed model output as reasoning history.
        continuation.refresh(&mut *dispatch.relevant_state);
        messages.clear();
        if push_message(&mut messages, "system", system.clone(), budgets.transport).is_err()
            || push_message(&mut messages, "user", request.into(), budgets.transport).is_err()
            || push_message(
                &mut messages,
                "user",
                continuation.context(
                    run.state.phase(),
                    budgets.refinements.saturating_sub(run.refinements),
                    completion.is_some(),
                ),
                budgets.transport,
            )
            .is_err()
        {
            return run.finish(OperationBudgetExhausted);
        }
        if let Some(feedback) = &continuation.feedback
            && push_message(&mut messages, "user", feedback.clone(), budgets.transport).is_err()
        {
            return run.finish(OperationBudgetExhausted);
        }
        let context = json!({"kind":"edit_phase", "phase":format!("{:?}",run.state.phase()), "catalog":if compact_discovery { discovery_summary(&catalog, &phase_visible) } else { compact_catalog(&catalog, &phase_visible, projected_arguments) }, "remaining_budgets":{"inspections":budgets.inspections.saturating_sub(run.inspections),"evaluations":budgets.refinements.saturating_add(1).saturating_sub(run.evaluations),"refinements":budgets.refinements.saturating_sub(run.refinements),"model_invocations":budgets.model_invocations.min(budgets.transport.max_decisions).saturating_sub(run.model_invocations)}, "discovery":discovery.map(|options| json!({"name":DISCOVER_SEMANTIC_OPERATIONS,"arguments":if compact_discovery { json!({"name":"optional fully qualified operation name for canonical detail"}) } else { json!({}) },"page_size":options.page_size,"description":if compact_discovery { "Use {} to reveal the next summary page, or {\"name\":\"fully.qualified.operation\"} for full canonical metadata and schema. Detail does not reveal or authorize invocation. Both consume normal model and tool-call budgets." } else { "Reveal the next sorted page of eligible operations for this phase; no authorization granted. Accepts exactly an empty JSON object: arguments must be {}. page_size is host-owned metadata, not an accepted argument; do not supply page_size inside arguments. No other argument keys are accepted." }}))}).to_string();
        if push_message(&mut messages, "user", context, budgets.transport).is_err() {
            return run.finish(OperationBudgetExhausted);
        }
        run.model_invocations += 1;
        run.trace
            .push(EditEvent::ModelInvocation(run.model_invocations));
        let text = match model.decide(&messages, budgets.transport.max_tokens_per_decision) {
            Ok(text) => text,
            Err(_) => {
                run.trace.push(EditEvent::ModelCompleted {
                    ordinal: run.model_invocations,
                    phase: run.state.phase(),
                    completion: ModelCompletion::BackendFailure,
                });
                return run.finish(AgentBackendFailure);
            }
        };
        run.trace.push(EditEvent::ModelCompleted {
            ordinal: run.model_invocations,
            phase: run.state.phase(),
            completion: ModelCompletion::Response { bytes: text.len() },
        });
        if text.len() > budgets.transport.max_message_bytes {
            return run.finish(OperationBudgetExhausted);
        }
        let call = match parse_decision(&text) {
            Ok(Decision::Complete { content }) if completion.is_some() => {
                let assessment = completion.as_mut().unwrap()(&content, &run.evidence);
                run.conclusion = Some(content);
                let supported = run.evidence.iter().any(|e| {
                    field(&e.response, "status").and_then(ToolValue::as_str) == Some("read_only")
                });
                return run.finish(match assessment {
                    InvestigationCompletion::Completed if supported => InvestigationCompleted,
                    InvestigationCompletion::Unsupported => UnsupportedRequest,
                    _ => InvestigationInconclusive,
                });
            }
            Ok(Decision::Complete { .. }) if run.state.phase() == EditPhase::Inspect => {
                if refinement_pending {
                    return run.finish(ValidationFailure);
                }
                run.transition(EditPhase::Propose);
                continue;
            }
            Ok(Decision::Complete { .. }) => {
                run.last_result = Some(ToolValue::object([
                    ("status", "unsupported_request".into()),
                    ("phase", "Propose".into()),
                    ("message", "Model returned complete during Propose without creating a proposal; source in a completion is not an edit".into()),
                ]));
                return run.finish(UnsupportedRequest);
            }
            Ok(Decision::Tool { name, arguments }) => ToolCall { name, arguments },
            Err(error) => {
                run.trace.push(EditEvent::ProtocolError {
                    category: error.category().into(),
                });
                malformed += 1;
                run.last_result = Some(ToolValue::object([
                    ("status", "protocol_error".into()),
                    ("message", error.to_string().into()),
                    ("category", error.category().into()),
                    ("malformed_decisions", ToolValue::Integer(malformed as i64)),
                ]));
                if malformed > budgets.transport.max_malformed {
                    run.last_result = Some(ToolValue::object([
                        ("status", "protocol_error".into()),
                        ("category", error.category().into()),
                        ("message", "Protocol retry budget exhausted".into()),
                        ("error", error.to_string().into()),
                        ("retry_budget_exhausted", true.into()),
                        ("malformed_decisions", ToolValue::Integer(malformed as i64)),
                    ]));
                    run.trace.push(EditEvent::NoProgressReason {
                        reason: "protocol_retry_budget_exhausted".into(),
                    });
                    return run.finish(NoProgress);
                }
                run.trace.push(EditEvent::ProtocolRetry {
                    category: error.category().into(),
                    attempt: malformed,
                });
                let feedback = json!({"kind":"protocol_error", "error":error.to_string(), "category":error.category(),
                    "instruction":error.correction()}).to_string();
                continuation.feedback = Some(feedback);
                continue;
            }
        };
        if call.name == DISCOVER_SEMANTIC_OPERATIONS {
            let Some(options) = discovery else {
                return run.finish(AuthorizationFailure);
            };
            let detail_name = if compact_discovery && call.arguments.len() == 1 {
                call.arguments.get("name").and_then(|value| match value {
                    ToolValue::String(name) => Some(name.as_str()),
                    _ => None,
                })
            } else {
                None
            };
            if !call.arguments.is_empty() && detail_name.is_none() {
                run.trace.push(EditEvent::NoProgressReason {
                    reason: "invalid_discovery_arguments".into(),
                });
                return run.finish(NoProgress);
            }
            if calls.len().saturating_add(discovery_calls) >= budgets.transport.max_tool_calls {
                return run.finish(OperationBudgetExhausted);
            }
            discovery_calls += 1;
            run.trace.push(EditEvent::ToolCall {
                ordinal: calls.len() + discovery_calls,
                phase: run.state.phase(),
            });
            run.trace.push(EditEvent::ToolObserved {
                name: DISCOVER_SEMANTIC_OPERATIONS.into(),
                phase: run.state.phase(),
            });
            if let Some(name) = detail_name {
                continuation.feedback = Some(discovery_detail(&catalog, allowed, name).to_string());
                continue;
            }
            let revealed = reveal_operations(allowed, &mut visible, options.page_size);
            run.discovered_operations = visible.clone();
            run.trace.push(EditEvent::Discovery { revealed });
            if revealed == 0 {
                run.trace.push(EditEvent::NoProgressReason {
                    reason: "discovery_exhausted".into(),
                });
                return run.finish(NoProgress);
            }
            let names: Vec<_> = allowed
                .iter()
                .filter(|name| visible.contains(name))
                .cloned()
                .collect();
            let feedback =
                json!({"kind":"tool_result","ok":true,"value":if compact_discovery { discovery_summary(&catalog, &names) } else { compact_catalog(&catalog, &names, projected_arguments) }})
                    .to_string();
            continuation.feedback = Some(feedback);
            continue;
        }
        if !visible.contains(&call.name) {
            run.last_result = Some(ToolValue::object([
                ("status", "unauthorized".into()),
                ("tool", call.name.clone().into()),
                ("phase", format!("{:?}", run.state.phase()).into()),
                ("message", "Requested tool is not visible in this edit session; use an exact listed name or discover eligible operations first".into()),
                ("visible_operations", ToolValue::array(phase_visible.iter().cloned().map(ToolValue::from))),
            ]));
            return run.finish(AuthorizationFailure);
        }
        if !allowed.contains(&call.name) {
            run.last_result = Some(ToolValue::object([
                ("status", "unauthorized".into()),
                ("tool", call.name.clone().into()),
                ("phase", format!("{:?}", run.state.phase()).into()),
                ("message", "Requested tool is not permitted in the current phase; during Inspect use read tools or return complete to advance to Propose".into()),
                ("visible_operations", ToolValue::array(phase_visible.iter().cloned().map(ToolValue::from))),
            ]));
            if capabilities.inspect.contains(&call.name) && run.inspections >= budgets.inspections {
                return run.finish(OperationBudgetExhausted);
            }
            return run.finish(AuthorizationFailure);
        }
        let relevant_state = (dispatch.relevant_state)(&call);
        if calls
            .iter()
            .any(|(previous, state)| previous == &call && state == &relevant_state)
        {
            repeated += 1;
            run.trace.push(EditEvent::RepeatedCall);
            let instruction = if completion.is_some() {
                "This operation was already invoked with identical arguments in the same source context; it was not dispatched again. Its retained result is in edit_continuation.evidence. Choose a different permitted operation, meaningfully different arguments, or complete with your own conclusion or explicit uncertainty. The host assesses completion; existing evidence is not necessarily sufficient."
            } else {
                "This candidate or operation was already evaluated in the same source context. Reuse retained evidence; revise the candidate or expectations before evaluating again, or advance to Propose only if the evidence supports it."
            };
            let feedback = json!({"kind":"no_progress", "reason":"identical call on unchanged target; not dispatched", "instruction":instruction});
            continuation.feedback = Some(feedback.to_string());
            if repeated > budgets.repeated_calls {
                run.trace.push(EditEvent::NoProgressReason {
                    reason: "repeated_suppressed_operation".into(),
                });
                // Preserve the actual evaluation in last_result; suppression is not new evidence.
                return run.finish(NoProgress);
            }
            continue;
        }
        if calls.len().saturating_add(discovery_calls) >= budgets.transport.max_tool_calls {
            return run.finish(OperationBudgetExhausted);
        }
        let inspecting = run.state.phase() == EditPhase::Inspect;
        let evaluating = inspecting && is_evaluation(&call.name);
        if evaluating {
            if run.evaluations >= budgets.refinements.saturating_add(1) {
                return run.finish(RefinementBudgetExhausted);
            }
            if refinement_pending {
                if run.refinements >= budgets.refinements {
                    return run.finish(RefinementBudgetExhausted);
                }
                run.refinements += 1;
                run.trace
                    .push(EditEvent::RefinementAttempt(run.refinements));
                refinement_pending = false;
            }
            run.evaluations += 1;
            run.trace
                .push(EditEvent::CandidateEvaluation(run.evaluations));
        } else if inspecting {
            if run.inspections >= budgets.inspections {
                return run.finish(ExplorationBudgetExhausted);
            }
            run.inspections += 1;
            run.trace.push(EditEvent::Inspection(run.inspections));
        } else {
            let initial = run.proposal_attempts.saturating_sub(run.corrections);
            if run.corrections == 0 && initial >= budgets.proposals {
                return run.finish(OperationBudgetExhausted);
            }
            run.proposal_attempts += 1;
            run.trace
                .push(EditEvent::ProposalAttempt(run.proposal_attempts));
        }
        run.trace.push(EditEvent::ToolCall {
            ordinal: calls.len() + discovery_calls + 1,
            phase: run.state.phase(),
        });
        run.trace.push(EditEvent::ToolObserved {
            name: call.name.clone(),
            phase: run.state.phase(),
        });
        calls.push((call.clone(), relevant_state.clone()));
        let value = match session.invoke_with_call_policy(call.clone(), &mut *dispatch.policy) {
            Ok(result) => result.value,
            Err(error) => {
                let (classification, outcome) = match error {
                    ToolError::Denied { .. } => {
                        (ToolResultClassification::Unauthorized, AuthorizationFailure)
                    }
                    ToolError::UnknownTool { .. } => {
                        (ToolResultClassification::Unsupported, UnsupportedRequest)
                    }
                    _ => (
                        ToolResultClassification::InternalFailure,
                        AgentBackendFailure,
                    ),
                };
                run.trace.push(EditEvent::ToolResult { classification });
                return run.finish(outcome);
            }
        };
        run.evidence.push(InvestigationEvidence {
            call: call.clone(),
            source: relevant_state.clone(),
            response: value.clone(),
        });
        continuation.retain(call.clone(), relevant_state.clone(), value.clone());
        run.last_result = Some(value.clone());
        let status = field(&value, "status").and_then(ToolValue::as_str);
        run.trace.push(EditEvent::ToolResult {
            classification: ToolResultClassification::from_status(status),
        });
        run.trace.push(EditEvent::EvidenceRetained {
            count: continuation.evidence.len(),
        });
        if !results.contains(&value) {
            run.trace.push(EditEvent::NewToolResult);
            if inspecting
                && status == Some("read_only")
                && field(&value, "value").is_some_and(|information| {
                    !results
                        .iter()
                        .any(|previous| field(previous, "value") == Some(information))
                })
            {
                run.trace.push(EditEvent::NewSourceInformation);
            }
            results.push(value.clone());
        }
        let diagnostic_count = match field(&value, "diagnostics") {
            Some(ToolValue::Array(items)) => items.len(),
            _ => 0,
        };
        if let Some(ToolValue::Array(checks)) = field(&value, "checks") {
            for check in checks {
                run.trace.push(EditEvent::ValidationStatus {
                    status: field(check, "outcome")
                        .and_then(ToolValue::as_str)
                        .unwrap_or("NotEvaluated")
                        .to_owned(),
                    diagnostic_count: 0,
                });
            }
        }
        if let Some(validation_status) =
            field(&value, "validation_status").and_then(ToolValue::as_str)
        {
            run.trace.push(EditEvent::ValidationStatus {
                status: diagnostic_category(validation_status),
                diagnostic_count,
            });
        }
        match status {
            Some("blocked") => {
                let recoverable =
                    matches!(field(&value, "recoverable"), Some(ToolValue::Boolean(true)));
                run.trace.push(EditEvent::SemanticBlocker {
                    category: diagnostic_category(
                        field(&value, "blocker_category")
                            .and_then(ToolValue::as_str)
                            .unwrap_or("unknown"),
                    ),
                    recoverable,
                });
                // Inspection has no correction edge; retain the blocker evidence
                // rather than inventing a backwards planning transition.
                if !recoverable || inspecting {
                    return run.finish(SemanticBlocked);
                }
                run.transition(EditPhase::Validate);
                if run.corrections >= budgets.corrections {
                    return run.finish(SemanticBlocked);
                }
                run.corrections += 1;
                run.trace
                    .push(EditEvent::CorrectionAttempt(run.corrections));
                run.transition(EditPhase::Propose);
            }
            Some("unauthorized") => return run.finish(AuthorizationFailure),
            Some("unsupported_operation") => return run.finish(UnsupportedRequest),
            Some("ambiguous_target") => return run.finish(AmbiguousTarget),
            Some("revision_conflict" | "missing_prerequisite") => {
                return run.finish(MissingPrerequisite);
            }
            Some("internal_failure") => return run.finish(AgentBackendFailure),
            Some("proposal") if !inspecting => {
                let Some(id) = field(&value, "proposal_id")
                    .and_then(ToolValue::as_str)
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|id| *id != 0)
                    .map(ProposalId)
                else {
                    return run.finish(AgentBackendFailure);
                };
                run.proposal = Some(id);
                run.trace.push(if run.corrections == 0 {
                    EditEvent::ProposalProduced(id)
                } else {
                    EditEvent::RevisedProposalProduced(id)
                });
                run.transition(EditPhase::Validate);
                // Only explicit provider evidence establishes semantic acceptance.
                if matches!(
                    field(&value, "validation_status").and_then(ToolValue::as_str),
                    Some("Accepted" | "AcceptedWithLimitations")
                ) {
                    run.trace.push(EditEvent::ValidProposalReached(id));
                }
                return run.finish(ProposalReady);
            }
            Some("invalid_source" | "invalid_arguments" | "rejected" | "validation_failed")
                if !inspecting =>
            {
                run.transition(EditPhase::Validate);
                run.trace.push(EditEvent::ValidationFailure);
                // Explicit eligible rejection is actionable even without diagnostics.
                let rejected = matches!(status, Some("rejected" | "validation_failed"));
                let eligible = !rejected
                    || matches!(
                        field(&value, "correction_eligible"),
                        Some(ToolValue::Boolean(true))
                    );
                let meaningful = rejected
                    || diagnostic_count > 0
                    || field(&value, "message")
                        .and_then(ToolValue::as_str)
                        .is_some_and(|s| !s.trim().is_empty());
                if !eligible || !meaningful || run.corrections >= budgets.corrections {
                    if run.corrections >= budgets.corrections
                        && let Some(ToolValue::Object(fields)) = &mut run.last_result
                    {
                        fields.insert(
                            "refinement_stopped".into(),
                            "correction_budget_exhausted".into(),
                        );
                    }
                    return run.finish(ValidationFailure);
                }
                run.corrections += 1;
                run.trace
                    .push(EditEvent::CorrectionAttempt(run.corrections));
                run.transition(EditPhase::Propose);
            }
            Some("invalid_arguments") if inspecting => {
                // Invocation-shape correction is transport recovery, not a
                // semantic validation phase or permission expansion.
                malformed += 1;
                run.trace.push(EditEvent::ProtocolError {
                    category: "invalid_arguments".into(),
                });
                if malformed > budgets.transport.max_malformed
                    || run.inspections >= budgets.inspections
                {
                    return run.finish(ValidationFailure);
                }
                run.trace.push(EditEvent::ProtocolRetry {
                    category: "invalid_arguments".into(),
                    attempt: malformed,
                });
            }
            Some("read_only") if inspecting => {
                // Effect is not semantic acceptance. Only the typed provider
                // evaluation envelope establishes check outcomes; opaque values
                // (including language-specific booleans) are never interpreted.
                if let Some(evaluation) = field(&value, "evaluation") {
                    let outcome = field(evaluation, "outcome").and_then(ToolValue::as_str);
                    if evaluating {
                        continuation.refinement =
                            Some((call.clone(), relevant_state.clone(), value.clone()));
                    }
                    let established = match outcome {
                        Some("Satisfied") => true,
                        Some(_) => false,
                        None => {
                            matches!(field(evaluation, "checks"), Some(ToolValue::Array(checks))
                            if !checks.is_empty() && checks.iter().all(|check|
                                field(check, "outcome").and_then(ToolValue::as_str) == Some("Passed")))
                        }
                    };
                    if !established && completion.is_none() {
                        if evaluating {
                            refinement_pending = true;
                            if run.refinements >= budgets.refinements
                                || run.evaluations >= budgets.refinements.saturating_add(1)
                            {
                                if let Some(ToolValue::Object(fields)) = &mut run.last_result {
                                    fields.insert(
                                        "refinement_stopped".into(),
                                        "refinement_budget_exhausted".into(),
                                    );
                                }
                                return run.finish(RefinementBudgetExhausted);
                            }
                            // The refinement handoff needs only the evaluated request and its evidence.
                            continuation
                                .evidence
                                .retain(|(previous, _, _)| previous == &call);
                        }
                        if !evaluating && run.inspections >= budgets.inspections {
                            if let Some(ToolValue::Object(fields)) = &mut run.last_result {
                                fields.insert(
                                    "refinement_stopped".into(),
                                    "inspection_budget_exhausted".into(),
                                );
                            }
                            return run.finish(ValidationFailure);
                        }
                        let (message, instruction) = match outcome {
                            Some("Violated") => (
                                "Candidate evaluation violated the evaluated criteria.",
                                "Revise the candidate and evaluate again. Use the failed criteria and concrete provider observations retained in evidence; do not repeat an identical call.",
                            ),
                            Some("Inconclusive") => (
                                "Candidate validation remains unresolved; evaluation was inconclusive.",
                                "Resolve the validation uncertainty using the provider summary and diagnostics, then evaluate again. Do not treat inconclusive evidence as satisfaction or assume the candidate is violated; do not repeat an identical call.",
                            ),
                            _ => (
                                "Candidate evaluation did not establish required behavioral checks.",
                                "Revise the candidate and evaluate again. Use the concrete provider observations retained in evidence; do not repeat an identical call.",
                            ),
                        };
                        continuation.feedback = Some(
                            json!({
                                "kind":"candidate_evaluation_feedback",
                                "message":message,
                                "candidate_call":{"name":call.name,"arguments":to_json(ToolValue::Object(call.arguments.clone()))},
                                "remaining_refinements":budgets.refinements.saturating_sub(run.refinements),
                                "next_action":"revise_candidate_and_evaluate",
                                "evaluation":to_json(evaluation.clone()),
                                "checks":field(&value, "checks").map(|checks| to_json(checks.clone())),
                                "candidate_lifecycle":"Disposable evaluation only: candidate definitions are not installed and active source is unchanged. Use the exact candidate and failed checks, including actual and expected outputs, in retained evidence.",
                                "next_permissible_actions":["evaluate a revised candidate", "inspect existing document definitions if needed", "complete to advance to Propose only when evidence supports the candidate"],
                                "instruction":instruction
                            })
                            .to_string(),
                        );
                    }
                }
            }
            _ => {
                return run.finish(if inspecting {
                    UnsupportedRequest
                } else {
                    ValidationFailure
                });
            }
        }

        // Inspection is host-bounded; the last permitted read automatically
        // advances rather than spending another decision to restate a plan.
        if completion.is_none()
            && inspecting
            && run.inspections >= budgets.inspections
            && !has_evaluation
        {
            run.transition(EditPhase::Propose);
        }
    }
}

#[cfg(test)]
#[path = "edit_observability_tests.rs"]
mod observability_tests;

/// An immutable invocation record owned by the returned run.
#[derive(Debug, Clone)]
pub struct InvestigationEvidence {
    pub call: ToolCall,
    pub source: ToolValue,
    pub response: ToolValue,
}

/// Host assessment, not a model assertion or a proof of language correctness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvestigationCompletion {
    Completed,
    Inconclusive,
    Unsupported,
}
type CompletionPolicy<'a> =
    dyn FnMut(&str, &[InvestigationEvidence]) -> InvestigationCompletion + 'a;

#[derive(Debug, Default)]
pub struct SemanticInvestigationOptions {
    /// Explicitly exposed read operations; independent authorization still applies.
    pub operations: Vec<String>,
    pub budgets: EditBudgets,
}

/// Investigate a bound snapshot without proposal authority. The host must assess
/// the conclusion against retained evidence. Missing successful read evidence
/// always yields an inconclusive result, even if the callback accepts it.
#[allow(clippy::too_many_arguments)]
pub fn run_semantic_investigation_with_discovery<T: SourceTarget>(
    model: &mut (impl DecisionModel + ?Sized),
    controller: &mut SourceEditController,
    target: &T,
    service: &dyn scope_language::LanguageService,
    request: AgentEditRequest<'_>,
    options: &SemanticInvestigationOptions,
    discovery: &SemanticDiscoveryOptions,
    policy: impl for<'a> FnMut(crate::tool::semantic::SemanticAuthorization<'a>) -> bool,
    mut completion: impl FnMut(&str, &[InvestigationEvidence]) -> InvestigationCompletion,
) -> EditRun {
    let options = SemanticEditOptions {
        capabilities: EditCapabilities {
            inspect: options.operations.clone(),
            propose: Vec::new(),
            inspect_first: true,
        },
        budgets: options.budgets,
    };
    run_semantic_objective_inventory(
        model,
        controller,
        target,
        service,
        request,
        &options,
        policy,
        Some(discovery),
        Some(&mut completion),
        false,
    )
}

/// Opt-in two-level discovery; otherwise identical to the investigation runner.
#[allow(clippy::too_many_arguments)]
pub fn run_semantic_investigation_with_compact_discovery<T: SourceTarget>(
    model: &mut (impl DecisionModel + ?Sized),
    controller: &mut SourceEditController,
    target: &T,
    service: &dyn scope_language::LanguageService,
    request: AgentEditRequest<'_>,
    options: &SemanticInvestigationOptions,
    discovery: &SemanticDiscoveryOptions,
    policy: impl for<'a> FnMut(crate::tool::semantic::SemanticAuthorization<'a>) -> bool,
    mut completion: impl FnMut(&str, &[InvestigationEvidence]) -> InvestigationCompletion,
) -> EditRun {
    let options = SemanticEditOptions {
        capabilities: EditCapabilities {
            inspect: options.operations.clone(),
            propose: Vec::new(),
            inspect_first: true,
        },
        budgets: options.budgets,
    };
    run_semantic_objective_inventory(
        model,
        controller,
        target,
        service,
        request,
        &options,
        policy,
        Some(discovery),
        Some(&mut completion),
        true,
    )
}

/// Execution policy for the M1/M2 convenience entry point.
#[derive(Debug, Default)]
pub struct SemanticEditOptions {
    pub capabilities: EditCapabilities,
    pub budgets: EditBudgets,
}

/// Construct the existing M2 adapter against the actual M1 target and registry.
/// The request must come from the host's resolved target. No extra providers or
/// authority are installed. The adapter borrow is released before returning for
/// normal `controller.get/diff/accept/reject` review.
pub fn run_semantic_edit<T: SourceTarget>(
    model: &mut (impl DecisionModel + ?Sized),
    controller: &mut SourceEditController,
    target: &T,
    service: &dyn scope_language::LanguageService,
    request: AgentEditRequest<'_>,
    options: &SemanticEditOptions,
    policy: impl for<'a> FnMut(crate::tool::semantic::SemanticAuthorization<'a>) -> bool,
) -> EditRun {
    run_semantic_edit_inventory(
        model, controller, target, service, request, options, policy, None,
    )
}

/// Contextual M4 entrypoint. The selected service, selection prerequisites and
/// explicit capabilities determine eligibility; host intent determines initial visibility.
#[allow(
    clippy::too_many_arguments,
    reason = "Preserve the semantic edit API with explicit host-owned discovery and authorization policy"
)]
pub fn run_semantic_edit_with_discovery<T: SourceTarget>(
    model: &mut (impl DecisionModel + ?Sized),
    controller: &mut SourceEditController,
    target: &T,
    service: &dyn scope_language::LanguageService,
    request: AgentEditRequest<'_>,
    options: &SemanticEditOptions,
    discovery: &SemanticDiscoveryOptions,
    policy: impl for<'a> FnMut(crate::tool::semantic::SemanticAuthorization<'a>) -> bool,
) -> EditRun {
    run_semantic_edit_inventory(
        model,
        controller,
        target,
        service,
        request,
        options,
        policy,
        Some(discovery),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Shared implementation mirrors the semantic edit entry points with optional discovery"
)]
fn run_semantic_edit_inventory<T: SourceTarget>(
    model: &mut (impl DecisionModel + ?Sized),
    controller: &mut SourceEditController,
    target: &T,
    service: &dyn scope_language::LanguageService,
    request: AgentEditRequest<'_>,
    options: &SemanticEditOptions,
    policy: impl for<'a> FnMut(crate::tool::semantic::SemanticAuthorization<'a>) -> bool,
    discovery: Option<&SemanticDiscoveryOptions>,
) -> EditRun {
    run_semantic_objective_inventory(
        model, controller, target, service, request, options, policy, discovery, None, false,
    )
}

fn run_semantic_objective_inventory<T: SourceTarget>(
    model: &mut (impl DecisionModel + ?Sized),
    controller: &mut SourceEditController,
    target: &T,
    service: &dyn scope_language::LanguageService,
    request: AgentEditRequest<'_>,
    options: &SemanticEditOptions,
    policy: impl for<'a> FnMut(crate::tool::semantic::SemanticAuthorization<'a>) -> bool,
    discovery: Option<&SemanticDiscoveryOptions>,
    completion: Option<&mut CompletionPolicy<'_>>,
    compact_discovery: bool,
) -> EditRun {
    let snapshot = ToolValue::object([
        ("source_id", format!("{:?}", target.source_id()).into()),
        ("version", target.version().to_string().into()),
        ("source", target.text().into()),
    ]);
    let result = (|| {
        if request.scope != PermittedEditScope::Document(target.source_id())
            || request.source.id != target.source_id()
        {
            return EditRun::default().finish(EditOutcome::AuthorizationFailure);
        }
        if request.source.version != target.version()
            || request.source.text != target.text()
            || request.source.language != target.language_id()
            || request.instruction.trim().is_empty()
        {
            return EditRun::default().finish(EditOutcome::MissingPrerequisite);
        }
        let task_context = controller.task_contract().map(|task| task.context());
        let provider = match crate::tool::semantic::SemanticToolProvider::new(
            service,
            target,
            controller,
            request.scope,
            request.selection,
            policy,
        ) {
            Ok(provider) => provider,
            Err(ToolError::Denied { .. }) => {
                return EditRun::default().finish(EditOutcome::AuthorizationFailure);
            }
            Err(_) => return EditRun::default().finish(EditOutcome::MissingPrerequisite),
        };
        let provider = match provider.with_projected_arguments() {
            Ok(provider) => provider,
            Err(_) => return EditRun::default().finish(EditOutcome::MissingPrerequisite),
        };
        let mut session = match AgentSession::builder().provider(provider).build() {
            Ok(session) => session,
            Err(_) => return EditRun::default().finish(EditOutcome::MissingPrerequisite),
        };
        let selection = request.selection.map(|range| json!({"start":{"line":range.start.line,"column":range.start.column},"end":{"line":range.end.line,"column":range.end.column}}));
        let context = json!({"kind":"edit_request", "instruction":request.instruction, "target_buffer":target.source_id().raw(), "target_kind":match target.source_id() { SourceId::Scratch(_) => "scratch", SourceId::Document(_) => "document" }, "language":target.language_id(), "expected_version":target.version().to_string(), "expected_source":target.text(), "selection":selection, "scope":"single_document", "task_contract":task_context}).to_string();
        let operations = service.semantic_operations();
        let eligible = |names: &[String], effect| {
            names
                .iter()
                .filter(|name| {
                    operations.iter().any(|operation| {
                        **name == format!("language.{}.{}", service.language_id(), operation.id)
                            && operation.effect == effect
                            && (operation.target
                                == scope_language::SemanticTargetRequirement::Document
                                || request.selection.is_some())
                    })
                })
                .cloned()
                .collect()
        };
        let capabilities = EditCapabilities {
            inspect: eligible(
                &options.capabilities.inspect,
                scope_language::SemanticOperationEffect::Read,
            ),
            propose: eligible(
                &options.capabilities.propose,
                scope_language::SemanticOperationEffect::ProposeChanges,
            ),
            inspect_first: options.capabilities.inspect_first,
        };
        run_objective_session_inventory(
            model,
            &mut session,
            Ok(target.source_id()),
            &context,
            &capabilities,
            options.budgets,
            EditDispatch {
                policy: &mut |_, _| true,
                relevant_state: &mut |_| {
                    ToolValue::object([
                        ("source_id", format!("{:?}", target.source_id()).into()),
                        ("version", target.version().to_string().into()),
                        ("source", target.text().into()),
                    ])
                },
            },
            discovery,
            completion,
            true,
            compact_discovery,
        )
    })();
    EditRun {
        source_snapshot: Some(snapshot),
        ..result
    }
}

#[cfg(test)]
mod continuation_tests {
    use super::*;
    use crate::tool::{ToolProvider, ToolResult};

    #[test]
    fn evidence_is_keyed_by_call_and_state_without_invented_candidate_facts() {
        let call = ToolCall {
            name: "read".into(),
            arguments: Default::default(),
        };
        let mut continuation = EditContinuation::default();
        continuation.retain(call.clone(), "revision-one".into(), "old evidence".into());
        continuation.retain(
            call.clone(),
            "revision-one".into(),
            "replacement evidence".into(),
        );
        continuation.retain(call, "revision-two".into(), "new evidence".into());
        let context: serde_json::Value = serde_json::from_str(&continuation.context(
            EditPhase::Propose,
            EditBudgets::default().refinements,
            false,
        ))
        .unwrap();
        assert_eq!(context["evidence"].as_array().unwrap().len(), 2);
        assert_eq!(context["evidence"][0]["value"], "replacement evidence");
        assert_eq!(context["evidence"][1]["relevant_state"], "revision-two");
        assert!(context.get("candidate").is_none());
        assert!(context.get("evaluation").is_none());
        assert_eq!(context["next_action"], "invoke_permitted_proposal_tool");
        assert_eq!(context["completion"], "unsupported_without_proposal");
        let mut first = ToolCall {
            name: "edit".into(),
            arguments: Default::default(),
        };
        first.arguments.insert("candidate".into(), "one".into());
        let mut second = first.clone();
        second.arguments.insert("candidate".into(), "two".into());
        continuation.retain(
            first,
            "revision-two".into(),
            ToolValue::object([
                ("status", "rejected".into()),
                ("validation_status", "Rejected".into()),
            ]),
        );
        continuation.retain(
            second,
            "revision-two".into(),
            ToolValue::object([
                ("status", "proposal".into()),
                ("validation_status", "NotEvaluated".into()),
            ]),
        );
        let context: serde_json::Value = serde_json::from_str(&continuation.context(
            EditPhase::Propose,
            EditBudgets::default().refinements,
            false,
        ))
        .unwrap();
        assert_eq!(
            context["evidence"][2]["call"]["arguments"]["candidate"],
            "one"
        );
        assert_eq!(
            context["evidence"][3]["call"]["arguments"]["candidate"],
            "two"
        );
        assert_eq!(
            context["evidence"][3]["value"]["validation_status"],
            "NotEvaluated"
        );
    }

    fn repeat_run(changed: bool) {
        use std::{cell::Cell, rc::Rc};
        struct Provider {
            tools: Vec<ToolDefinition>,
            state: Rc<Cell<usize>>,
            calls: Rc<Cell<usize>>,
            changed: bool,
        }
        impl ToolProvider for Provider {
            fn tools(&self) -> &[ToolDefinition] {
                &self.tools
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                self.calls.set(self.calls.get() + 1);
                let evidence = format!("evidence-{}", self.state.get());
                if self.changed {
                    self.state.set(1);
                }
                Ok(ToolResult::new(ToolValue::object([
                    ("status", "read_only".into()),
                    ("value", evidence.into()),
                ])))
            }
        }
        struct Model {
            step: usize,
            changed: bool,
        }
        impl DecisionModel for Model {
            fn decide(
                &mut self,
                messages: &[crate::agent::DecisionMessage],
                _: u32,
            ) -> Result<String, String> {
                assert_eq!(messages[1].content, "original source");
                let context: serde_json::Value =
                    serde_json::from_str(&messages[2].content).unwrap();
                if self.step == 1 && self.changed {
                    assert!(context["evidence"].as_array().unwrap().is_empty());
                    assert_eq!(context["invalidation"]["removed_results"], 1);
                    assert!(!messages[2].content.contains("evidence-0"));
                } else if self.step > 0 {
                    assert_eq!(context["evidence"].as_array().unwrap().len(), 1);
                    assert_eq!(
                        context["evidence"][0]["value"]["value"],
                        if self.changed {
                            "evidence-1"
                        } else {
                            "evidence-0"
                        }
                    );
                    assert!(
                        context["evidence"][0]["relevant_state"]
                            .get("source")
                            .is_none()
                    );
                    assert_eq!(
                        context["evidence"][0]["relevant_state"]["source_id"],
                        "scratch-1"
                    );
                    assert_eq!(context["evidence"][0]["relevant_state"]["version"], "7");
                }
                let decision = if self.step < 2 {
                    json!({"type":"tool","name":"read","arguments":{}})
                } else {
                    json!({"type":"complete","content":"done"})
                };
                self.step += 1;
                Ok(decision.to_string())
            }
        }
        let state = Rc::new(Cell::new(0));
        let calls = Rc::new(Cell::new(0));
        let mut session = AgentSession::builder()
            .provider(Provider {
                tools: vec![
                    ToolDefinition::new("read", "read"),
                    ToolDefinition::new("edit", "edit").effect(ToolEffect::Write),
                ],
                state: state.clone(),
                calls: calls.clone(),
                changed,
            })
            .build()
            .unwrap();
        let run = run_edit_session_with_state(
            &mut Model { step: 0, changed },
            &mut session,
            Ok(SourceId::Scratch(1)),
            "original source",
            &EditCapabilities {
                inspect: vec!["read".into()],
                propose: vec!["edit".into()],
                inspect_first: true,
            },
            EditBudgets {
                repeated_calls: 1,
                ..Default::default()
            },
            EditDispatch {
                policy: &mut |_, _| true,
                relevant_state: &mut |_| {
                    ToolValue::object([
                        ("source_id", "scratch-1".into()),
                        ("version", "7".into()),
                        ("source", format!("source-{}", state.get()).into()),
                    ])
                },
            },
        );
        assert_eq!(run.outcome, EditOutcome::UnsupportedRequest);
        assert_eq!(calls.get(), if changed { 2 } else { 1 });
        assert_eq!(run.trace.contains(&EditEvent::RepeatedCall), !changed);
    }

    #[test]
    fn unchanged_evidence_is_reused_without_repeat_dispatch() {
        repeat_run(false);
    }

    #[test]
    fn changed_source_invalidates_evidence_even_at_same_version() {
        repeat_run(true);
    }

    #[test]
    fn compact_handoff_preserves_reads_request_and_authorization() {
        struct Provider(Vec<ToolDefinition>);
        impl ToolProvider for Provider {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }
            fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
                assert_eq!(call.name, "read");
                Ok(ToolResult::new(ToolValue::object([
                    ("status", "read_only".into()),
                    ("value", "provider evidence".into()),
                ])))
            }
        }
        struct Model {
            step: usize,
        }
        impl DecisionModel for Model {
            fn decide(
                &mut self,
                messages: &[crate::agent::DecisionMessage],
                _: u32,
            ) -> Result<String, String> {
                assert!(messages.iter().all(|message| message.role != "assistant"));
                assert_eq!(
                    messages[1].content,
                    "original request and source preconditions"
                );
                let context: serde_json::Value =
                    serde_json::from_str(&messages[2].content).unwrap();
                assert_eq!(messages.len(), 4);
                let decision = match self.step {
                    0 => json!({"type":"tool","name":"read","arguments":{}}),
                    1 => {
                        assert_eq!(context["phase"], "Inspect");
                        assert_eq!(
                            context["evidence"][0]["value"]["value"],
                            "provider evidence"
                        );
                        json!({"type":"complete","content":"private completion prose must not be retained"})
                    }
                    2 => {
                        assert_eq!(context["phase"], "Propose");
                        assert_eq!(context["evidence"][0]["call"]["name"], "read");
                        assert!(
                            !messages.iter().any(|message| message
                                .content
                                .contains("private completion prose"))
                        );
                        json!({"type":"tool","name":"edit","arguments":{}})
                    }
                    _ => unreachable!(),
                };
                self.step += 1;
                Ok(decision.to_string())
            }
        }
        let mut session = AgentSession::builder()
            .provider(Provider(vec![
                ToolDefinition::new("read", "read"),
                ToolDefinition::new("edit", "edit").effect(ToolEffect::Write),
            ]))
            .build()
            .unwrap();
        let mut authorized = Vec::new();
        let run = run_edit_session(
            &mut Model { step: 0 },
            &mut session,
            Ok(SourceId::Scratch(1)),
            "original request and source preconditions",
            &EditCapabilities {
                inspect: vec!["read".into()],
                propose: vec!["edit".into()],
                inspect_first: true,
            },
            EditBudgets::default(),
            |_, call| {
                authorized.push(call.name.clone());
                call.name == "read"
            },
        );
        assert_eq!(authorized, ["read", "edit"]);
        assert_eq!(run.outcome, EditOutcome::AuthorizationFailure);
        assert_eq!(run.inspections, 1);
    }
}

#[cfg(test)]
mod inventory_tests {
    use super::*;
    use crate::tool::{ToolProvider, ToolResult};

    struct Model(Vec<String>);
    impl DecisionModel for Model {
        fn decide(
            &mut self,
            _: &[crate::agent::DecisionMessage],
            _: u32,
        ) -> Result<String, String> {
            Ok(self.0.remove(0))
        }
    }
    struct Provider(Vec<ToolDefinition>);
    impl ToolProvider for Provider {
        fn tools(&self) -> &[ToolDefinition] {
            &self.0
        }
        fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
            panic!("invisible or discovery call reached provider")
        }
    }
    fn execute(decisions: Vec<String>, budget: usize) -> EditRun {
        let mut session = AgentSession::builder()
            .provider(Provider(vec![
                ToolDefinition::new("language.test.replace", "replace").effect(ToolEffect::Write),
            ]))
            .build()
            .unwrap();
        let mut budgets = EditBudgets::default();
        budgets.transport.max_tool_calls = budget;
        run_edit_session_inventory(
            &mut Model(decisions),
            &mut session,
            Ok(SourceId::Scratch(1)),
            "replace",
            &EditCapabilities {
                propose: vec!["language.test.replace".into()],
                ..Default::default()
            },
            budgets,
            EditDispatch {
                policy: &mut |_, _| panic!("policy must not run"),
                relevant_state: &mut |_| panic!("state must not run"),
            },
            Some(&SemanticDiscoveryOptions {
                initial_operations: vec![],
                page_size: 1,
            }),
        )
    }
    #[test]
    fn compact_summary_and_detail_are_canonical_and_bounded() {
        let session = AgentSession::builder()
            .provider(Provider(vec![ToolDefinition::new(
                "language.test.read",
                "é".repeat(200),
            )]))
            .build()
            .unwrap();
        let catalog = session.catalog();
        let names = vec!["language.test.read".into()];
        let summary = discovery_summary(&catalog, &names);
        assert_eq!(summary["tools"][0]["description_truncated"], true);
        assert_eq!(
            summary["tools"][0]["description"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            161
        );
        assert!(summary["tools"][0].get("inputs").is_none());
        for key in ["target", "purpose", "produces_evaluation"] {
            assert_eq!(summary["tools"][0].get(key), Some(&serde_json::Value::Null));
        }
        let detail = discovery_detail(&catalog, &names, "language.test.read");
        assert_eq!(
            detail["value"]["tools"][0],
            to_json(catalog.to_value())["tools"][0]
        );
        assert_eq!(discovery_detail(&catalog, &names, "missing")["ok"], false);
        assert_eq!(
            discovery_detail(&catalog, &[], "language.test.read")["ok"],
            false
        );
    }

    #[test]
    fn compact_detail_never_reveals_or_dispatches_and_consumes_budget() {
        let detail = json!({"type":"tool", "name":DISCOVER_SEMANTIC_OPERATIONS,
            "arguments":{"name":"language.test.replace"}})
        .to_string();
        for budget in [0, 1] {
            let mut session = AgentSession::builder()
                .provider(Provider(vec![
                    ToolDefinition::new("language.test.replace", "replace")
                        .effect(ToolEffect::Write),
                ]))
                .build()
                .unwrap();
            let mut budgets = EditBudgets::default();
            budgets.transport.max_tool_calls = budget;
            let run = run_objective_session_inventory(
                &mut Model(vec![detail.clone(), detail.clone()]),
                &mut session,
                Ok(SourceId::Scratch(1)),
                "replace",
                &EditCapabilities {
                    propose: vec!["language.test.replace".into()],
                    ..Default::default()
                },
                budgets,
                EditDispatch {
                    policy: &mut |_, _| panic!("detail must not authorize"),
                    relevant_state: &mut |_| panic!("detail must not inspect"),
                },
                Some(&SemanticDiscoveryOptions {
                    initial_operations: vec![],
                    page_size: 1,
                }),
                None,
                false,
                true,
            );
            assert_eq!(run.outcome, EditOutcome::OperationBudgetExhausted);
            assert!(run.discovered_operations.is_empty());
            assert_eq!(run.inspections, 0);
            assert_eq!(run.model_invocations, budget + 1);
        }
    }

    fn discover() -> String {
        json!({"type":"tool","name":DISCOVER_SEMANTIC_OPERATIONS,"arguments":{}}).to_string()
    }

    fn status_run(replies: Vec<ToolValue>, inspecting: bool) -> EditRun {
        struct StatusProvider {
            definitions: Vec<ToolDefinition>,
            replies: std::collections::VecDeque<ToolValue>,
        }
        impl ToolProvider for StatusProvider {
            fn tools(&self) -> &[ToolDefinition] {
                &self.definitions
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::new(self.replies.pop_front().unwrap()))
            }
        }
        let decisions = (0..replies.len())
            .map(|attempt| json!({"type":"tool","name":if inspecting {"read"} else {"edit"},"arguments":{"attempt":attempt}}).to_string())
            .collect();
        let mut session = AgentSession::builder()
            .provider(StatusProvider {
                definitions: vec![
                    ToolDefinition::new("read", "inspect"),
                    ToolDefinition::new("edit", "propose").effect(ToolEffect::Write),
                ],
                replies: replies.into(),
            })
            .build()
            .unwrap();
        run_edit_session(
            &mut Model(decisions),
            &mut session,
            Ok(SourceId::Scratch(1)),
            "replace",
            &EditCapabilities {
                inspect: vec!["read".into()],
                propose: vec!["edit".into()],
                inspect_first: inspecting,
            },
            EditBudgets::default(),
            |_, _| true,
        )
    }
    fn blocked(recoverable: bool) -> ToolValue {
        ToolValue::object([
            ("status", "blocked".into()),
            ("recoverable", recoverable.into()),
            ("blocker_category", "dependency".into()),
            ("diagnostics", ToolValue::Array(vec![])),
            ("missing_dependencies", ToolValue::array(["library".into()])),
        ])
    }
    fn rejected(eligible: bool) -> ToolValue {
        ToolValue::object([
            ("status", "rejected".into()),
            ("validation_status", "Rejected".into()),
            ("correction_eligible", eligible.into()),
            ("diagnostics", ToolValue::Array(vec![])),
        ])
    }
    fn validated_proposal(status: &str) -> ToolValue {
        ToolValue::object([
            ("status", "proposal".into()),
            ("proposal_id", "1".into()),
            ("validation_status", status.into()),
            ("diagnostics", ToolValue::array(["limitation".into()])),
        ])
    }
    #[test]
    fn semantic_blocks_stop_or_use_the_shared_correction_budget() {
        for (recoverable, inspecting) in [(false, false), (true, true)] {
            let evidence = blocked(recoverable);
            let run = status_run(vec![evidence.clone()], inspecting);
            assert_eq!(run.outcome, EditOutcome::SemanticBlocked);
            assert_eq!(run.model_invocations, 1);
            assert_eq!(run.corrections, 0);
            assert_eq!(run.last_result, Some(evidence));
            assert!(run.trace.contains(&EditEvent::SemanticBlocker {
                category: "dependency".into(),
                recoverable,
            }));
        }
        let run = status_run(vec![blocked(true), blocked(true)], false);
        assert_eq!(run.outcome, EditOutcome::SemanticBlocked);
        assert_eq!(run.corrections, 1);
        assert_eq!(run.proposal_attempts, 2);
        assert!(run.trace.contains(&EditEvent::CorrectionAttempt(1)));
    }
    #[test]
    fn rejection_without_diagnostics_is_actionable_but_bounded() {
        let run = status_run(vec![rejected(true), rejected(true)], false);
        assert_eq!(run.outcome, EditOutcome::ValidationFailure);
        assert_eq!(run.corrections, 1);
        assert_eq!(run.proposal_attempts, 2);
        assert_eq!(run.model_invocations, 2);
        assert_eq!(run.proposal, None);
        let mut expected = rejected(true);
        if let ToolValue::Object(fields) = &mut expected {
            fields.insert(
                "refinement_stopped".into(),
                "correction_budget_exhausted".into(),
            );
        }
        assert_eq!(run.last_result, Some(expected.clone()));
        assert!(run.trace.contains(&EditEvent::ValidationStatus {
            status: "Rejected".into(),
            diagnostic_count: 0,
        }));
        let run = status_run(vec![rejected(false)], false);
        assert_eq!(run.outcome, EditOutcome::ValidationFailure);
        assert_eq!(run.corrections, 0);
        assert_eq!(run.last_result, Some(rejected(false)));
        let run = status_run(vec![blocked(true), rejected(true)], false);
        assert_eq!(run.outcome, EditOutcome::ValidationFailure);
        assert_eq!(run.corrections, 1);
        assert_eq!(run.last_result, Some(expected));
    }
    fn validation_failed(eligible: Option<bool>) -> ToolValue {
        let mut fields = vec![
            ("status", "validation_failed".into()),
            ("validation_status", "Rejected".into()),
            ("diagnostics", ToolValue::Array(vec![])),
        ];
        if let Some(eligible) = eligible {
            fields.push(("correction_eligible", eligible.into()));
        }
        ToolValue::object(fields)
    }
    #[test]
    fn validation_failed_obeys_eligibility_and_shared_correction_budget() {
        for eligible in [None, Some(false)] {
            let evidence = validation_failed(eligible);
            let run = status_run(vec![evidence.clone()], false);
            assert_eq!(run.outcome, EditOutcome::ValidationFailure);
            assert_eq!(run.corrections, 0);
            assert_eq!(run.model_invocations, 1);
            assert_eq!(run.last_result, Some(evidence));
            assert!(run.trace.contains(&EditEvent::ToolResult {
                classification: ToolResultClassification::Rejected,
            }));
        }
        for first in [validation_failed(Some(true)), rejected(true), blocked(true)] {
            let mut evidence = validation_failed(Some(true));
            let run = status_run(vec![first, evidence.clone()], false);
            if let ToolValue::Object(fields) = &mut evidence {
                fields.insert(
                    "refinement_stopped".into(),
                    "correction_budget_exhausted".into(),
                );
            }
            assert_eq!(run.outcome, EditOutcome::ValidationFailure);
            assert_eq!(run.corrections, 1);
            assert_eq!(run.proposal_attempts, 2);
            assert_eq!(run.model_invocations, 2);
            assert_eq!(run.proposal, None);
            assert_eq!(run.last_result, Some(evidence));
            assert_eq!(
                run.trace
                    .iter()
                    .filter(|event| matches!(event, EditEvent::CorrectionAttempt(_)))
                    .count(),
                1,
            );
        }
    }
    #[test]
    fn validation_failed_correction_stops_at_proposal_ready() {
        let evidence = validated_proposal("Accepted");
        let run = status_run(vec![validation_failed(Some(true)), evidence.clone()], false);
        assert_eq!(run.outcome, EditOutcome::ProposalReady);
        assert_eq!(run.proposal, Some(ProposalId(1)));
        assert_eq!(run.corrections, 1);
        assert_eq!(run.proposal_attempts, 2);
        assert_eq!(run.model_invocations, 2);
        assert_eq!(run.last_result, Some(evidence));
        assert!(run.trace.contains(&EditEvent::ToolResult {
            classification: ToolResultClassification::Rejected,
        }));
        assert!(run.trace.contains(&EditEvent::ValidationFailure));
        assert!(run.trace.contains(&EditEvent::CorrectionAttempt(1)));
        assert!(
            run.trace
                .contains(&EditEvent::RevisedProposalProduced(ProposalId(1)))
        );
        assert!(
            run.trace
                .contains(&EditEvent::ValidProposalReached(ProposalId(1)))
        );
        assert_eq!(
            run.trace.last(),
            Some(&EditEvent::Terminated(EditOutcome::ProposalReady))
        );
    }
    #[test]
    fn corrected_proposals_preserve_validation_evidence() {
        for status in ["Accepted", "AcceptedWithLimitations", "NotEvaluated"] {
            let evidence = validated_proposal(status);
            let run = status_run(vec![rejected(true), evidence.clone()], false);
            assert_eq!(run.outcome, EditOutcome::ProposalReady);
            assert_eq!(run.last_result, Some(evidence));
            assert_eq!(run.corrections, 1);
            assert!(run.trace.contains(&EditEvent::ValidationStatus {
                status: status.into(),
                diagnostic_count: 1,
            }));
            assert_eq!(
                run.trace
                    .contains(&EditEvent::ValidProposalReached(ProposalId(1))),
                status != "NotEvaluated",
            );
        }
        let run = status_run(vec![blocked(true), validated_proposal("Accepted")], false);
        assert_eq!(run.outcome, EditOutcome::ProposalReady);
        assert_eq!(run.corrections, 1);
    }
    #[test]
    fn protocol_retry_exhaustion_is_explicit() {
        let run = execute(vec!["not json".into(); 3], 2);
        assert_eq!(run.outcome, EditOutcome::NoProgress);
        let result = run.last_result.unwrap();
        assert_eq!(
            field(&result, "retry_budget_exhausted"),
            Some(&ToolValue::Boolean(true))
        );
        assert_eq!(
            field(&result, "message").and_then(ToolValue::as_str),
            Some("Protocol retry budget exhausted")
        );
    }
    #[test]
    fn protocol_retries_are_categorized_and_bounded() {
        let run = execute(
            vec![
                "not json".into(),
                discover(),
                json!({"type":"complete","content":"unsupported"}).to_string(),
            ],
            2,
        );
        assert_eq!(run.outcome, EditOutcome::UnsupportedRequest);
        assert_eq!(run.model_invocations, 3);
        assert!(run.trace.iter().any(|event| matches!(event,
            EditEvent::ProtocolRetry { category, attempt: 1 } if !category.is_empty()
        )));
    }
    #[test]
    fn deterministic_bounded_pages() {
        let names = vec!["z".into(), "a".into(), "a".into(), "m".into()];
        let mut visible = vec![];
        assert_eq!(reveal_operations(&names, &mut visible, 2), 2);
        assert_eq!(visible, vec!["a", "m"]);
        assert_eq!(reveal_operations(&names, &mut visible, 2), 1);
        assert_eq!(visible, vec!["a", "m", "z"]);
        assert_eq!(reveal_operations(&names, &mut visible, 2), 0);
    }
    #[test]
    fn invisible_calls_never_reach_session() {
        let run = execute(
            vec![json!({"type":"tool","name":"language.test.replace","arguments":{}}).to_string()],
            3,
        );
        assert_eq!(run.outcome, EditOutcome::AuthorizationFailure);
        assert_eq!(run.proposal_attempts, 0);
    }
    #[test]
    fn discovery_exhaustion_is_no_progress_not_dispatch() {
        let run = execute(vec![discover(), discover()], 3);
        assert_eq!(run.outcome, EditOutcome::NoProgress);
        assert_eq!(run.model_invocations, 2);
        assert_eq!(run.proposal_attempts, 0);
    }
    #[test]
    fn compact_metadata_is_sorted_and_not_duplicated() {
        struct MetadataProvider(Vec<ToolDefinition>);
        impl ToolProvider for MetadataProvider {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }
            fn metadata(&self, _: &str) -> Option<ToolValue> {
                Some(ToolValue::object([
                    ("id", "replace".into()),
                    ("description", "Replace source".into()),
                    ("effect", "ProposeChanges".into()),
                    ("target", "Document".into()),
                    ("arguments", ToolValue::Array(vec![])),
                ]))
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                panic!("metadata must not invoke")
            }
        }
        let session = AgentSession::builder()
            .provider(MetadataProvider(vec![
                ToolDefinition::new("language.test.z", "duplicated schema"),
                ToolDefinition::new("language.test.a", "duplicated schema"),
                ToolDefinition::new("excluded", "hidden"),
            ]))
            .build()
            .unwrap();
        let compact = compact_catalog(
            &session.catalog(),
            &["language.test.z".into(), "language.test.a".into()],
            false,
        );
        assert_eq!(compact["tools"][0]["name"], "language.test.a");
        assert_eq!(compact["tools"][1]["name"], "language.test.z");
        assert_eq!(compact["tools"].as_array().unwrap().len(), 2);
        assert_eq!(compact["tools"][0]["operation"]["id"], "replace");
        assert!(!compact.to_string().contains("duplicated schema"));
    }
    #[test]
    fn discovery_consumes_model_budget() {
        let mut session = AgentSession::builder()
            .provider(Provider(vec![
                ToolDefinition::new("language.test.replace", "replace").effect(ToolEffect::Write),
            ]))
            .build()
            .unwrap();
        let budgets = EditBudgets {
            model_invocations: 1,
            ..Default::default()
        };
        let run = run_edit_session_inventory(
            &mut Model(vec![discover()]),
            &mut session,
            Ok(SourceId::Scratch(1)),
            "replace",
            &EditCapabilities {
                propose: vec!["language.test.replace".into()],
                ..Default::default()
            },
            budgets,
            EditDispatch {
                policy: &mut |_, _| panic!("discovery is not dispatch"),
                relevant_state: &mut |_| ToolValue::Null,
            },
            Some(&SemanticDiscoveryOptions {
                initial_operations: vec![],
                page_size: 1,
            }),
        );
        assert_eq!(run.outcome, EditOutcome::OperationBudgetExhausted);
        assert_eq!(run.model_invocations, 1);
    }
    #[test]
    fn discovery_consumes_transport_budget() {
        let run = execute(vec![discover()], 0);
        assert_eq!(run.outcome, EditOutcome::OperationBudgetExhausted);
    }
}

#[cfg(test)]
#[path = "intent_tests.rs"]
mod intent_tests;

#[cfg(test)]
mod semantic_inventory_tests {
    mod investigation_tests {
        include!("investigation_tests.rs");
    }
    use super::*;
    use crate::source_edit::SourceId;
    use scope_language::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Target;
    impl SourceTarget for Target {
        fn source_id(&self) -> SourceId {
            SourceId::Scratch(42)
        }
        fn text(&self) -> &str {
            "old"
        }
        fn version(&self) -> u64 {
            7
        }
        fn language_id(&self) -> &str {
            "mock"
        }
        fn apply_source_edit(
            &mut self,
            _: crate::text::TextEdit,
        ) -> Result<(), crate::proposal::ProposalError> {
            panic!("lifecycle must not apply")
        }
    }
    struct Service {
        language: &'static str,
        dispatches: AtomicUsize,
    }
    impl LanguageService for Service {
        fn language_id(&self) -> &'static str {
            self.language
        }
        fn language_name(&self) -> &'static str {
            "Mock"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            true
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            Default::default()
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            [
                (
                    "replace",
                    SemanticOperationEffect::ProposeChanges,
                    SemanticTargetRequirement::Document,
                ),
                (
                    "selected",
                    SemanticOperationEffect::ProposeChanges,
                    SemanticTargetRequirement::Selection,
                ),
                (
                    "ungranted",
                    SemanticOperationEffect::ProposeChanges,
                    SemanticTargetRequirement::Document,
                ),
                (
                    "inspect",
                    SemanticOperationEffect::Read,
                    SemanticTargetRequirement::Document,
                ),
            ]
            .into_iter()
            .map(|(id, effect, target)| SemanticOperationDescription {
                id: id.into(),
                description: format!("Mock {id}"),
                effect,
                target,
                arguments: vec![SemanticArgument {
                    name: "text".into(),
                    kind: SemanticArgumentKind::String,
                    required: true,
                    description: "Replacement or query".into(),
                }],
                constraints: Some(json!({"opaque_large_payload":"x".repeat(10_000)})),
            })
            .collect()
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            self.dispatches.fetch_add(1, Ordering::SeqCst);
            if request.operation_id == "inspect" {
                return Ok(SemanticOperationResult::ReadOnly {
                    value: json!("old"),
                    rationale: "inspection".into(),
                });
            }
            Ok(SemanticOperationResult::Changes {
                edits: vec![SemanticEdit {
                    range: crate::text::TextRange::new(
                        crate::text::Position::new(0, 0),
                        crate::text::Position::new(0, 3),
                    ),
                    replacement: request.arguments["text"].as_str().unwrap().into(),
                }],
                rationale: "replacement".into(),
            })
        }
    }
    #[derive(Default)]
    struct Model {
        decisions: std::collections::VecDeque<String>,
        contexts: Vec<serde_json::Value>,
    }
    impl DecisionModel for Model {
        fn decide(
            &mut self,
            messages: &[crate::agent::DecisionMessage],
            _: u32,
        ) -> Result<String, String> {
            let context = serde_json::from_str(&messages.last().unwrap().content).unwrap();
            self.contexts.push(context);
            Ok(self.decisions.pop_front().expect("unexpected decision"))
        }
    }
    fn tool(name: &str) -> String {
        json!({"type":"tool","name":name,"arguments":{"text":"new"}}).to_string()
    }
    fn discover() -> String {
        json!({"type":"tool","name":DISCOVER_SEMANTIC_OPERATIONS,"arguments":{}}).to_string()
    }
    fn complete() -> String {
        json!({"type":"complete","content":"done"}).to_string()
    }
    fn options() -> SemanticEditOptions {
        SemanticEditOptions {
            capabilities: EditCapabilities {
                inspect: vec!["language.mock.inspect".into()],
                propose: vec![
                    "language.mock.replace".into(),
                    "language.mock.selected".into(),
                    "language.other.replace".into(),
                ],
                inspect_first: false,
            },
            ..Default::default()
        }
    }
    struct ResultSet {
        run: EditRun,
        contexts: Vec<serde_json::Value>,
        dispatches: usize,
        policies: usize,
    }
    fn execute(
        language: &'static str,
        decisions: Vec<String>,
        options: SemanticEditOptions,
        selection: bool,
        initial: Vec<String>,
        authorize: bool,
    ) -> ResultSet {
        let target = Target;
        let service = Service {
            language,
            dispatches: AtomicUsize::new(0),
        };
        let policies = Cell::new(0);
        let mut controller = SourceEditController::default();
        let mut model = Model {
            decisions: decisions.into(),
            ..Default::default()
        };
        let mut request = AgentEditRequest::new(&target, "replace source");
        if selection {
            request.selection = Some(crate::text::TextRange::new(
                crate::text::Position::new(0, 0),
                crate::text::Position::new(0, 3),
            ));
        }
        let run = run_semantic_edit_with_discovery(
            &mut model,
            &mut controller,
            &target,
            &service,
            request,
            &options,
            &SemanticDiscoveryOptions {
                initial_operations: initial,
                page_size: 8,
            },
            |_| {
                policies.set(policies.get() + 1);
                authorize
            },
        );
        if let Some(id) = run.proposal {
            assert_eq!(controller.preview(id, &target).unwrap(), "new");
            assert_eq!(
                controller.get(id).unwrap().status(),
                crate::proposal::ProposalStatus::Pending
            );
        }
        assert_eq!(target.text(), "old");
        ResultSet {
            run,
            contexts: model.contexts,
            dispatches: service.dispatches.load(Ordering::SeqCst),
            policies: policies.get(),
        }
    }
    fn names(context: &serde_json::Value) -> Vec<&str> {
        context["catalog"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect()
    }
    #[test]
    fn discovery_description_reaches_model_and_empty_arguments_reveal_catalog() {
        let result = execute(
            "mock",
            vec![discover(), complete()],
            options(),
            false,
            vec![],
            true,
        );
        let descriptor = &result.contexts[0]["discovery"];
        assert_eq!(descriptor["name"], DISCOVER_SEMANTIC_OPERATIONS);
        assert_eq!(descriptor["arguments"], json!({}));
        assert_eq!(descriptor["page_size"], 8);
        let description = descriptor["description"].as_str().unwrap();
        assert!(
            description.contains("Accepts exactly an empty JSON object: arguments must be {}.")
        );
        assert!(description.contains("page_size is host-owned metadata, not an accepted argument"));
        assert!(description.contains("do not supply page_size inside arguments"));
        assert!(description.contains("No other argument keys are accepted."));
        assert!(
            result
                .run
                .trace
                .iter()
                .any(|event| matches!(event, EditEvent::Discovery { revealed: 1 }))
        );
        assert!(names(&result.contexts[0]).is_empty());
        assert_eq!(names(&result.contexts[1]), vec!["language.mock.replace"]);
        assert_eq!((result.dispatches, result.policies), (0, 0));
    }

    #[test]
    fn discovery_rejects_host_metadata_and_other_argument_keys() {
        for arguments in [json!({"page_size": 2}), json!({"unexpected": true})] {
            let result = execute(
                "mock",
                vec![
                    json!({"type":"tool", "name":DISCOVER_SEMANTIC_OPERATIONS,
                    "arguments":arguments})
                    .to_string(),
                ],
                options(),
                false,
                vec![],
                true,
            );
            assert_eq!(result.run.outcome, EditOutcome::NoProgress);
            assert!(result.run.trace.iter().any(|event| matches!(
                event,
                EditEvent::NoProgressReason { reason } if reason == "invalid_discovery_arguments"
            )));
            assert!(
                !result
                    .run
                    .trace
                    .iter()
                    .any(|event| matches!(event, EditEvent::Discovery { .. }))
            );
            assert_eq!(result.contexts.len(), 1);
            assert_eq!((result.dispatches, result.policies), (0, 0));
        }
    }

    #[test]
    fn language_mismatch_stops_before_model_or_policy() {
        let result = execute("other", vec![], options(), false, vec![], true);
        assert_eq!(result.run.outcome, EditOutcome::AuthorizationFailure);
        assert!(result.contexts.is_empty());
        assert_eq!((result.dispatches, result.policies), (0, 0));
    }
    #[test]
    fn discovery_excludes_unrelated_language_and_missing_selection() {
        let result = execute(
            "mock",
            vec![discover(), complete()],
            options(),
            false,
            vec![],
            true,
        );
        assert!(names(&result.contexts[0]).is_empty());
        assert_eq!(names(&result.contexts[1]), vec!["language.mock.replace"]);
        assert_eq!((result.dispatches, result.policies), (0, 0));
        let selected = execute(
            "mock",
            vec![discover(), complete()],
            options(),
            true,
            vec![],
            true,
        );
        assert_eq!(
            names(&selected.contexts[1]),
            vec!["language.mock.replace", "language.mock.selected"]
        );
    }
    #[test]
    fn selection_excluded_call_is_not_dispatched() {
        let result = execute(
            "mock",
            vec![discover(), tool("language.mock.selected")],
            options(),
            false,
            vec![],
            true,
        );
        assert_eq!(result.run.outcome, EditOutcome::AuthorizationFailure);
        assert_eq!((result.dispatches, result.policies), (0, 0));
    }
    #[test]
    fn discovery_then_proposal_has_complete_compact_invocation_contract() {
        let result = execute(
            "mock",
            vec![discover(), tool("language.mock.replace")],
            options(),
            false,
            vec![],
            true,
        );
        assert_eq!(result.run.outcome, EditOutcome::ProposalReady);
        assert_eq!((result.dispatches, result.policies), (1, 2));
        let advertised = &result.contexts[1]["catalog"]["tools"][0];
        assert_eq!(advertised["inputs"].as_array().unwrap().len(), 1);
        assert_eq!(advertised["inputs"][0]["name"], "text");
        assert_eq!(advertised["operation"]["arguments"][0]["name"], "text");
        assert_eq!(advertised["operation"]["arguments"][0]["required"], true);
        assert_eq!(
            advertised["invocation_schema"]["properties"]["text"]["type"],
            "string"
        );
        assert_eq!(
            advertised["invocation_schema"]["additionalProperties"],
            false
        );
        assert_eq!(advertised["scope"], "single_document");
        assert!(advertised["preconditions"]["ownership"].is_string());
        assert!(
            advertised["invocation_schema"]["properties"]
                .get("expected_source")
                .is_none()
        );
        assert!(advertised["result"].as_str().unwrap().contains("proposal"));
        let text = advertised.to_string();
        assert!(!text.contains("constraints"));
        assert!(!text.contains("opaque_large_payload"));
        assert!(text.len() < 2000);
    }
    #[test]
    fn discovery_does_not_grant_host_authorization() {
        let result = execute(
            "mock",
            vec![discover(), tool("language.mock.replace")],
            options(),
            false,
            vec![],
            false,
        );
        assert_eq!(result.run.outcome, EditOutcome::AuthorizationFailure);
        assert!(result.run.proposal.is_none());
        assert_eq!((result.dispatches, result.policies), (0, 1));
    }
    #[test]
    fn registered_operation_absent_capabilities_remains_unavailable() {
        let result = execute(
            "mock",
            vec![discover(), tool("language.mock.ungranted")],
            options(),
            false,
            vec![],
            true,
        );
        assert_eq!(names(&result.contexts[1]), vec!["language.mock.replace"]);
        assert_eq!(result.run.outcome, EditOutcome::AuthorizationFailure);
        assert_eq!((result.dispatches, result.policies), (0, 0));
    }
    #[test]
    fn inspect_phase_excludes_edits_even_when_initially_visible() {
        let mut options = options();
        options.capabilities.inspect_first = true;
        let result = execute(
            "mock",
            vec![discover(), tool("language.mock.replace")],
            options,
            false,
            vec!["language.mock.replace".into()],
            true,
        );
        assert!(names(&result.contexts[0]).is_empty());
        assert_eq!(names(&result.contexts[1]), vec!["language.mock.inspect"]);
        assert_eq!(result.run.outcome, EditOutcome::AuthorizationFailure);
        assert_eq!((result.dispatches, result.policies), (0, 0));
    }
    struct ValidationService {
        inner: Service,
        validations: AtomicUsize,
    }
    impl LanguageService for ValidationService {
        fn language_id(&self) -> &'static str {
            self.inner.language_id()
        }
        fn language_name(&self) -> &'static str {
            self.inner.language_name()
        }
        fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
            self.inner.matches_path(path)
        }
        fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
            self.inner.analyze(document)
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            self.inner.semantic_operations()
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            if request.arguments["text"] == "dependency" {
                self.inner.dispatches.fetch_add(1, Ordering::SeqCst);
                return Ok(SemanticOperationResult::Blocked {
                    blocker: SemanticBlocker {
                        operation_id: request.operation_id.into(),
                        category: BlockerCategory::MissingDependency,
                        explanation: "required library is unavailable".into(),
                        recoverable: false,
                        missing_dependencies: vec!["library".into()],
                        diagnostics: vec![],
                        suggested_next_action: Some("provide library context".into()),
                    },
                });
            }
            let limited = request.arguments["text"] == "limited";
            let result = self.inner.perform_semantic_operation(request)?;
            if limited {
                let SemanticOperationResult::Changes { edits, rationale } = result else {
                    panic!("expected changes")
                };
                return Ok(SemanticOperationResult::ValidatedChanges {
                    edits,
                    rationale,
                    validation: CandidateValidation {
                        status: ValidationStatus::AcceptedWithLimitations,
                        limitations: vec!["external symbols were not checked".into()],
                        missing_dependencies: vec!["optional-library".into()],
                        ..Default::default()
                    },
                });
            }
            Ok(result)
        }
        fn validate_semantic_candidate(
            &self,
            request: SemanticValidationRequest<'_>,
        ) -> Result<CandidateValidation, SemanticOperationError> {
            self.validations.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.operation_id, "replace");
            assert_eq!(request.document.text(), "old");
            assert_eq!(request.document.version(), 7);
            assert_eq!(request.edits.len(), 1);
            assert_eq!(
                request.candidate_source,
                request.arguments["text"].as_str().unwrap()
            );
            Ok(CandidateValidation {
                status: if request.candidate_source.starts_with("bad") {
                    ValidationStatus::Rejected
                } else {
                    ValidationStatus::Accepted
                },
                ..Default::default()
            })
        }
    }
    fn validated_run(texts: &[&str]) -> (EditRun, SourceEditController, usize, usize, usize) {
        let target = Target;
        let service = ValidationService {
            inner: Service {
                language: "mock",
                dispatches: AtomicUsize::new(0),
            },
            validations: AtomicUsize::new(0),
        };
        let mut model = Model {
            decisions: texts
                .iter()
                .map(|text| {
                    json!({"type":"tool","name":"language.mock.replace","arguments":{"text":text}})
                        .to_string()
                })
                .collect(),
            ..Default::default()
        };
        let mut controller = SourceEditController::default();
        let policies = Cell::new(0);
        let run = run_semantic_edit(
            &mut model,
            &mut controller,
            &target,
            &service,
            AgentEditRequest::new(&target, "replace source"),
            &options(),
            |_| {
                policies.set(policies.get() + 1);
                true
            },
        );
        assert_eq!(target.text(), "old");
        assert_eq!(target.version(), 7);
        (
            run,
            controller,
            service.inner.dispatches.load(Ordering::SeqCst),
            service.validations.load(Ordering::SeqCst),
            policies.get(),
        )
    }
    #[test]
    fn real_semantic_edit_accepts_hook_validated_candidate() {
        let (run, controller, dispatches, validations, policies) = validated_run(&["new"]);
        assert_eq!(run.outcome, EditOutcome::ProposalReady);
        assert_eq!((dispatches, validations, policies), (1, 1, 2));
        let id = run.proposal.unwrap();
        assert_eq!(controller.preview(id, &Target).unwrap(), "new");
        let proposal = controller.get(id).unwrap();
        assert_eq!(proposal.status(), crate::proposal::ProposalStatus::Pending);
        assert_eq!(proposal.validation().status, ValidationStatus::Accepted);
        assert_eq!(
            field(run.last_result.as_ref().unwrap(), "validation_status")
                .and_then(ToolValue::as_str),
            Some("Accepted")
        );
    }
    #[test]
    fn real_semantic_edit_retains_limited_validation_metadata() {
        let (run, controller, dispatches, validations, policies) = validated_run(&["limited"]);
        assert_eq!(run.outcome, EditOutcome::ProposalReady);
        assert_eq!((dispatches, validations, policies), (1, 0, 2));
        let validation = controller.get(run.proposal.unwrap()).unwrap().validation();
        assert_eq!(validation.status, ValidationStatus::AcceptedWithLimitations);
        assert_eq!(
            validation.limitations,
            vec!["external symbols were not checked"]
        );
        assert_eq!(validation.missing_dependencies, vec!["optional-library"]);
        assert!(run.trace.contains(&EditEvent::ValidationStatus {
            status: "AcceptedWithLimitations".into(),
            diagnostic_count: 0,
        }));
    }
    #[test]
    fn real_semantic_edit_corrects_rejection_without_diagnostics() {
        let (run, controller, dispatches, validations, policies) = validated_run(&["bad", "new"]);
        assert_eq!(run.outcome, EditOutcome::ProposalReady);
        assert_eq!((dispatches, validations, policies), (2, 2, 3));
        assert_eq!((run.proposal_attempts, run.corrections), (2, 1));
        assert_eq!(run.proposal, Some(ProposalId(1)));
        assert_eq!(
            controller.get(ProposalId(1)).unwrap().validation().status,
            ValidationStatus::Accepted
        );
        assert!(controller.get(ProposalId(2)).is_err());
        assert!(run.trace.contains(&EditEvent::ValidationStatus {
            status: "Rejected".into(),
            diagnostic_count: 0,
        }));
        assert!(
            run.trace
                .contains(&EditEvent::RevisedProposalProduced(ProposalId(1)))
        );
    }
    #[test]
    fn real_semantic_edit_exhausts_corrections_without_registration() {
        let (run, controller, dispatches, validations, policies) =
            validated_run(&["bad", "bad-again"]);
        assert_eq!(run.outcome, EditOutcome::ValidationFailure);
        assert_eq!((dispatches, validations, policies), (2, 2, 2));
        assert_eq!((run.proposal_attempts, run.corrections), (2, 1));
        assert!(run.proposal.is_none());
        assert!(controller.get(ProposalId(1)).is_err());
        let evidence = run.last_result.as_ref().unwrap();
        assert_eq!(
            field(evidence, "status").and_then(ToolValue::as_str),
            Some("rejected")
        );
        assert_eq!(
            field(evidence, "correction_eligible"),
            Some(&ToolValue::Boolean(true))
        );
    }
    #[test]
    fn real_semantic_edit_stops_at_terminal_dependency_blocker() {
        let (run, controller, dispatches, validations, policies) =
            validated_run(&["dependency", "new"]);
        assert_eq!(run.outcome, EditOutcome::SemanticBlocked);
        assert_eq!((dispatches, validations, policies), (1, 0, 1));
        assert_eq!((run.model_invocations, run.corrections), (1, 0));
        assert!(run.proposal.is_none());
        assert!(controller.get(ProposalId(1)).is_err());
        let evidence = run.last_result.as_ref().unwrap();
        assert_eq!(
            field(evidence, "missing_dependencies"),
            Some(&ToolValue::array(["library".into()]))
        );
        assert!(run.trace.contains(&EditEvent::SemanticBlocker {
            category: "MissingDependency".into(),
            recoverable: false,
        }));
    }
    #[test]
    fn identical_context_catalogs_are_stable() {
        let first = execute(
            "mock",
            vec![discover(), complete()],
            options(),
            true,
            vec![],
            true,
        );
        let mut reordered = options();
        reordered.capabilities.propose.reverse();
        let second = execute(
            "mock",
            vec![discover(), complete()],
            reordered,
            true,
            vec![],
            true,
        );
        assert_eq!(first.contexts, second.contexts);
    }
}

#[cfg(test)]
#[path = "edit_lifecycle_tests.rs"]
mod tests;
