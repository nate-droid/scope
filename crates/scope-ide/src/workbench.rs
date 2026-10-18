//! Headless, synchronous workbench for host-controlled document operations.
//!
//! Catalog metadata, model messages, and verification evidence grant no authority.
//! Native providers remain trusted code; cancellation only removes queued work and
//! cannot interrupt an invocation. Checked saves retain the session's best-effort
//! conflict detection (not atomic compare-and-swap).

use crate::document::{
    DocumentError, DocumentSession, DocumentSnapshot, EditDecision, EditError, EditReview, Revision,
};
use crate::tool::editing::{OperationEdit, OperationEditError};
use crate::tool::operations::{
    OperationAuthorization, OperationCall, OperationDescriptor, OperationError, OperationId,
    OperationRegistry, OperationResult,
};
use scope_language::Diagnostic;

/// Purpose is chosen by the host, not inferred from operation names or effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationPurpose {
    Inspect,
    ProposeEdit,
    Verify,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRequest {
    pub call: OperationCall,
    pub revision: Revision,
    pub purpose: OperationPurpose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationApproval {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    Idle,
    AwaitingApproval,
    Completed,
    Denied,
    Cancelled,
    Stale,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalStatus {
    None,
    Proposed,
    Reviewed,
    Rejected,
    Applied,
    Stale,
    ApplyFailed,
}

/// A successful provider invocation is not necessarily a passing verification.
/// Hosts must interpret the validated output using the provider's contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationOutcome {
    Passed,
    Failed,
    Inconclusive,
    /// The provider/host deliberately did not perform this check.
    Skipped,
    /// The check could not be performed (for example, a missing prerequisite).
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStatus {
    NotRun,
    Passed,
    Failed,
    Inconclusive,
    Skipped,
    Unavailable,
    Denied,
    Cancelled,
    ProviderError,
    OperationError,
    Stale,
}

/// Host interpretation of validated output, not an execution receipt or grant.
/// Limitations should describe coverage gaps and reasons for skipped/unavailable
/// checks. An empty list does not imply exhaustive verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub check_name: String,
    pub outcome: VerificationOutcome,
    pub limitations: Vec<String>,
}

/// Evidence describes exactly one accepted source revision, never a future edit.
#[derive(Debug, Clone)]
pub struct VerificationEvidence {
    pub request: OperationRequest,
    pub result: OperationResult,
    pub outcome: VerificationOutcome,
    pub check_name: String,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Host,
    Assistant,
    Status,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchMessage {
    pub kind: MessageKind,
    pub content: String,
}

#[derive(Debug)]
pub enum WorkbenchError {
    Busy,
    NoRequest,
    NoProposal,
    NotReviewed,
    Document(DocumentError),
    Operation(OperationError),
    Proposal(OperationEditError),
    Edit(EditError),
}

impl std::fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("an operation is already awaiting approval"),
            Self::NoRequest => f.write_str("no queued operation"),
            Self::NoProposal => f.write_str("no active proposal"),
            Self::NotReviewed => f.write_str("proposal must be reviewed before applying"),
            Self::Document(e) => e.fmt(f),
            Self::Operation(e) => e.fmt(f),
            Self::Proposal(e) => e.fmt(f),
            Self::Edit(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for WorkbenchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Document(e) => Some(e),
            Self::Operation(e) => Some(e),
            Self::Proposal(e) => Some(e),
            Self::Edit(e) => Some(e),
            _ => None,
        }
    }
}

/// Owns one session and registry. Consumers render borrowed state and issue host
/// decisions. No chat endpoint, language adapter, shell, or execution grant is
/// installed implicitly. Assistant text is display data, not a dispatch protocol.
pub struct Workbench {
    session: DocumentSession,
    registry: OperationRegistry,
    pending: Option<OperationRequest>,
    request_status: RequestStatus,
    proposal: Option<OperationEdit>,
    review: Option<EditReview>,
    proposal_status: ProposalStatus,
    verification: Option<VerificationEvidence>,
    verification_status: VerificationStatus,
    messages: Vec<WorkbenchMessage>,
}

impl Workbench {
    pub fn new(session: DocumentSession, registry: OperationRegistry) -> Self {
        Self {
            session,
            registry,
            pending: None,
            request_status: RequestStatus::Idle,
            proposal: None,
            review: None,
            proposal_status: ProposalStatus::None,
            verification: None,
            verification_status: VerificationStatus::NotRun,
            messages: Vec::new(),
        }
    }

    pub fn operations(&self) -> impl Iterator<Item = (OperationId, &OperationDescriptor)> {
        self.registry.descriptors()
    }
    pub fn pending(&self) -> Option<&OperationRequest> {
        self.pending.as_ref()
    }
    pub fn request_status(&self) -> RequestStatus {
        self.request_status
    }
    pub fn proposal(&self) -> Option<&OperationEdit> {
        self.proposal.as_ref()
    }
    pub fn review(&self) -> Option<&EditReview> {
        self.review.as_ref()
    }
    pub fn proposal_status(&self) -> ProposalStatus {
        self.proposal_status
    }
    pub fn verification(&self) -> Option<&VerificationEvidence> {
        self.verification.as_ref()
    }
    pub fn verification_status(&self) -> VerificationStatus {
        self.verification_status
    }
    pub fn messages(&self) -> &[WorkbenchMessage] {
        &self.messages
    }
    pub fn push_message(&mut self, kind: MessageKind, content: impl Into<String>) {
        self.messages.push(WorkbenchMessage {
            kind,
            content: content.into(),
        });
    }
    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }

    /// Revalidate retained proposals and evidence. Status getters alone do not do I/O.
    pub fn snapshot(&mut self) -> Result<DocumentSnapshot, WorkbenchError> {
        match self.session.snapshot() {
            Ok(snapshot) => {
                self.refresh(&snapshot.revision);
                Ok(snapshot)
            }
            Err(e) => {
                self.invalidate();
                Err(WorkbenchError::Document(e))
            }
        }
    }
    pub fn reload(&mut self) -> Result<DocumentSnapshot, WorkbenchError> {
        match self.session.reload() {
            Ok(snapshot) => {
                self.refresh(&snapshot.revision);
                Ok(snapshot)
            }
            Err(e) => {
                self.invalidate();
                Err(WorkbenchError::Document(e))
            }
        }
    }
    fn invalidate(&mut self) {
        if self.proposal.is_some()
            && !matches!(
                self.proposal_status,
                ProposalStatus::Applied | ProposalStatus::Rejected
            )
        {
            self.proposal_status = ProposalStatus::Stale;
        }
        if self.verification.is_some() {
            self.verification_status = VerificationStatus::Stale;
        }
    }
    fn refresh(&mut self, revision: &Revision) {
        if let Some(p) = &self.proposal
            && &p.proposal().base().revision != revision
            && !matches!(
                self.proposal_status,
                ProposalStatus::Applied | ProposalStatus::Rejected
            )
        {
            self.proposal_status = ProposalStatus::Stale;
        }
        if let Some(e) = &self.verification
            && &e.result.revision != revision
        {
            self.verification_status = VerificationStatus::Stale;
        }
    }

    /// Queue without dispatching. Registry validation and policy run at approval.
    pub fn request(&mut self, request: OperationRequest) -> Result<(), WorkbenchError> {
        if self.pending.is_some() {
            return Err(WorkbenchError::Busy);
        }
        self.pending = Some(request);
        self.request_status = RequestStatus::AwaitingApproval;
        self.push_message(MessageKind::Status, "Operation awaiting host approval");
        Ok(())
    }
    pub fn cancel(&mut self) -> Result<(), WorkbenchError> {
        let request = self.pending.take().ok_or(WorkbenchError::NoRequest)?;
        self.request_status = RequestStatus::Cancelled;
        if request.purpose == OperationPurpose::Verify {
            self.verification = None;
            self.verification_status = VerificationStatus::Cancelled;
        }
        self.push_message(MessageKind::Status, "Queued operation cancelled");
        Ok(())
    }

    /// Explicit approval is necessary but not sufficient: registry policy remains
    /// call-aware and may deny any approved request. `interpret` only classifies
    /// verification output; it cannot authorize invocation or applying an edit.
    pub fn approve(
        &mut self,
        approval: OperationApproval,
        policy: impl FnOnce(OperationAuthorization<'_>) -> bool,
        interpret: impl FnOnce(&OperationResult) -> VerificationOutcome,
    ) -> Result<OperationResult, WorkbenchError> {
        let call = &self.pending.as_ref().ok_or(WorkbenchError::NoRequest)?.call;
        let check_name = format!(
            "{}/{}",
            call.operation.provider_id, call.operation.action_id
        );
        self.approve_with_verification(approval, policy, |result| VerificationReport {
            check_name,
            outcome: interpret(result),
            limitations: vec![
                "Outcome-only interpreter supplied no check-specific coverage or limitations"
                    .into(),
            ],
        })
    }

    /// Like `approve`, with an explicit check name and coverage limitations retained
    /// in revision-bound evidence. Called only for successful Verify invocations;
    /// skipped/unavailable output is not treated as a pass or a provider error.
    pub fn approve_with_verification(
        &mut self,
        approval: OperationApproval,
        policy: impl FnOnce(OperationAuthorization<'_>) -> bool,
        interpret: impl FnOnce(&OperationResult) -> VerificationReport,
    ) -> Result<OperationResult, WorkbenchError> {
        let request = self.pending.take().ok_or(WorkbenchError::NoRequest)?;
        let authorize = |context: OperationAuthorization<'_>| {
            approval == OperationApproval::Allow && policy(context)
        };
        let result = if request.purpose == OperationPurpose::ProposeEdit {
            self.registry
                .propose_edit(
                    &mut self.session,
                    &request.revision,
                    &request.call,
                    authorize,
                )
                .map(|edit| {
                    let result = edit.result().clone();
                    self.proposal = Some(edit);
                    self.review = None;
                    self.proposal_status = ProposalStatus::Proposed;
                    result
                })
                .map_err(WorkbenchError::Proposal)
        } else {
            self.registry
                .invoke(
                    &mut self.session,
                    &request.revision,
                    &request.call,
                    authorize,
                )
                .map_err(WorkbenchError::Operation)
        };
        match result {
            Ok(result) => {
                self.request_status = RequestStatus::Completed;
                if request.purpose == OperationPurpose::Verify {
                    let report = interpret(&result);
                    self.verification_status = match report.outcome {
                        VerificationOutcome::Passed => VerificationStatus::Passed,
                        VerificationOutcome::Failed => VerificationStatus::Failed,
                        VerificationOutcome::Inconclusive => VerificationStatus::Inconclusive,
                        VerificationOutcome::Skipped => VerificationStatus::Skipped,
                        VerificationOutcome::Unavailable => VerificationStatus::Unavailable,
                    };
                    self.verification = Some(VerificationEvidence {
                        request,
                        result: result.clone(),
                        outcome: report.outcome,
                        check_name: report.check_name,
                        limitations: report.limitations,
                    });
                }
                self.push_message(MessageKind::Status, "Operation completed");
                Ok(result)
            }
            Err(error) => {
                let operation = match &error {
                    WorkbenchError::Operation(e)
                    | WorkbenchError::Proposal(OperationEditError::Operation(e)) => Some(e),
                    _ => None,
                };
                self.request_status = match operation {
                    Some(OperationError::Unauthorized) => RequestStatus::Denied,
                    Some(OperationError::StaleRevision) => RequestStatus::Stale,
                    _ => RequestStatus::Failed,
                };
                if request.purpose == OperationPurpose::Verify {
                    self.verification = None;
                    self.verification_status = match self.request_status {
                        RequestStatus::Denied => VerificationStatus::Denied,
                        RequestStatus::Stale => VerificationStatus::Stale,
                        _ if matches!(operation, Some(OperationError::ProviderFailure(_))) => {
                            VerificationStatus::ProviderError
                        }
                        _ => VerificationStatus::OperationError,
                    };
                }
                // A stale request token need not mean the accepted source changed.
                // Also recheck after provider errors, which may follow side effects.
                let _ = self.snapshot();
                self.push_message(MessageKind::Error, error.to_string());
                Err(error)
            }
        }
    }

    /// Diagnostics must be computed by the host from the exact candidate snapshot.
    pub fn review_proposal(
        &mut self,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<&EditReview, WorkbenchError> {
        let proposal = self.proposal.as_ref().ok_or(WorkbenchError::NoProposal)?;
        if matches!(
            self.proposal_status,
            ProposalStatus::Rejected | ProposalStatus::Applied
        ) {
            return Err(WorkbenchError::NoProposal);
        }
        match self.session.review(proposal.proposal(), diagnostics) {
            Ok(review) => {
                self.review = Some(review);
                self.proposal_status = ProposalStatus::Reviewed;
                Ok(self.review.as_ref().unwrap())
            }
            Err(error) => {
                self.edit_failure(&error);
                Err(WorkbenchError::Edit(error))
            }
        }
    }

    /// A separate host decision for the exact reviewed candidate. Verification is
    /// informative only: even passing evidence never implicitly allows a save.
    pub fn decide_edit(
        &mut self,
        decision: EditDecision,
    ) -> Result<Option<DocumentSnapshot>, WorkbenchError> {
        if self.proposal_status != ProposalStatus::Reviewed
            && self.proposal_status != ProposalStatus::ApplyFailed
        {
            return Err(WorkbenchError::NotReviewed);
        }
        let review = self.review.as_ref().ok_or(WorkbenchError::NotReviewed)?;
        let result = self
            .session
            .authorize(review, decision)
            .and_then(|authorization| {
                if decision == EditDecision::Deny {
                    Ok(None)
                } else {
                    self.session.apply(review, &authorization).map(Some)
                }
            });
        match result {
            Ok(snapshot) => {
                self.proposal_status = if snapshot.is_some() {
                    ProposalStatus::Applied
                } else {
                    ProposalStatus::Rejected
                };
                if let Some(s) = &snapshot {
                    self.refresh(&s.revision);
                }
                self.push_message(
                    MessageKind::Status,
                    if snapshot.is_some() {
                        "Edit applied"
                    } else {
                        "Edit rejected"
                    },
                );
                Ok(snapshot)
            }
            Err(error) => {
                self.edit_failure(&error);
                Err(WorkbenchError::Edit(error))
            }
        }
    }
    fn edit_failure(&mut self, error: &EditError) {
        self.proposal_status = if matches!(error, EditError::Document(DocumentError::StaleRevision))
        {
            ProposalStatus::Stale
        } else {
            ProposalStatus::ApplyFailed
        };
        let _ = self.snapshot();
        self.push_message(MessageKind::Error, error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::operations::{
        OperationExecution, OperationFailure, OperationLimits, OperationProvider, OperationSchema,
    };
    use crate::tool::{ToolArguments, ToolEffect, ToolValue};
    use crate::{document::DocumentId, test_support::TempWorkspace, workspace::Workspace};
    use std::{cell::Cell, collections::BTreeMap, rc::Rc};

    struct Neutral {
        descriptors: Vec<OperationDescriptor>,
        calls: Rc<Cell<usize>>,
        fail: bool,
    }
    impl OperationProvider for Neutral {
        fn provider_id(&self) -> &str {
            "neutral"
        }
        fn descriptors(&self) -> &[OperationDescriptor] {
            &self.descriptors
        }
        fn invoke(
            &self,
            _: &str,
            _: &DocumentSnapshot,
            _: &ToolArguments,
        ) -> Result<ToolValue, OperationFailure> {
            self.calls.set(self.calls.get() + 1);
            if self.fail {
                Err(OperationFailure {
                    message: "provider unavailable".into(),
                })
            } else {
                Ok(ToolValue::String("replacement".into()))
            }
        }
    }
    fn setup(readonly: bool, fail: bool) -> (TempWorkspace, Workbench, Rc<Cell<usize>>) {
        let fixture = TempWorkspace::new("workbench");
        fixture.write_file("example.txt", "original");
        let workspace = if readonly {
            Workspace::read_only(fixture.root().to_string_lossy()).unwrap()
        } else {
            fixture.workspace()
        };
        let session = DocumentSession::open(workspace, DocumentId::new(1), "example.txt").unwrap();
        let calls = Rc::new(Cell::new(0));
        let mut registry = OperationRegistry::new(OperationLimits::default());
        registry
            .register(Box::new(Neutral {
                descriptors: vec![OperationDescriptor {
                    action_id: "replace".into(),
                    description: "Neutral replacement".into(),
                    input: OperationSchema::Object {
                        fields: BTreeMap::new(),
                        required: Default::default(),
                    },
                    output: OperationSchema::String,
                    effect: ToolEffect::Read,
                    execution: OperationExecution::Immediate,
                    authorization: ["host".into()].into(),
                }],
                calls: calls.clone(),
                fail,
            }))
            .unwrap();
        (fixture, Workbench::new(session, registry), calls)
    }
    fn queue(w: &mut Workbench, purpose: OperationPurpose) {
        let revision = w.snapshot().unwrap().revision;
        w.request(OperationRequest {
            revision,
            purpose,
            call: OperationCall {
                operation: OperationId {
                    provider_id: "neutral".into(),
                    action_id: "replace".into(),
                },
                arguments: BTreeMap::new(),
            },
        })
        .unwrap();
    }
    fn propose(w: &mut Workbench) {
        queue(w, OperationPurpose::ProposeEdit);
        w.approve(
            OperationApproval::Allow,
            |context| context.descriptor.authorization.contains("host"),
            |_| VerificationOutcome::Inconclusive,
        )
        .unwrap();
        w.review_proposal(vec![]).unwrap();
    }
    #[test]
    fn neutral_proposal_rejects_without_changing_source() {
        let (f, mut w, calls) = setup(false, false);
        let base = w.snapshot().unwrap();
        propose(&mut w);
        assert_eq!(calls.get(), 1);
        assert!(w.review().unwrap().diff().contains("+replacement"));
        assert_eq!(w.decide_edit(EditDecision::Deny).unwrap(), None);
        assert_eq!(w.proposal_status(), ProposalStatus::Rejected);
        assert_eq!(w.snapshot().unwrap(), base);
        assert_eq!(f.read_file("example.txt"), "original");
    }
    #[test]
    fn readonly_apply_failure_preserves_source() {
        let (f, mut w, _) = setup(true, false);
        propose(&mut w);
        assert!(matches!(
            w.decide_edit(EditDecision::Allow),
            Err(WorkbenchError::Edit(EditError::Save(_)))
        ));
        assert_eq!(w.proposal_status(), ProposalStatus::ApplyFailed);
        assert_eq!(w.snapshot().unwrap().text, "original");
        assert_eq!(f.read_file("example.txt"), "original");
    }
    #[test]
    fn stale_proposal_and_queued_request_never_apply_or_dispatch() {
        let (f, mut w, calls) = setup(false, false);
        propose(&mut w);
        queue(&mut w, OperationPurpose::Inspect);
        f.write_file("example.txt", "external");
        assert!(matches!(
            w.decide_edit(EditDecision::Allow),
            Err(WorkbenchError::Edit(EditError::Document(
                DocumentError::StaleRevision
            )))
        ));
        assert_eq!(w.proposal_status(), ProposalStatus::Stale);
        assert!(
            w.approve(
                OperationApproval::Allow,
                |_| true,
                |_| VerificationOutcome::Passed
            )
            .is_err()
        );
        assert_eq!(w.request_status(), RequestStatus::Stale);
        assert_eq!(calls.get(), 1);
        w.reload().unwrap();
        assert!(w.review_proposal(vec![]).is_err());
        assert_eq!(f.read_file("example.txt"), "external");
    }
    #[test]
    fn verification_outcomes_are_distinct_and_revision_bound() {
        let (_fixture, mut w, _) = setup(false, false);
        assert_eq!(w.verification_status(), VerificationStatus::NotRun);
        for (outcome, status) in [
            (VerificationOutcome::Passed, VerificationStatus::Passed),
            (VerificationOutcome::Failed, VerificationStatus::Failed),
            (VerificationOutcome::Skipped, VerificationStatus::Skipped),
            (
                VerificationOutcome::Unavailable,
                VerificationStatus::Unavailable,
            ),
            (
                VerificationOutcome::Inconclusive,
                VerificationStatus::Inconclusive,
            ),
        ] {
            queue(&mut w, OperationPurpose::Verify);
            w.approve(OperationApproval::Allow, |_| true, |_| outcome)
                .unwrap();
            assert_eq!(w.verification_status(), status);
            let current = w.snapshot().unwrap();
            assert_eq!(w.verification().unwrap().result.revision, current.revision);
            assert_eq!(w.verification().unwrap().check_name, "neutral/replace");
            assert!(!w.verification().unwrap().limitations.is_empty());
        }
        propose(&mut w);
        w.decide_edit(EditDecision::Allow).unwrap();
        assert_eq!(w.verification_status(), VerificationStatus::Stale);
        assert_eq!(w.proposal_status(), ProposalStatus::Applied);
    }
    #[test]
    fn named_skipped_and_unavailable_checks_retain_limitations() {
        let (_fixture, mut w, _) = setup(false, false);
        for (outcome, status, reason) in [
            (
                VerificationOutcome::Skipped,
                VerificationStatus::Skipped,
                "Host disabled integration checks",
            ),
            (
                VerificationOutcome::Unavailable,
                VerificationStatus::Unavailable,
                "No configured runner",
            ),
        ] {
            queue(&mut w, OperationPurpose::Verify);
            w.approve_with_verification(
                OperationApproval::Allow,
                |_| true,
                |_| VerificationReport {
                    check_name: "Integration checks".into(),
                    outcome,
                    limitations: vec![reason.into()],
                },
            )
            .unwrap();
            assert_eq!(w.verification_status(), status);
            let evidence = w.verification().unwrap();
            assert_eq!(evidence.outcome, outcome);
            assert_eq!(evidence.check_name, "Integration checks");
            assert_eq!(evidence.limitations, vec![reason.to_string()]);
        }
    }

    #[test]
    fn stale_request_does_not_invalidate_current_evidence() {
        let (fixture, mut w, _) = setup(false, false);
        let old = w.snapshot().unwrap().revision;
        fixture.write_file("example.txt", "new source");
        w.reload().unwrap();
        queue(&mut w, OperationPurpose::Verify);
        w.approve(
            OperationApproval::Allow,
            |_| true,
            |_| VerificationOutcome::Passed,
        )
        .unwrap();
        let mut request = w.verification().unwrap().request.clone();
        request.revision = old;
        request.purpose = OperationPurpose::Inspect;
        w.request(request).unwrap();
        assert!(
            w.approve(
                OperationApproval::Allow,
                |_| true,
                |_| VerificationOutcome::Passed
            )
            .is_err()
        );
        assert_eq!(w.request_status(), RequestStatus::Stale);
        assert_eq!(w.verification_status(), VerificationStatus::Passed);
    }

    #[test]
    fn stale_edit_invalidates_verification_evidence() {
        let (fixture, mut w, _) = setup(false, false);
        queue(&mut w, OperationPurpose::Verify);
        w.approve(
            OperationApproval::Allow,
            |_| true,
            |_| VerificationOutcome::Passed,
        )
        .unwrap();
        propose(&mut w);
        fixture.write_file("example.txt", "external");
        assert!(w.decide_edit(EditDecision::Allow).is_err());
        assert_eq!(w.proposal_status(), ProposalStatus::Stale);
        assert_eq!(w.verification_status(), VerificationStatus::Stale);
    }

    #[test]
    fn cancellation_denial_and_provider_errors_are_not_success() {
        let (_fixture, mut w, calls) = setup(false, true);
        queue(&mut w, OperationPurpose::Verify);
        w.cancel().unwrap();
        assert_eq!(w.verification_status(), VerificationStatus::Cancelled);
        assert!(matches!(
            w.approve(
                OperationApproval::Allow,
                |_| true,
                |_| VerificationOutcome::Passed
            ),
            Err(WorkbenchError::NoRequest)
        ));
        queue(&mut w, OperationPurpose::Verify);
        assert!(
            w.approve(
                OperationApproval::Deny,
                |_| panic!("denied policy must not run"),
                |_| panic!("no evidence")
            )
            .is_err()
        );
        assert_eq!(w.verification_status(), VerificationStatus::Denied);
        queue(&mut w, OperationPurpose::Inspect);
        assert!(
            w.approve(
                OperationApproval::Allow,
                |_| false,
                |_| panic!("no evidence")
            )
            .is_err()
        );
        assert_eq!(calls.get(), 0);
        queue(&mut w, OperationPurpose::Verify);
        assert!(matches!(
            w.approve(
                OperationApproval::Allow,
                |_| true,
                |_| VerificationOutcome::Passed
            ),
            Err(WorkbenchError::Operation(OperationError::ProviderFailure(
                _
            )))
        ));
        assert_eq!(w.request_status(), RequestStatus::Failed);
        assert_eq!(w.verification_status(), VerificationStatus::ProviderError);
        assert!(w.verification().is_none());
        assert_eq!(calls.get(), 1);
        assert_eq!(w.messages().last().unwrap().kind, MessageKind::Error);
    }
}
