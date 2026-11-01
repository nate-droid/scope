//! Typed, sanitized lifecycle. Imported turns are records, never execution authority.
use super::decision::{AgentLimits, AgentStop};
use crate::tool::verification::{
    VerificationClassification, VerificationCompleteness, VerificationMetadata,
};
use crate::tool::{ToolError, ToolValue};

pub(super) fn valid_verification(
    metadata: &VerificationMetadata,
    name: &str,
    source: Option<&AgentSourceBinding>,
) -> bool {
    let label = |s: &str| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control);
    metadata.operation == name
        && source == Some(&metadata.source)
        && label(&metadata.operation)
        && label(&metadata.authority)
        && label(&metadata.source.language)
        && (metadata.completeness != VerificationCompleteness::Unavailable
            || metadata.classification == VerificationClassification::Inconclusive)
        && serde_json::to_vec(metadata).is_ok_and(|bytes| bytes.len() <= 2048)
}
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "identity", rename_all = "snake_case")]
pub enum AgentSourceIdentity {
    ScratchBuffer(String),
    File(String),
    Host(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSourceBinding {
    pub identity: AgentSourceIdentity,
    pub revision: u64,
    pub language: String,
}

/// Sanitized failure classification: never retains backend/configuration text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentFailure {
    Model,
    Configuration,
    ToolSetup,
    Application,
    Stopped(AgentStop),
}
impl AgentFailure {
    pub(super) fn from_error(error: &super::AgentError) -> Self {
        match error {
            super::AgentError::Chat(_) => Self::Model,
            super::AgentError::ConfigureChat(_) => Self::Configuration,
            super::AgentError::Tool(_) => Self::ToolSetup,
            super::AgentError::App(_) => Self::Application,
            super::AgentError::Stopped(stop) => Self::Stopped(*stop),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentState {
    Ready,
    Deciding {
        request_ordinal: usize,
    },
    ToolRunning {
        request_ordinal: usize,
        call_ordinal: usize,
        name: String,
    },
    Completed,
    BudgetExhausted(AgentStop),
    Cancelled,
    Failed(AgentFailure),
}
impl AgentState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::BudgetExhausted(_) | Self::Cancelled | Self::Failed(_)
        )
    }
    fn attribution(&self) -> (Option<usize>, Option<usize>) {
        match self {
            Self::Deciding { request_ordinal } => (Some(*request_ordinal), None),
            Self::ToolRunning {
                request_ordinal,
                call_ordinal,
                ..
            } => (Some(*request_ordinal), Some(*call_ordinal)),
            _ => (None, None),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentTrigger {
    ModelRequested,
    ToolRequested,
    Completed,
    BudgetExhausted(AgentStop),
    Cancelled,
    Failed,
}
impl AgentTrigger {
    fn destination(state: &AgentState) -> Option<Self> {
        Some(match state {
            AgentState::Ready => return None,
            AgentState::Deciding { .. } => Self::ModelRequested,
            AgentState::ToolRunning { .. } => Self::ToolRequested,
            AgentState::Completed => Self::Completed,
            AgentState::BudgetExhausted(stop) => Self::BudgetExhausted(*stop),
            AgentState::Cancelled => Self::Cancelled,
            AgentState::Failed(_) => Self::Failed,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTransition {
    pub turn_id: u64,
    pub ordinal: usize,
    pub request_ordinal: Option<usize>,
    pub call_ordinal: Option<usize>,
    pub trigger: AgentTrigger,
    pub from: AgentState,
    pub to: AgentState,
}
/// Provider execution category. `Success` means `Ok`, not domain acceptance
/// or a grant of authority; structured domain rejections remain native output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentToolOutcome {
    Success,
    UnknownTool,
    DuplicateTool,
    MissingArgument,
    InvalidArgument,
    Conflict,
    Denied,
    Execution,
}
impl AgentToolOutcome {
    pub(super) fn from_result(result: &Result<crate::tool::ToolResult, ToolError>) -> Self {
        match result {
            Ok(_) => Self::Success,
            Err(ToolError::UnknownTool { .. }) => Self::UnknownTool,
            Err(ToolError::DuplicateTool { .. }) => Self::DuplicateTool,
            Err(ToolError::MissingArgument { .. }) => Self::MissingArgument,
            Err(ToolError::InvalidArgument { .. }) => Self::InvalidArgument,
            Err(ToolError::Conflict { .. }) => Self::Conflict,
            Err(ToolError::Denied { .. }) => Self::Denied,
            Err(ToolError::Execution { .. }) => Self::Execution,
        }
    }
}
/// A text response returned by the model adapter, not protocol acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentModelResponseOutcome {
    Returned,
    Error(AgentFailure),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentEvidence {
    /// Assembly and source-binding validation only, not delivery to the model.
    /// A successfully assembled packet can still be rejected by message budgets.
    /// See ContextSupplied for exact bytes handed to the first model call.
    ContextAssembly {
        success: bool,
        /// Serialized packet bytes assembled, not bytes supplied to the model or
        /// charged to conversation usage. Zero when assembly itself failed.
        bytes: usize,
        bounded: bool,
        failure: Option<String>,
    },
    /// Exact UTF-8 JSON bytes handed to the first model call, not tokens or
    /// transport framing. Recorded once at the call boundary, without payloads.
    ContextSupplied {
        packet_bytes: usize,
        initial_message_bytes: usize,
    },
    Decision {
        ordinal: usize,
    },
    ModelResponse {
        ordinal: usize,
        outcome: AgentModelResponseOutcome,
        /// UTF-8 bytes of returned text, not tokens or backend error text length.
        response_bytes: Option<usize>,
    },
    Malformed {
        ordinal: usize,
    },
    Verification {
        request_ordinal: usize,
        call_ordinal: usize,
        observation: VerificationMetadata,
    },
    Tool {
        request_ordinal: usize,
        ordinal: usize,
        name: String,
        outcome: AgentToolOutcome,
        /// Trusted in-process output. Never serialized or loaded from retained JSON.
        #[serde(skip, default)]
        output: Option<ToolValue>,
    },
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentBudgetUsage {
    pub decisions: usize,
    pub tool_calls: usize,
    pub malformed: usize,
    /// Accepted conversation message bytes plus charged workflow input/result bytes.
    /// Not transport bytes, tokens, or attempted/rejected oversized messages.
    pub conversation_bytes: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLifecycleError {
    Terminal,
    DuplicateTransition,
    InvalidTransition,
    InvalidEvidence,
    InvalidSnapshot,
}
impl std::fmt::Display for AgentLifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid agent lifecycle: {self:?}")
    }
}
impl std::error::Error for AgentLifecycleError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RetainedAgentTurn")]
pub struct AgentTurn {
    id: u64,
    model_identity: Option<String>,
    provider_identity: Option<String>,
    source_binding: Option<AgentSourceBinding>,
    limits: AgentLimits,
    state: AgentState,
    transitions: Vec<AgentTransition>,
    evidence: Vec<AgentEvidence>,
    usage: AgentBudgetUsage,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedAgentTurn {
    id: u64,
    model_identity: Option<String>,
    provider_identity: Option<String>,
    source_binding: Option<AgentSourceBinding>,
    limits: AgentLimits,
    state: AgentState,
    transitions: Vec<AgentTransition>,
    evidence: Vec<AgentEvidence>,
    usage: AgentBudgetUsage,
}
impl TryFrom<RetainedAgentTurn> for AgentTurn {
    type Error = AgentLifecycleError;
    fn try_from(raw: RetainedAgentTurn) -> Result<Self, Self::Error> {
        let mut turn = Self {
            id: raw.id,
            model_identity: raw.model_identity,
            provider_identity: raw.provider_identity,
            source_binding: raw.source_binding,
            limits: raw.limits,
            state: AgentState::Ready,
            transitions: vec![],
            evidence: vec![],
            usage: AgentBudgetUsage::default(),
        };
        if turn.id == 0 {
            return Err(AgentLifecycleError::InvalidSnapshot);
        }
        for transition in raw.transitions {
            turn.transition(transition.to.clone())?;
            if turn.transitions.last() != Some(&transition) {
                return Err(AgentLifecycleError::InvalidSnapshot);
            }
        }
        if turn.state != raw.state {
            return Err(AgentLifecycleError::InvalidSnapshot);
        }
        // Evidence is validated against its attributed start boundary. Terminal
        // history can be loaded, but no live runner ever accepts an imported turn.
        for evidence in raw.evidence {
            turn.validate_evidence(&evidence)?;
            turn.account_evidence(&evidence);
            turn.evidence.push(evidence);
        }
        if turn.usage.decisions != raw.usage.decisions
            || turn.usage.tool_calls != raw.usage.tool_calls
            || turn.usage.malformed != raw.usage.malformed
            || raw.usage.conversation_bytes > turn.limits.max_conversation_bytes
            || turn.usage.decisions > turn.limits.max_decisions
            || turn.usage.tool_calls > turn.limits.max_tool_calls
            || turn.usage.malformed > turn.limits.max_malformed.saturating_add(1)
        {
            return Err(AgentLifecycleError::InvalidSnapshot);
        }
        for transition in &turn.transitions {
            match transition.to {
                AgentState::Deciding { request_ordinal } => {
                    if request_ordinal > turn.limits.max_decisions
                        || request_ordinal.saturating_sub(1) > turn.usage.decisions
                    {
                        return Err(AgentLifecycleError::InvalidSnapshot);
                    }
                }
                AgentState::ToolRunning {
                    request_ordinal, ..
                } => {
                    if request_ordinal > turn.usage.decisions || !turn.has_returned_response(request_ordinal) {
                        return Err(AgentLifecycleError::InvalidSnapshot);
                    }
                }
                AgentState::Completed => {
                    if transition.request_ordinal != Some(turn.usage.decisions)
                        || turn.usage.decisions == 0
                        || !turn.has_returned_response(turn.usage.decisions)
                    {
                        return Err(AgentLifecycleError::InvalidSnapshot);
                    }
                }
                AgentState::Failed(AgentFailure::Model) => {
                    if !turn.evidence.iter().any(|e| matches!(e, AgentEvidence::ModelResponse { ordinal, outcome: AgentModelResponseOutcome::Error(AgentFailure::Model), .. } if Some(*ordinal) == transition.request_ordinal)) {
                        return Err(AgentLifecycleError::InvalidSnapshot);
                    }
                }
                _ => {}
            }
        }
        if turn
            .evidence
            .iter()
            .any(|e| matches!(e, AgentEvidence::ContextAssembly { success: false, .. }))
            && (!matches!(turn.state, AgentState::Failed(AgentFailure::ToolSetup))
                || turn.transitions.len() != 1
                || turn.evidence.len() != 1
                || raw.usage != AgentBudgetUsage::default())
        {
            return Err(AgentLifecycleError::InvalidSnapshot);
        }
        if turn.evidence.iter().any(|e| {
            matches!(e,
            AgentEvidence::ContextSupplied { initial_message_bytes, .. }
                if *initial_message_bytes > raw.usage.conversation_bytes)
        }) {
            return Err(AgentLifecycleError::InvalidSnapshot);
        }
        turn.usage.conversation_bytes = raw.usage.conversation_bytes;
        Ok(turn)
    }
}
impl AgentTurn {
    pub(crate) fn new(
        model_identity: Option<String>,
        source_binding: Option<AgentSourceBinding>,
        limits: AgentLimits,
    ) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("turn id space exhausted");
        Self {
            id,
            model_identity,
            provider_identity: None,
            source_binding,
            limits,
            state: AgentState::Ready,
            transitions: vec![],
            evidence: vec![],
            usage: AgentBudgetUsage::default(),
        }
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn model_identity(&self) -> Option<&str> {
        self.model_identity.as_deref()
    }
    pub fn provider_identity(&self) -> Option<&str> {
        self.provider_identity.as_deref()
    }
    pub(super) fn bind_provider_identity(
        &mut self,
        identity: Option<String>,
    ) -> Result<(), AgentLifecycleError> {
        if self.state.is_terminal() {
            return Err(AgentLifecycleError::Terminal);
        }
        if self.state != AgentState::Ready {
            return Err(AgentLifecycleError::InvalidTransition);
        }
        self.provider_identity = identity;
        Ok(())
    }
    pub fn source_binding(&self) -> Option<&AgentSourceBinding> {
        self.source_binding.as_ref()
    }
    pub fn limits(&self) -> AgentLimits {
        self.limits
    }
    pub fn state(&self) -> &AgentState {
        &self.state
    }
    pub fn transitions(&self) -> &[AgentTransition] {
        &self.transitions
    }
    pub fn usage(&self) -> AgentBudgetUsage {
        self.usage
    }
    /// Trusted in-process evidence; output may be sensitive. Serialization omits it.
    pub fn evidence(&self) -> &[AgentEvidence] {
        &self.evidence
    }
    pub(super) fn transition(&mut self, to: AgentState) -> Result<(), AgentLifecycleError> {
        use AgentState::*;
        if self.state.is_terminal() {
            return Err(AgentLifecycleError::Terminal);
        }
        if self.state == to {
            return Err(AgentLifecycleError::DuplicateTransition);
        }
        let next_request = self.state.attribution().0.unwrap_or(0).checked_add(1);
        let valid = match &to {
            Ready => false,
            Deciding { request_ordinal } => Some(*request_ordinal) == next_request,
            ToolRunning {
                request_ordinal,
                call_ordinal,
                ..
            } => {
                self.state.attribution().0 == Some(*request_ordinal)
                    && self.usage.tool_calls.checked_add(1) == Some(*call_ordinal)
            }
            Completed => matches!(self.state, Deciding { .. }),
            BudgetExhausted(stop) => !matches!(stop, AgentStop::Completed | AgentStop::Cancelled),
            Cancelled | Failed(_) => true,
        };
        if !valid {
            return Err(AgentLifecycleError::InvalidTransition);
        }
        let (request_ordinal, call_ordinal) = if to.is_terminal() {
            self.state.attribution()
        } else {
            to.attribution()
        };
        self.transitions.push(AgentTransition {
            turn_id: self.id,
            ordinal: self.transitions.len() + 1,
            request_ordinal,
            call_ordinal,
            trigger: AgentTrigger::destination(&to)
                .ok_or(AgentLifecycleError::InvalidTransition)?,
            from: self.state.clone(),
            to: to.clone(),
        });
        if let ToolRunning { call_ordinal, .. } = &to {
            self.usage.tool_calls = *call_ordinal;
        }
        self.state = to;
        Ok(())
    }
    fn has_returned_response(&self, ordinal: usize) -> bool {
        self.evidence.iter().any(|e| matches!(e, AgentEvidence::ModelResponse { ordinal: n, outcome: AgentModelResponseOutcome::Returned, .. } if *n == ordinal))
    }
    fn validate_evidence(&self, evidence: &AgentEvidence) -> Result<(), AgentLifecycleError> {
        let valid =
            match evidence {
                AgentEvidence::ContextAssembly { success, bytes, bounded, failure } => {
                    self.evidence.is_empty()
                        && self.source_binding.is_some()
                        && if *success {
                            failure.is_none()
                        } else {
                            matches!(failure.as_deref(), Some("semantic context binding mismatch"))
                                || (failure.as_deref() == Some("semantic context assembly failed")
                                    && *bytes == 0 && !*bounded)
                        }
                }
                AgentEvidence::ContextSupplied { packet_bytes, initial_message_bytes } => {
                    self.usage.decisions == 1
                        && matches!(self.evidence.last(), Some(AgentEvidence::Decision { ordinal: 1 }))
                        && self.transitions.iter().any(|t| t.to == AgentState::Deciding { request_ordinal: 1 })
                        && matches!(self.evidence.first(), Some(AgentEvidence::ContextAssembly { success: true, bytes, .. }) if bytes == packet_bytes)
                        && *packet_bytes > 0
                        && *initial_message_bytes > *packet_bytes
                        && *initial_message_bytes <= self.limits.max_message_bytes
                        && *initial_message_bytes <= self.limits.max_conversation_bytes
                        && !self.evidence.iter().any(|e| matches!(e, AgentEvidence::ContextSupplied { .. }))
                }
                AgentEvidence::Decision { ordinal } => {
                    Some(*ordinal) == self.usage.decisions.checked_add(1)
                        && self.transitions.iter().any(|t| {
                            t.to == AgentState::Deciding {
                                request_ordinal: *ordinal,
                            }
                        })
                }
                AgentEvidence::ModelResponse { ordinal, outcome, response_bytes } => {
                    *ordinal == self.usage.decisions && self.usage.decisions > 0
                        && (!self.evidence.iter().any(|e| matches!(e, AgentEvidence::ContextAssembly { .. }))
                            || self.evidence.iter().any(|e| matches!(e, AgentEvidence::ContextSupplied { .. })))
                        && !self.evidence.iter().any(|e| matches!(e, AgentEvidence::ModelResponse { ordinal: n, .. } if n == ordinal))
                        && matches!((outcome, response_bytes),
                            (AgentModelResponseOutcome::Returned, Some(_)) |
                            (AgentModelResponseOutcome::Error(AgentFailure::Model), None))
                }
                AgentEvidence::Malformed { ordinal } => self.has_returned_response(*ordinal) && *ordinal == self.usage.decisions
                    && self.evidence.iter().any(
                        |e| matches!(e, AgentEvidence::Decision { ordinal: n } if n == ordinal),
                    )
                    && !self.evidence.iter().any(
                        |e| matches!(e, AgentEvidence::Malformed { ordinal: n } if n == ordinal),
                    ),
                AgentEvidence::Verification { request_ordinal, call_ordinal, observation } => {
                    *request_ordinal == self.usage.decisions
                        && !self.evidence.iter().take(self.evidence.len().saturating_sub(1)).any(|e| matches!(e,
                            AgentEvidence::Tool { ordinal, .. } if ordinal >= call_ordinal))
                        && valid_verification(observation, &observation.operation, self.source_binding())
                        && matches!(self.evidence.last(), Some(AgentEvidence::Tool {
                            request_ordinal: request, ordinal, name, outcome: AgentToolOutcome::Success, ..
                        }) if request == request_ordinal && ordinal == call_ordinal && name == &observation.operation)
                }
                AgentEvidence::Tool {
                    request_ordinal,
                    ordinal,
                    name,
                    ..
                } => {
                    self.has_returned_response(*request_ordinal) && *request_ordinal == self.usage.decisions
                        && self.usage.decisions > 0
                        && self.transitions.iter().any(|t| {
                            t.to == AgentState::ToolRunning {
                                request_ordinal: *request_ordinal,
                                call_ordinal: *ordinal,
                                name: name.clone(),
                            }
                        })
                        && !self.evidence.iter().any(
                            |e| matches!(e, AgentEvidence::Tool { ordinal: n, .. } if n == ordinal),
                        )
                }
            };
        if valid {
            Ok(())
        } else {
            Err(AgentLifecycleError::InvalidEvidence)
        }
    }
    fn account_evidence(&mut self, evidence: &AgentEvidence) {
        match evidence {
            AgentEvidence::Decision { .. } => self.usage.decisions += 1,
            AgentEvidence::Malformed { .. } => self.usage.malformed += 1,
            AgentEvidence::ContextAssembly { .. }
            | AgentEvidence::ContextSupplied { .. }
            | AgentEvidence::ModelResponse { .. }
            | AgentEvidence::Tool { .. }
            | AgentEvidence::Verification { .. } => {}
        }
    }
    pub(super) fn record(&mut self, evidence: AgentEvidence) -> Result<(), AgentLifecycleError> {
        if self.state.is_terminal() {
            return Err(AgentLifecycleError::Terminal);
        }
        if matches!(evidence, AgentEvidence::ContextSupplied { .. })
            && self.state != (AgentState::Deciding { request_ordinal: 1 })
        {
            return Err(AgentLifecycleError::InvalidEvidence);
        }
        if let AgentEvidence::Verification {
            request_ordinal,
            call_ordinal,
            observation,
        } = &evidence
        {
            if self.state
                != (AgentState::ToolRunning {
                    request_ordinal: *request_ordinal,
                    call_ordinal: *call_ordinal,
                    name: observation.operation.clone(),
                })
            {
                return Err(AgentLifecycleError::InvalidEvidence);
            }
        }
        self.validate_evidence(&evidence)?;
        self.account_evidence(&evidence);
        self.evidence.push(evidence);
        Ok(())
    }
    pub(super) fn charge_bytes(&mut self, bytes: usize) -> Result<(), AgentLifecycleError> {
        if self.state.is_terminal() {
            return Err(AgentLifecycleError::Terminal);
        }
        let total = self
            .usage
            .conversation_bytes
            .checked_add(bytes)
            .ok_or(AgentLifecycleError::InvalidSnapshot)?;
        if total > self.limits.max_conversation_bytes {
            return Err(AgentLifecycleError::InvalidSnapshot);
        }
        self.usage.conversation_bytes = total;
        Ok(())
    }
}
/// Cooperative cancellation only: does not interrupt a blocking model/provider.
#[derive(Debug, Clone, Default)]
pub struct AgentCancellationToken(Arc<AtomicBool>);
impl AgentCancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod verification_tests {
    use super::*;
    use crate::tool::verification::VerificationSubject;

    fn fixture() -> (AgentTurn, AgentEvidence) {
        let source = AgentSourceBinding {
            identity: AgentSourceIdentity::ScratchBuffer("1".into()),
            revision: 1,
            language: "test".into(),
        };
        let mut turn = AgentTurn::new(None, Some(source.clone()), AgentLimits::default());
        turn.transition(AgentState::Deciding { request_ordinal: 1 })
            .unwrap();
        turn.record(AgentEvidence::Decision { ordinal: 1 }).unwrap();
        turn.record(AgentEvidence::ModelResponse {
            ordinal: 1,
            outcome: AgentModelResponseOutcome::Returned,
            response_bytes: Some(1),
        })
        .unwrap();
        turn.transition(AgentState::ToolRunning {
            request_ordinal: 1,
            call_ordinal: 1,
            name: "test.check".into(),
        })
        .unwrap();
        turn.record(AgentEvidence::Tool {
            request_ordinal: 1,
            ordinal: 1,
            name: "test.check".into(),
            outcome: AgentToolOutcome::Success,
            output: Some("PRIVATE_FIELDS".into()),
        })
        .unwrap();
        let evidence = AgentEvidence::Verification {
            request_ordinal: 1,
            call_ordinal: 1,
            observation: VerificationMetadata {
                operation: "test.check".into(),
                source,
                classification: VerificationClassification::Rejected,
                subject: VerificationSubject::HypotheticalCandidate,
                completeness: VerificationCompleteness::SelectedFields,
                authority: "test".into(),
            },
        };
        (turn, evidence)
    }

    #[test]
    fn metadata_roundtrip_and_tampering() {
        let (mut turn, evidence) = fixture();
        turn.record(evidence.clone()).unwrap();
        assert!(turn.record(evidence).is_err());
        turn.transition(AgentState::Deciding { request_ordinal: 2 })
            .unwrap();
        turn.record(AgentEvidence::Decision { ordinal: 2 }).unwrap();
        turn.record(AgentEvidence::ModelResponse {
            ordinal: 2,
            outcome: AgentModelResponseOutcome::Returned,
            response_bytes: Some(1),
        })
        .unwrap();
        turn.transition(AgentState::Completed).unwrap();
        let json = serde_json::to_value(&turn).unwrap();
        assert!(!json.to_string().contains("PRIVATE_FIELDS"));
        assert!(serde_json::from_value::<AgentTurn>(json.clone()).is_ok());
        let index = turn.evidence.len() - 3;
        for key in ["request_ordinal", "call_ordinal"] {
            let mut tampered = json.clone();
            tampered["evidence"][index]["Verification"][key] = 99.into();
            assert!(serde_json::from_value::<AgentTurn>(tampered).is_err());
        }
        let mut duplicate = json.clone();
        let item = duplicate["evidence"][index].clone();
        duplicate["evidence"]
            .as_array_mut()
            .unwrap()
            .insert(index, item);
        assert!(serde_json::from_value::<AgentTurn>(duplicate).is_err());
        let mut delayed = json;
        let items = delayed["evidence"].as_array_mut().unwrap();
        let item = items.remove(index);
        items.push(item);
        assert!(serde_json::from_value::<AgentTurn>(delayed).is_err());
    }

    #[test]
    fn live_verification_requires_current_tool_state_and_bounded_metadata() {
        let (mut turn, evidence) = fixture();
        turn.transition(AgentState::Deciding { request_ordinal: 2 })
            .unwrap();
        assert!(turn.record(evidence).is_err());
        let (turn, mut evidence) = fixture();
        if let AgentEvidence::Verification { observation, .. } = &mut evidence {
            observation.authority = "x".repeat(2049);
        }
        assert!(turn.validate_evidence(&evidence).is_err());
    }
}

#[cfg(test)]
#[path = "agent_lifecycle_tests.rs"]
mod tests;
