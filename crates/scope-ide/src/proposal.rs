//! Single-buffer source proposals with explicit review. No evaluation or I/O.
//!
//! Coordinates use existing half-open `TextRange`s: zero-based lines and UTF-8
//! byte columns on character boundaries. All edits address the original source.
mod diff;

use crate::source_edit::{SourceId, SourceTarget};
use crate::text::{Span, TextBuffer, TextBufferError, TextEdit};

/// Caller-allocated identity, unique within the caller's proposal collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProposalId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalStatus {
    Pending,
    Applied,
    Rejected,
    Discarded,
}

/// Immutable proposal payload with pending-to-terminal lifecycle transitions.
/// Tagged target IDs and versions are scoped to the originating source owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEditProposal {
    id: ProposalId,
    target: SourceId,
    expected_source: String,
    expected_version: u64,
    edits: Vec<TextEdit>,
    rationale: Option<String>,
    status: ProposalStatus,
    validation: scope_language::CandidateValidation,
    evidence: Option<ValidationEvidence>,
    task_contract: Option<crate::task_contract::TaskContract>,
}

/// Host-retained binding, not an authorization token. Full source is internal;
/// model-facing feedback contains only the contract and observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationEvidence {
    pub operation: String,
    pub arguments: serde_json::Value,
    pub contract: scope_language::ValidationContract,
    pub observations: scope_language::ContractValidation,
    pub target: SourceId,
    pub version: u64,
    pub original: String,
    pub candidate: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalError {
    EmptyEdits,
    SourceConflict,
    WrongTarget {
        expected: SourceId,
        actual: SourceId,
    },
    Applied,
    Rejected,
    Discarded,
    WrongBuffer {
        expected: u64,
        actual: u64,
    },
    RevisionConflict {
        expected: u64,
        actual: u64,
    },
    InvalidEdit {
        index: usize,
        error: TextBufferError,
    },
    /// Original input indices. Shared starts are rejected, including insertions.
    OverlappingEdits {
        first: usize,
        second: usize,
    },
    Text(TextBufferError),
    /// Non-text scratch failures, represented without exposing scratch capabilities.
    Application(String),
}

impl std::fmt::Display for ProposalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "source proposal error: {self:?}")
    }
}
impl std::error::Error for ProposalError {}

/// Bounded display output, not an applicable patch or an acceptance candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffDisplay {
    pub text: String,
    pub truncated: bool,
}

impl SourceEditProposal {
    /// Capture the exact current buffer version without acquiring capabilities.
    pub fn new(
        id: ProposalId,
        buffer: &impl SourceTarget,
        edits: Vec<TextEdit>,
        rationale: Option<String>,
    ) -> Result<Self, ProposalError> {
        if edits.is_empty() {
            return Err(ProposalError::EmptyEdits);
        }
        Ok(Self {
            id,
            target: buffer.source_id(),
            expected_source: buffer.text().to_owned(),
            expected_version: buffer.version(),
            edits,
            rationale,
            status: ProposalStatus::Pending,
            validation: Default::default(),
            evidence: None,
            task_contract: None,
        })
    }

    /// Language evidence for this proposal's original revision; registration is
    /// structural validation only. Staleness checks remain authoritative.
    pub fn validation(&self) -> &scope_language::CandidateValidation {
        &self.validation
    }
    pub(crate) fn set_validation(&mut self, validation: scope_language::CandidateValidation) {
        self.validation = validation;
    }
    pub fn task_contract(&self) -> Option<&crate::task_contract::TaskContract> {
        self.task_contract.as_ref()
    }
    pub(crate) fn set_task_contract(&mut self, task: crate::task_contract::TaskContract) {
        self.task_contract = Some(task);
    }
    /// Check IDs not established as passing. Provisional criteria remain visible
    /// here, but do not become authoritative requirements.
    pub fn unresolved_obligations(&self) -> Vec<&str> {
        let Some(evidence) = &self.evidence else {
            return self
                .task_contract()
                .map(|task| task.criteria.keys().map(String::as_str).collect())
                .unwrap_or_default();
        };
        evidence
            .contract
            .checks
            .keys()
            .filter(|id| {
                evidence
                    .observations
                    .checks
                    .get(*id)
                    .is_none_or(|check| check.outcome != scope_language::CheckOutcome::Passed)
            })
            .map(String::as_str)
            .collect()
    }

    pub fn evidence(&self) -> Option<&ValidationEvidence> {
        self.evidence.as_ref()
    }
    pub(crate) fn set_evidence(&mut self, evidence: ValidationEvidence) {
        self.evidence = Some(evidence);
    }
    pub fn id(&self) -> ProposalId {
        self.id
    }
    pub fn target(&self) -> SourceId {
        self.target
    }
    pub fn expected_source(&self) -> &str {
        &self.expected_source
    }
    /// Legacy numeric target field. Use `target()` to distinguish source kinds.
    pub fn target_buffer(&self) -> u64 {
        self.target.raw()
    }
    pub fn expected_version(&self) -> u64 {
        self.expected_version
    }
    pub fn edits(&self) -> &[TextEdit] {
        &self.edits
    }
    pub fn rationale(&self) -> Option<&str> {
        self.rationale.as_deref()
    }
    pub fn status(&self) -> ProposalStatus {
        self.status
    }

    /// Idempotent; does not release, edit, or save the target buffer.
    pub fn discard(&mut self) {
        if self.status == ProposalStatus::Pending {
            self.status = ProposalStatus::Discarded;
        }
    }

    /// Idempotent rejection. Terminal proposals cannot be reopened or relabeled.
    pub fn reject(&mut self) {
        if self.status == ProposalStatus::Pending {
            self.status = ProposalStatus::Rejected;
        }
    }

    /// Revalidate the live target, then commit exactly one full-document edit.
    /// Failures preserve both proposal and buffer; success dirties but never saves.
    pub fn accept(&mut self, buffer: &mut impl SourceTarget) -> Result<(), Vec<ProposalError>> {
        let candidate = self.preview(buffer)?;
        let source = TextBuffer::new(buffer.text());
        let range = source
            .range_for_span(Span::new(0, buffer.text().len()))
            .map_err(|error| vec![ProposalError::Text(error)])?;
        buffer
            .apply_source_edit(TextEdit::replace(range, candidate))
            .map_err(|error| vec![error])?;
        self.status = ProposalStatus::Applied;
        Ok(())
    }

    /// Pure unified diff with three surrounding context lines and stable labels.
    /// An unchanged candidate produces an empty diff.
    pub fn unified_diff(&self, buffer: &impl SourceTarget) -> Result<String, Vec<ProposalError>> {
        let candidate = self.preview(buffer)?;
        Ok(diff::unified(buffer.text(), &candidate))
    }

    /// UTF-8-safe display prefix limited to `max_bytes` (including headers).
    /// `truncated` explicitly signals an incomplete, display-only diff; the
    /// proposal payload and full acceptance candidate are never shortened.
    /// This bounds returned display size, not preview computation or allocation.
    pub fn unified_diff_bounded(
        &self,
        buffer: &impl SourceTarget,
        max_bytes: usize,
    ) -> Result<DiffDisplay, Vec<ProposalError>> {
        let mut text = self.unified_diff(buffer)?;
        let truncated = text.len() > max_bytes;
        if truncated {
            let mut end = max_bytes;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        Ok(DiffDisplay { text, truncated })
    }

    /// Deterministic JSON inspection/export. IDs retain their integer precision.
    /// Exported data is not an authorization or a portable buffer identity.
    pub fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "id": self.id.0,
            "target_buffer": self.target.raw(),
            "expected_version": self.expected_version,
            "edits": self.edits.iter().map(|edit| serde_json::json!({
                "range": {
                    "start": {"line": edit.range.start.line, "column": edit.range.start.column},
                    "end": {"line": edit.range.end.line, "column": edit.range.end.column}
                },
                "replacement": edit.replacement
            })).collect::<Vec<_>>(),
            "rationale": self.rationale,
            "validation": {
                "status": format!("{:?}", self.validation.status),
                "limitations": self.validation.limitations,
                "missing_dependencies": self.validation.missing_dependencies,
                "diagnostic_source": "candidate",
                "diagnostics": self.validation.diagnostics.iter().map(|d| serde_json::json!({
                    "code": d.code, "severity": format!("{:?}", d.severity),
                    "message": d.message, "source": d.source,
                    "range": {
                        "start": {"line": d.range.start.line, "column": d.range.start.column},
                        "end": {"line": d.range.end.line, "column": d.range.end.column}
                    }
                })).collect::<Vec<_>>()
            },
            "status": match self.status {
                ProposalStatus::Pending => "pending",
                ProposalStatus::Applied => "applied",
                ProposalStatus::Rejected => "rejected",
                ProposalStatus::Discarded => "discarded",
            }
        });
        // Preserve the established scratch export while tagging document targets.
        if let SourceId::Document(id) = self.target {
            value["target"] = serde_json::json!({"kind": "document", "id": id.as_u64()});
        }
        value
    }

    /// Pure preview against a caller-supplied live buffer. Conflict/lifecycle
    /// errors precede range validation. Range errors are collected in input order;
    /// overlap errors in source order. Failure never returns a partial candidate.
    /// Adjacent edits are allowed, including insertion at a replacement's end;
    /// insertion inside a replacement or at its start is rejected.
    pub fn preview(&self, buffer: &impl SourceTarget) -> Result<String, Vec<ProposalError>> {
        let mut errors = Vec::new();
        match self.status {
            ProposalStatus::Pending => {}
            ProposalStatus::Applied => errors.push(ProposalError::Applied),
            ProposalStatus::Rejected => errors.push(ProposalError::Rejected),
            ProposalStatus::Discarded => errors.push(ProposalError::Discarded),
        }
        if self.target != buffer.source_id() {
            errors.push(match (self.target, buffer.source_id()) {
                (SourceId::Scratch(expected), SourceId::Scratch(actual)) => {
                    ProposalError::WrongBuffer { expected, actual }
                }
                (expected, actual) => ProposalError::WrongTarget { expected, actual },
            });
        }
        if self.expected_version != buffer.version() {
            errors.push(ProposalError::RevisionConflict {
                expected: self.expected_version,
                actual: buffer.version(),
            });
        }
        if self.expected_source != buffer.text() {
            errors.push(ProposalError::SourceConflict);
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut candidate = TextBuffer::new(buffer.text());
        let mut spans: Vec<(usize, Span)> = Vec::new();
        for (index, edit) in self.edits.iter().enumerate() {
            match candidate.span_for_range(edit.range) {
                Ok(span) => spans.push((index, span)),
                Err(error) => errors.push(ProposalError::InvalidEdit { index, error }),
            }
        }
        spans.sort_by_key(|(index, span)| (span.start, *index));
        for (ordinal, (first, left)) in spans.iter().enumerate() {
            for (second, right) in &spans[ordinal + 1..] {
                if right.start > left.end {
                    break;
                }
                if left.start == right.start || left.end > right.start {
                    errors.push(ProposalError::OverlappingEdits {
                        first: *first,
                        second: *second,
                    });
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        candidate
            .apply_edits(self.edits.clone())
            .map_err(|error| vec![ProposalError::Text(error)])?;
        Ok(candidate.into_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchBuffers;
    use crate::text::{Position, TextRange};

    fn replacement(start: usize, end: usize, text: &str) -> TextEdit {
        TextEdit::replace(
            TextRange::new(Position::new(0, start), Position::new(0, end)),
            text,
        )
    }
    fn buffers(text: &str) -> ScratchBuffers {
        let mut buffers = ScratchBuffers::default();
        buffers.create("test", text);
        buffers
    }

    #[test]
    fn single_and_multiple_edits_preserve_live_state() {
        let buffers = buffers("aé b\r\nsecond\n");
        let buffer = buffers.selected().unwrap();
        let before = (
            buffer.text().to_owned(),
            buffer.version(),
            buffer.is_dirty(),
        );
        for (edits, expected) in [
            (vec![replacement(1, 3, "🦀")], "a🦀 b\r\nsecond\n"),
            (
                vec![
                    replacement(4, 5, ""),
                    TextEdit::insert(Position::new(1, 6), "!"),
                    replacement(1, 3, "🦀"),
                ],
                "a🦀 \r\nsecond!\n",
            ),
            (
                vec![TextEdit::replace(
                    TextRange::new(Position::new(0, 3), Position::new(1, 3)),
                    "\n",
                )],
                "aé\nond\n",
            ),
        ] {
            let proposal = SourceEditProposal::new(ProposalId(7), buffer, edits, None).unwrap();
            assert_eq!(proposal.preview(buffer).unwrap(), expected);
            assert_eq!(proposal.preview(buffer), proposal.preview(buffer));
            assert_eq!(
                proposal.to_json().to_string(),
                proposal.to_json().to_string()
            );
        }
        assert_eq!(
            before,
            (
                buffer.text().to_owned(),
                buffer.version(),
                buffer.is_dirty()
            )
        );
    }

    #[test]
    fn rejects_invalid_ranges_transactionally() {
        let buffers = buffers("éabc");
        let buffer = buffers.selected().unwrap();
        let proposal = SourceEditProposal::new(
            ProposalId(1),
            buffer,
            vec![
                replacement(1, 2, "x"),
                replacement(3, 2, "x"),
                replacement(0, 99, "x"),
            ],
            None,
        )
        .unwrap();
        let errors = proposal.preview(buffer).unwrap_err();
        assert_eq!(errors.len(), 3);
        for (index, error) in errors.iter().enumerate() {
            assert!(
                matches!(error, ProposalError::InvalidEdit { index: actual, .. } if *actual == index)
            );
        }
        assert_eq!(buffer.text(), "éabc");
    }

    #[test]
    fn overlap_and_boundary_policy() {
        let buffers = buffers("abcdef");
        let buffer = buffers.selected().unwrap();
        for edits in [
            vec![replacement(0, 4, ""), replacement(2, 5, "")],
            vec![replacement(0, 4, ""), replacement(2, 2, "x")],
            vec![replacement(2, 2, "x"), replacement(2, 2, "y")],
            vec![replacement(2, 4, "x"), replacement(2, 2, "y")],
        ] {
            let proposal = SourceEditProposal::new(ProposalId(1), buffer, edits, None).unwrap();
            assert!(matches!(
                proposal.preview(buffer).unwrap_err()[0],
                ProposalError::OverlappingEdits { .. }
            ));
        }
        let proposal = SourceEditProposal::new(
            ProposalId(1),
            buffer,
            vec![replacement(2, 4, "X"), replacement(4, 4, "Y")],
            None,
        )
        .unwrap();
        assert_eq!(proposal.preview(buffer).unwrap(), "abXYef");
    }

    #[test]
    fn stale_wrong_target_and_discard() {
        let mut buffers = buffers("abc");
        let mut proposal = SourceEditProposal::new(
            ProposalId(1),
            buffers.selected().unwrap(),
            vec![replacement(0, 1, "a")],
            Some("inspect".into()),
        )
        .unwrap();
        let original = proposal.to_json();
        buffers
            .selected_mut()
            .unwrap()
            .apply_edit(replacement(0, 1, "a"))
            .unwrap();
        assert!(matches!(
            proposal.preview(buffers.selected().unwrap()).unwrap_err()[0],
            ProposalError::RevisionConflict { .. }
        ));
        assert_eq!(proposal.to_json(), original);
        buffers.create("test", "abc");
        assert!(
            proposal
                .preview(buffers.selected().unwrap())
                .unwrap_err()
                .iter()
                .any(|e| matches!(e, ProposalError::WrongBuffer { .. }))
        );
        proposal.discard();
        proposal.discard();
        assert_eq!(proposal.status(), ProposalStatus::Discarded);
        assert_eq!(
            proposal.preview(buffers.selected().unwrap()).unwrap_err()[0],
            ProposalError::Discarded
        );
    }

    #[test]
    fn workspace_backed_preview_does_not_persist_or_change_baseline() {
        let fixture = crate::test_support::TempWorkspace::new("source-proposal");
        fixture.write_file("a.txt", "original\r\n");
        let mut buffers = ScratchBuffers::default();
        buffers
            .open(
                &fixture.workspace(),
                &crate::language::LanguageRegistry::builtins(),
                "a.txt",
            )
            .unwrap();
        let buffer = buffers.selected().unwrap();
        let baseline = buffer.destination_revision().unwrap().to_owned();
        let version = buffer.version();
        let proposal = SourceEditProposal::new(
            ProposalId(1),
            buffer,
            vec![replacement(0, 8, "candidate")],
            None,
        )
        .unwrap();
        assert_eq!(proposal.preview(buffer).unwrap(), "candidate\r\n");
        assert_eq!(buffer.text(), "original\r\n");
        assert_eq!(buffer.version(), version);
        assert!(!buffer.is_dirty());
        assert_eq!(buffer.destination_revision(), Some(baseline.as_str()));
        assert_eq!(fixture.read_file("a.txt"), "original\r\n");
    }

    #[test]
    fn acceptance_is_one_atomic_dirty_edit_and_terminal() {
        for text in ["", "é", "é\n", "a\r\nb\n"] {
            let mut buffers = buffers(text);
            let buffer = buffers.selected_mut().unwrap();
            let version = buffer.version();
            let mut proposal = SourceEditProposal::new(
                ProposalId(1),
                buffer,
                vec![TextEdit::insert(Position::new(0, 0), "🦀\n")],
                None,
            )
            .unwrap();
            let candidate = proposal.preview(buffer).unwrap();
            let display = proposal.unified_diff_bounded(buffer, 29).unwrap();
            assert!(display.text.len() <= 29);
            assert!(display.truncated);
            assert_eq!(proposal.preview(buffer).unwrap(), candidate);
            proposal.accept(buffer).unwrap();
            assert_eq!(buffer.text(), candidate);
            assert_eq!(buffer.version(), version + 1);
            assert!(buffer.is_dirty());
            assert_eq!(proposal.status(), ProposalStatus::Applied);
            assert_eq!(proposal.to_json()["status"], "applied");
            assert_eq!(
                proposal.accept(buffer).unwrap_err()[0],
                ProposalError::Applied
            );
            proposal.reject();
            proposal.discard();
            assert_eq!(proposal.status(), ProposalStatus::Applied);
            assert_eq!(buffer.version(), version + 1);
        }
    }

    #[test]
    fn acceptance_failures_leave_all_state_unchanged() {
        let mut buffers = buffers("éabc");
        for edits in [
            vec![replacement(0, 2, "valid"), replacement(1, 2, "invalid")],
            vec![replacement(0, 3, "x"), replacement(2, 4, "y")],
        ] {
            let buffer = buffers.selected_mut().unwrap();
            let mut proposal = SourceEditProposal::new(ProposalId(1), buffer, edits, None).unwrap();
            let before = (
                buffer.text().to_owned(),
                buffer.version(),
                buffer.is_dirty(),
            );
            let payload = proposal.to_json();
            assert!(proposal.accept(buffer).is_err());
            assert_eq!(
                (
                    buffer.text().to_owned(),
                    buffer.version(),
                    buffer.is_dirty()
                ),
                before
            );
            assert_eq!(proposal.to_json(), payload);
        }
        let mut proposal = SourceEditProposal::new(
            ProposalId(2),
            buffers.selected().unwrap(),
            vec![replacement(0, 2, "x")],
            None,
        )
        .unwrap();
        proposal.unified_diff(buffers.selected().unwrap()).unwrap();
        let buffer = buffers.selected_mut().unwrap();
        buffer.apply_edit(replacement(2, 3, "z")).unwrap();
        let before = (
            buffer.text().to_owned(),
            buffer.version(),
            buffer.is_dirty(),
        );
        assert!(matches!(
            proposal.accept(buffer).unwrap_err()[0],
            ProposalError::RevisionConflict { .. }
        ));
        assert_eq!(
            (
                buffer.text().to_owned(),
                buffer.version(),
                buffer.is_dirty()
            ),
            before
        );
        assert_eq!(proposal.status(), ProposalStatus::Pending);
        buffers.create("test", "other");
        let buffer = buffers.selected_mut().unwrap();
        let dirty = buffer.is_dirty();
        assert!(
            proposal
                .accept(buffer)
                .unwrap_err()
                .iter()
                .any(|e| matches!(e, ProposalError::WrongBuffer { .. }))
        );
        assert_eq!(buffer.text(), "other");
        assert_eq!(buffer.is_dirty(), dirty);
    }

    #[test]
    fn rejection_and_discard_are_terminal_and_inert() {
        let mut buffers = buffers("abc");
        let buffer = buffers.selected_mut().unwrap();
        for rejected in [true, false] {
            let mut proposal =
                SourceEditProposal::new(ProposalId(1), buffer, vec![replacement(0, 1, "x")], None)
                    .unwrap();
            let version = buffer.version();
            let dirty = buffer.is_dirty();
            if rejected {
                proposal.reject();
            } else {
                proposal.discard();
            }
            let status = proposal.status();
            proposal.reject();
            proposal.discard();
            assert_eq!(proposal.status(), status);
            assert!(proposal.accept(buffer).is_err());
            assert!(proposal.unified_diff(buffer).is_err());
            assert_eq!(buffer.text(), "abc");
            assert_eq!(buffer.version(), version);
            assert_eq!(buffer.is_dirty(), dirty);
        }
    }

    #[test]
    fn acceptance_never_saves_or_changes_destination_baseline() {
        let fixture = crate::test_support::TempWorkspace::new("source-acceptance");
        fixture.write_file("a.txt", "original\n");
        let mut buffers = ScratchBuffers::default();
        buffers
            .open(
                &fixture.workspace(),
                &crate::language::LanguageRegistry::builtins(),
                "a.txt",
            )
            .unwrap();
        let buffer = buffers.selected_mut().unwrap();
        let baseline = buffer.destination_revision().unwrap().to_owned();
        let destination = buffer.destination_path().unwrap().clone();
        let mut proposal = SourceEditProposal::new(
            ProposalId(1),
            buffer,
            vec![replacement(0, 8, "candidate")],
            None,
        )
        .unwrap();
        proposal.accept(buffer).unwrap();
        assert_eq!(buffer.text(), "candidate\n");
        assert!(buffer.is_dirty());
        assert_eq!(buffer.destination_revision(), Some(baseline.as_str()));
        assert_eq!(buffer.destination_path(), Some(&destination));
        assert_eq!(fixture.read_file("a.txt"), "original\n");
    }

    #[test]
    fn payload_is_inert_and_empty_proposals_are_malformed() {
        let buffers = buffers("");
        let buffer = buffers.selected().unwrap();
        assert_eq!(
            SourceEditProposal::new(ProposalId(1), buffer, vec![], None),
            Err(ProposalError::EmptyEdits)
        );
        let payload = "execute('/tmp/untrusted');\r\n";
        let proposal = SourceEditProposal::new(
            ProposalId(2),
            buffer,
            vec![TextEdit::insert(Position::new(0, 0), payload)],
            Some("save /tmp/untrusted".into()),
        )
        .unwrap();
        assert_eq!(proposal.preview(buffer).unwrap(), payload);
        assert_eq!(buffer.text(), "");
        assert!(buffer.destination_path().is_none());
    }
}
