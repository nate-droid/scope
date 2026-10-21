//! Public, in-memory source editing. No save, evaluation, or workspace capability.
//! Identities are scoped to the host's document/buffer owner. Hosts must not reuse
//! IDs while proposals remain live, or mix owners with overlapping IDs.
use crate::{
    document::{Document, DocumentId},
    proposal::{DiffDisplay, ProposalError, ProposalId, SourceEditProposal},
    scratch::ScratchBuffer,
    text::TextEdit,
};
use std::collections::BTreeMap;

/// Tagged existing identity; documents and scratch buffers never alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceId {
    Scratch(u64),
    Document(DocumentId),
}
impl SourceId {
    pub fn raw(self) -> u64 {
        match self {
            Self::Scratch(id) => id,
            Self::Document(id) => id.as_u64(),
        }
    }
}

/// A read-only borrow of source context, not a second buffer or revision model.
#[derive(Debug, Clone, Copy)]
pub struct SourceSnapshot<'a> {
    pub id: SourceId,
    pub version: u64,
    pub language: &'a str,
    pub text: &'a str,
}

/// A host-owned candidate. Paths are compared lexically in the owner's namespace;
/// resolution performs no filesystem access and does not open or copy documents.
#[derive(Debug, Clone, Copy)]
pub struct SourceCandidate<'a> {
    pub id: SourceId,
    pub path: Option<&'a std::path::Path>,
    pub language: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub enum TargetSelector<'a> {
    Id(SourceId),
    Path(&'a std::path::Path),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetResolutionError {
    NotFound,
    Ambiguous { candidates: Vec<SourceId> },
}
impl std::fmt::Display for TargetResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "edit target resolution failed: {self:?}")
    }
}
impl std::error::Error for TargetResolutionError {}

/// Explicit selectors never fall through on failure. The host supplies only
/// language-supported workspace candidates (empty language disables fallback).
pub fn resolve_edit_target(
    candidates: &[SourceCandidate<'_>],
    user: Option<TargetSelector<'_>>,
    command: Option<SourceId>,
    active: Option<SourceId>,
) -> Result<SourceId, TargetResolutionError> {
    let selector = user.or(command.or(active).map(TargetSelector::Id));
    let ids: std::collections::BTreeSet<_> = candidates
        .iter()
        .filter(|candidate| match selector {
            Some(TargetSelector::Id(id)) => candidate.id == id,
            Some(TargetSelector::Path(path)) => candidate.path == Some(path),
            None => !candidate.language.is_empty(),
        })
        .map(|candidate| candidate.id)
        .collect();
    match ids.len() {
        0 => Err(TargetResolutionError::NotFound),
        1 => Ok(*ids.first().unwrap()),
        _ => Err(TargetResolutionError::Ambiguous {
            candidates: ids.into_iter().collect(),
        }),
    }
}

/// M1 permits one whole document; selection is context, not range authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermittedEditScope {
    Document(SourceId),
}

/// Borrowed from the existing target, never a second editable document model.
#[derive(Debug, Clone, Copy)]
pub struct AgentEditRequest<'a> {
    pub source: SourceSnapshot<'a>,
    pub selection: Option<crate::text::TextRange>,
    pub scope: PermittedEditScope,
    pub instruction: &'a str,
}
impl<'a> AgentEditRequest<'a> {
    pub fn new(target: &'a impl SourceTarget, instruction: &'a str) -> Self {
        let source = target.snapshot();
        Self {
            scope: PermittedEditScope::Document(source.id),
            source,
            selection: None,
            instruction,
        }
    }
}

/// Narrow source access. Implementations must apply edits transactionally and
/// advance their existing revision, without saving or evaluating source.
pub trait SourceTarget {
    fn source_id(&self) -> SourceId;
    fn text(&self) -> &str;
    fn version(&self) -> u64;
    fn language_id(&self) -> &str;
    /// Language context only; this path grants no filesystem authority.
    fn language_path(&self) -> Option<scope_language::LanguagePath<'_>> {
        None
    }
    fn apply_source_edit(&mut self, edit: TextEdit) -> Result<(), ProposalError>;
    fn snapshot(&self) -> SourceSnapshot<'_> {
        SourceSnapshot {
            id: self.source_id(),
            version: self.version(),
            language: self.language_id(),
            text: self.text(),
        }
    }
}
impl SourceTarget for ScratchBuffer {
    fn source_id(&self) -> SourceId {
        SourceId::Scratch(self.id())
    }
    fn text(&self) -> &str {
        self.text()
    }
    fn version(&self) -> u64 {
        self.version()
    }
    fn language_id(&self) -> &str {
        self.language_id()
    }
    fn apply_source_edit(&mut self, edit: TextEdit) -> Result<(), ProposalError> {
        self.apply_edit(edit).map_err(|e| match e {
            crate::scratch::ScratchError::Text(e) => ProposalError::Text(e),
            e => ProposalError::Application(e.to_string()),
        })
    }
}
/// Borrow the actual document, supplying the host's selected language identifier.
/// No text, identity, revision, or saved-state model is copied into this adapter.
pub struct DocumentSource<'a> {
    document: &'a mut Document,
    language: &'a str,
}
impl<'a> DocumentSource<'a> {
    pub fn new(document: &'a mut Document, language: &'a str) -> Self {
        Self { document, language }
    }
}
impl SourceTarget for DocumentSource<'_> {
    fn source_id(&self) -> SourceId {
        SourceId::Document(self.document.id())
    }
    fn text(&self) -> &str {
        self.document.text()
    }
    fn version(&self) -> u64 {
        self.document.version()
    }
    fn language_id(&self) -> &str {
        self.language
    }
    fn language_path(&self) -> Option<scope_language::LanguagePath<'_>> {
        Some(scope_language::LanguagePath::new(
            self.document.path().to_string_lossy(),
        ))
    }
    fn apply_source_edit(&mut self, edit: TextEdit) -> Result<(), ProposalError> {
        self.document.apply_edit(edit).map_err(ProposalError::Text)
    }
}

#[derive(Debug)]
pub enum ControllerError {
    UnknownProposal(ProposalId),
    DuplicateProposal(ProposalId),
    IdExhausted,
    Validation(Vec<ProposalError>),
    RequiredValidation(ValidationFailure),
    #[cfg(feature = "agent")]
    Generation(crate::agent_edit::SourceEditError),
}
impl std::fmt::Display for ControllerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownProposal(id) => write!(f, "unknown source proposal {}", id.0),
            Self::Validation(errors) => {
                for (i, e) in errors.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{e}")?;
                }
                Ok(())
            }
            _ => write!(f, "source edit controller error: {self:?}"),
        }
    }
}
impl std::error::Error for ControllerError {}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::{
        proposal::ValidationEvidence,
        text::{Position, TextRange},
    };
    use scope_language::{
        CheckObservation, CheckOutcome, ContractValidation, ExpectationOrigin, ValidationContract,
    };

    struct Target {
        id: SourceId,
        version: u64,
        text: String,
    }
    impl SourceTarget for Target {
        fn source_id(&self) -> SourceId {
            self.id
        }
        fn version(&self) -> u64 {
            self.version
        }
        fn text(&self) -> &str {
            &self.text
        }
        fn language_id(&self) -> &str {
            "neutral"
        }
        fn apply_source_edit(&mut self, _: TextEdit) -> Result<(), ProposalError> {
            panic!("must not apply")
        }
    }
    #[test]
    fn task_policy_matrix_and_proposal_snapshot() {
        use crate::task_contract::{Acceptance, AssurancePolicy, TaskContract, TaskCriterion};
        for policy in [
            AssurancePolicy::Exploratory,
            AssurancePolicy::Guided,
            AssurancePolicy::Strict,
        ] {
            for accepted in [false, true] {
                for mandatory in [false, true] {
                    // None covers absent evidence, NotEvaluated preserves unresolved evidence.
                    for outcome in [
                        None,
                        Some(CheckOutcome::NotEvaluated),
                        Some(CheckOutcome::Failed),
                        Some(CheckOutcome::Passed),
                    ] {
                        let (_, target, original) = fixture();
                        let task = TaskContract {
                            original_request: "request".into(),
                            goal: "goal".into(),
                            policy,
                            criteria: BTreeMap::from([(
                                "output".into(),
                                TaskCriterion {
                                    expectation: serde_json::json!(16),
                                    origin: ExpectationOrigin::Agent("agent".into()),
                                    acceptance: if accepted {
                                        Acceptance::Accepted {
                                            consumer: "host".into(),
                                        }
                                    } else {
                                        Acceptance::Provisional
                                    },
                                    mandatory,
                                },
                            )]),
                        };
                        let mut controller = SourceEditController::with_task_contract(task.clone());
                        let mut proposal = SourceEditProposal::new(
                            original.id(),
                            &target,
                            original.edits().to_vec(),
                            None,
                        )
                        .unwrap();
                        if let Some(outcome) = outcome {
                            let mut evidence = original.evidence().unwrap().clone();
                            evidence.contract = controller.validation_contract().unwrap().clone();
                            evidence
                                .observations
                                .checks
                                .get_mut("output")
                                .unwrap()
                                .outcome = outcome;
                            proposal.set_evidence(evidence);
                        }
                        let blocked = accepted
                            && ((policy != AssurancePolicy::Exploratory
                                && outcome == Some(CheckOutcome::Failed))
                                || (policy == AssurancePolicy::Strict
                                    && mandatory
                                    && outcome != Some(CheckOutcome::Passed)));
                        let result = controller.register(proposal, &target);
                        assert_eq!(
                            result.is_err(),
                            blocked,
                            "{policy:?} {accepted} {mandatory} {outcome:?}"
                        );
                        if let Ok(id) = result {
                            let proposal = controller.get(id).unwrap();
                            assert_eq!(proposal.task_contract(), Some(&task));
                            assert_eq!(
                                proposal
                                    .evidence()
                                    .map(|e| e.observations.checks["output"].outcome),
                                outcome
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn task_policies_reject_misbound_evidence_and_language_rejection() {
        use crate::task_contract::{AssurancePolicy, TaskContract};
        for policy in [
            AssurancePolicy::Exploratory,
            AssurancePolicy::Guided,
            AssurancePolicy::Strict,
        ] {
            for mismatch in 0..4 {
                let (_, target, mut proposal) = fixture();
                let mut controller = SourceEditController::with_task_contract(TaskContract {
                    policy,
                    ..Default::default()
                });
                let mut evidence = proposal.evidence().unwrap().clone();
                evidence.contract = controller.validation_contract().unwrap().clone();
                match mismatch {
                    0 => evidence.contract.id.push('x'),
                    1 => evidence.version += 1,
                    2 => evidence.candidate.push('x'),
                    _ => {
                        let mut validation = proposal.validation().clone();
                        validation.status = scope_language::ValidationStatus::Rejected;
                        proposal.set_validation(validation);
                    }
                }
                proposal.set_evidence(evidence);
                assert!(controller.register(proposal, &target).is_err());
            }
        }
    }

    fn fixture() -> (SourceEditController, Target, SourceEditProposal) {
        let contract = ValidationContract {
            id: "required".into(),
            origin: ExpectationOrigin::User("request-1".into()),
            checks: BTreeMap::from([("output".into(), serde_json::json!(16))]),
        };
        let target = Target {
            id: SourceId::Scratch(1),
            version: 1,
            text: "base".into(),
        };
        let mut proposal = SourceEditProposal::new(
            ProposalId(1),
            &target,
            vec![TextEdit::replace(
                TextRange::new(Position::new(0, 0), Position::new(0, 4)),
                "correct",
            )],
            None,
        )
        .unwrap();
        proposal.set_evidence(ValidationEvidence {
            operation: "replace".into(),
            arguments: serde_json::json!({"source":"correct"}),
            contract: contract.clone(),
            observations: ContractValidation {
                checks: BTreeMap::from([(
                    "output".into(),
                    CheckObservation {
                        outcome: CheckOutcome::Passed,
                        actual: serde_json::json!(16),
                    },
                )]),
            },
            target: target.id,
            version: target.version,
            original: target.text.clone(),
            candidate: "correct".into(),
        });
        (
            SourceEditController::with_validation_contract(contract),
            target,
            proposal,
        )
    }
    fn blocked(
        controller: &mut SourceEditController,
        target: &Target,
        proposal: SourceEditProposal,
        reason: ValidationFailure,
    ) {
        assert!(
            matches!(controller.register(proposal, target), Err(ControllerError::RequiredValidation(actual)) if actual == reason)
        );
        assert_eq!(controller.list().count(), 0);
    }
    #[test]
    fn exact_evidence_registers_without_applying() {
        let (mut controller, target, proposal) = fixture();
        let id = controller.register(proposal, &target).unwrap();
        assert_eq!(
            controller.get(id).unwrap().status(),
            crate::proposal::ProposalStatus::Pending
        );
        assert_eq!(target.text, "base");
    }
    #[test]
    fn missing_and_failed_evidence_block_registration() {
        let (mut controller, target, mut proposal) = fixture();
        let mut evidence = proposal.evidence().unwrap().clone();
        evidence
            .observations
            .checks
            .get_mut("output")
            .unwrap()
            .outcome = CheckOutcome::Failed;
        proposal.set_evidence(evidence);
        blocked(
            &mut controller,
            &target,
            proposal,
            ValidationFailure::FailedEvidence,
        );
        let (_, _, valid) = fixture();
        let missing =
            SourceEditProposal::new(ProposalId(2), &target, valid.edits().to_vec(), None).unwrap();
        blocked(
            &mut controller,
            &target,
            missing,
            ValidationFailure::MissingEvidence,
        );
    }
    #[test]
    fn evidence_is_invalid_after_target_revision_or_content_change() {
        for change_version in [false, true] {
            let (mut controller, mut target, proposal) = fixture();
            if change_version {
                target.version += 1;
            } else {
                target.text = "changed".into();
            }
            blocked(
                &mut controller,
                &target,
                proposal,
                ValidationFailure::StaleEvidence,
            );
        }
    }
    #[test]
    fn evidence_cannot_be_transferred_to_another_target_or_contract() {
        let (mut controller, target, mut proposal) = fixture();
        let mut evidence = proposal.evidence().unwrap().clone();
        evidence.target = SourceId::Scratch(2);
        proposal.set_evidence(evidence);
        blocked(
            &mut controller,
            &target,
            proposal,
            ValidationFailure::StaleEvidence,
        );
        let (mut controller, target, mut proposal) = fixture();
        let mut evidence = proposal.evidence().unwrap().clone();
        evidence
            .contract
            .checks
            .insert("output".into(), serde_json::json!(81));
        proposal.set_evidence(evidence);
        blocked(
            &mut controller,
            &target,
            proposal,
            ValidationFailure::ContractMismatch,
        );
    }
    #[test]
    fn modified_candidate_and_transformed_output_cannot_reuse_evidence() {
        let (mut controller, target, valid) = fixture();
        let mut changed = SourceEditProposal::new(
            ProposalId(2),
            &target,
            vec![TextEdit::replace(
                TextRange::new(Position::new(0, 0), Position::new(0, 4)),
                "different",
            )],
            None,
        )
        .unwrap();
        changed.set_evidence(valid.evidence().unwrap().clone());
        blocked(
            &mut controller,
            &target,
            changed,
            ValidationFailure::CandidateMismatch,
        );
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn explicit_path_and_identity_override_command_and_active() {
        let candidates = candidates();
        assert_eq!(
            resolve_edit_target(
                &candidates,
                Some(TargetSelector::Path(Path::new("b.rs"))),
                Some(SourceId::Scratch(1)),
                Some(SourceId::Scratch(1))
            ),
            Ok(SourceId::Scratch(2))
        );
        assert_eq!(
            resolve_edit_target(
                &candidates,
                Some(TargetSelector::Id(SourceId::Scratch(2))),
                None,
                None
            ),
            Ok(SourceId::Scratch(2))
        );
        assert_eq!(
            resolve_edit_target(
                &candidates,
                Some(TargetSelector::Path(Path::new("missing"))),
                Some(SourceId::Scratch(1)),
                None
            ),
            Err(TargetResolutionError::NotFound)
        );
    }

    fn candidates() -> [SourceCandidate<'static>; 2] {
        [
            SourceCandidate {
                id: SourceId::Scratch(1),
                path: Some(Path::new("a.rs")),
                language: "rust",
            },
            SourceCandidate {
                id: SourceId::Scratch(2),
                path: Some(Path::new("b.rs")),
                language: "rust",
            },
        ]
    }

    #[test]
    fn active_command_and_unique_fallback() {
        let candidates = candidates();
        assert_eq!(
            resolve_edit_target(&candidates, None, None, Some(SourceId::Scratch(2))),
            Ok(SourceId::Scratch(2))
        );
        assert_eq!(
            resolve_edit_target(
                &candidates,
                None,
                Some(SourceId::Scratch(1)),
                Some(SourceId::Scratch(2))
            ),
            Ok(SourceId::Scratch(1))
        );
        assert_eq!(
            resolve_edit_target(&candidates[..1], None, None, None),
            Ok(SourceId::Scratch(1))
        );
        assert_eq!(
            resolve_edit_target(&candidates, None, None, None),
            Err(TargetResolutionError::Ambiguous {
                candidates: vec![SourceId::Scratch(1), SourceId::Scratch(2)]
            })
        );
        assert_eq!(
            resolve_edit_target(&[], None, None, None),
            Err(TargetResolutionError::NotFound)
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationFailure {
    UntrustedExpectations,
    MissingEvidence,
    FailedEvidence,
    StaleEvidence,
    CandidateMismatch,
    ContractMismatch,
}

/// One proposal registry shared by CLI and external frontends. Targets remain
/// host-owned; review and acceptance resolve them by passing their live borrow.
#[derive(Default)]
pub struct SourceEditController {
    proposals: BTreeMap<ProposalId, SourceEditProposal>,
    next_id: u64,
    contract: Option<scope_language::ValidationContract>,
    task_contract: Option<crate::task_contract::TaskContract>,
}
impl SourceEditController {
    /// Immutable policy for this registry; model tool arguments cannot replace it.
    pub fn with_validation_contract(contract: scope_language::ValidationContract) -> Self {
        Self {
            contract: Some(contract),
            ..Self::default()
        }
    }
    pub fn with_task_contract(task: crate::task_contract::TaskContract) -> Self {
        Self {
            contract: Some(task.validation_contract()),
            task_contract: Some(task),
            ..Self::default()
        }
    }
    pub fn task_contract(&self) -> Option<&crate::task_contract::TaskContract> {
        self.task_contract.as_ref()
    }
    pub fn validation_contract(&self) -> Option<&scope_language::ValidationContract> {
        self.contract.as_ref()
    }
    pub(crate) fn next(&self) -> Result<ProposalId, ControllerError> {
        self.next_id
            .checked_add(1)
            .map(ProposalId)
            .ok_or(ControllerError::IdExhausted)
    }
    pub fn get(&self, id: ProposalId) -> Result<&SourceEditProposal, ControllerError> {
        self.proposals
            .get(&id)
            .ok_or(ControllerError::UnknownProposal(id))
    }
    pub fn list(&self) -> impl Iterator<Item = &SourceEditProposal> {
        self.proposals.values()
    }
    pub fn register(
        &mut self,
        mut proposal: SourceEditProposal,
        target: &impl SourceTarget,
    ) -> Result<ProposalId, ControllerError> {
        if proposal.validation().status == scope_language::ValidationStatus::Rejected {
            return Err(ControllerError::Validation(vec![
                ProposalError::Application("language rejected candidate".into()),
            ]));
        }
        if let Some(contract) = &self.contract {
            if let Some(task) = &self.task_contract {
                self.check_task_evidence(&proposal, target, contract, task)
                    .map_err(ControllerError::RequiredValidation)?;
            } else {
                self.check_evidence(&proposal, target, contract)
                    .map_err(ControllerError::RequiredValidation)?;
            }
        }
        let id = proposal.id();
        if self.proposals.contains_key(&id) {
            return Err(ControllerError::DuplicateProposal(id));
        }
        proposal
            .preview(target)
            .map_err(ControllerError::Validation)?;
        if let Some(task) = &self.task_contract {
            proposal.set_task_contract(task.clone());
        }
        self.next_id = self.next_id.max(id.0);
        self.proposals.insert(id, proposal);
        Ok(id)
    }
    fn check_task_evidence(
        &self,
        proposal: &SourceEditProposal,
        target: &impl SourceTarget,
        contract: &scope_language::ValidationContract,
        task: &crate::task_contract::TaskContract,
    ) -> Result<(), ValidationFailure> {
        use crate::task_contract::AssurancePolicy;
        use scope_language::CheckOutcome;
        let required = task.policy == AssurancePolicy::Strict
            && task
                .criteria
                .values()
                .any(|c| c.is_accepted() && c.mandatory);
        let Some(evidence) = proposal.evidence() else {
            return if required {
                Err(ValidationFailure::MissingEvidence)
            } else {
                Ok(())
            };
        };
        // Even permissive policies must not attribute observations to a different candidate.
        self.check_evidence_binding(proposal, target, contract)?;
        for (id, criterion) in &task.criteria {
            if !criterion.is_accepted() {
                continue;
            }
            let outcome = evidence.observations.checks.get(id).map(|c| c.outcome);
            if task.policy != AssurancePolicy::Exploratory && outcome == Some(CheckOutcome::Failed)
            {
                return Err(ValidationFailure::FailedEvidence);
            }
            if task.policy == AssurancePolicy::Strict
                && criterion.mandatory
                && outcome != Some(CheckOutcome::Passed)
            {
                return Err(ValidationFailure::MissingEvidence);
            }
        }
        Ok(())
    }
    fn check_evidence(
        &self,
        proposal: &SourceEditProposal,
        target: &impl SourceTarget,
        contract: &scope_language::ValidationContract,
    ) -> Result<(), ValidationFailure> {
        use scope_language::{CheckOutcome, ExpectationOrigin};
        if matches!(contract.origin, ExpectationOrigin::Agent(_)) {
            return Err(ValidationFailure::UntrustedExpectations);
        }
        self.check_evidence_binding(proposal, target, contract)?;
        let evidence = proposal
            .evidence()
            .ok_or(ValidationFailure::MissingEvidence)?;
        if contract.checks.is_empty()
            || evidence.observations.checks.len() != contract.checks.len()
            || contract
                .checks
                .keys()
                .any(|id| !evidence.observations.checks.contains_key(id))
        {
            return Err(ValidationFailure::MissingEvidence);
        }
        if evidence
            .observations
            .checks
            .values()
            .any(|check| check.outcome == CheckOutcome::Failed)
        {
            return Err(ValidationFailure::FailedEvidence);
        }
        if evidence
            .observations
            .checks
            .values()
            .any(|check| check.outcome != CheckOutcome::Passed)
        {
            return Err(ValidationFailure::MissingEvidence);
        }
        Ok(())
    }
    fn check_evidence_binding(
        &self,
        proposal: &SourceEditProposal,
        target: &impl SourceTarget,
        contract: &scope_language::ValidationContract,
    ) -> Result<(), ValidationFailure> {
        let evidence = proposal
            .evidence()
            .ok_or(ValidationFailure::MissingEvidence)?;
        if &evidence.contract != contract {
            return Err(ValidationFailure::ContractMismatch);
        }
        if evidence.target != target.source_id()
            || evidence.version != target.version()
            || evidence.original != target.text()
        {
            return Err(ValidationFailure::StaleEvidence);
        }
        let candidate = proposal
            .preview(target)
            .map_err(|_| ValidationFailure::StaleEvidence)?;
        if evidence.candidate != candidate {
            return Err(ValidationFailure::CandidateMismatch);
        }
        Ok(())
    }
    pub fn propose(
        &mut self,
        target: &impl SourceTarget,
        edits: Vec<TextEdit>,
        rationale: Option<String>,
    ) -> Result<ProposalId, ControllerError> {
        let proposal = SourceEditProposal::new(self.next()?, target, edits, rationale)
            .map_err(|e| ControllerError::Validation(vec![e]))?;
        self.register(proposal, target)
    }
    #[cfg(feature = "agent")]
    pub fn generate(
        &mut self,
        model: &mut impl crate::agent::DecisionModel,
        target: &impl SourceTarget,
        instruction: &str,
        limits: crate::agent_edit::SourceEditLimits,
    ) -> Result<ProposalId, ControllerError> {
        let proposal = crate::agent_edit::generate_source_edit(
            model,
            self.next()?,
            target,
            instruction,
            limits,
        )
        .map_err(ControllerError::Generation)?;
        self.register(proposal, target)
    }
    pub fn preview(
        &self,
        id: ProposalId,
        target: &impl SourceTarget,
    ) -> Result<String, ControllerError> {
        self.get(id)?
            .preview(target)
            .map_err(ControllerError::Validation)
    }
    pub fn diff(
        &self,
        id: ProposalId,
        target: &impl SourceTarget,
    ) -> Result<String, ControllerError> {
        self.get(id)?
            .unified_diff(target)
            .map_err(ControllerError::Validation)
    }
    pub fn diff_bounded(
        &self,
        id: ProposalId,
        target: &impl SourceTarget,
        max_bytes: usize,
    ) -> Result<DiffDisplay, ControllerError> {
        self.get(id)?
            .unified_diff_bounded(target, max_bytes)
            .map_err(ControllerError::Validation)
    }
    pub fn accept(
        &mut self,
        id: ProposalId,
        target: &mut impl SourceTarget,
    ) -> Result<(), ControllerError> {
        self.proposals
            .get_mut(&id)
            .ok_or(ControllerError::UnknownProposal(id))?
            .accept(target)
            .map_err(ControllerError::Validation)
    }
    pub fn reject(&mut self, id: ProposalId) -> Result<(), ControllerError> {
        self.proposals
            .get_mut(&id)
            .ok_or(ControllerError::UnknownProposal(id))?
            .reject();
        Ok(())
    }
}
