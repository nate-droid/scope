//! Headless full-replacement review. All source access uses workspace capabilities.

use super::{DocumentError, DocumentSession, DocumentSnapshot, Revision};
use crate::workspace::WorkspaceError;
use scope_language::Diagnostic;
use std::sync::Arc;

/// Immutable replacement derived from one accepted session snapshot.
/// A bridge can pass validated structured operation output to `DocumentSession::propose`;
/// constructing a proposal grants no save authority.
#[derive(Debug, Clone)]
pub struct EditProposal {
    base: DocumentSnapshot,
    candidate: DocumentSnapshot,
    provenance: String,
}

impl EditProposal {
    /// Source snapshot against which the replacement was generated.
    pub fn base(&self) -> &DocumentSnapshot {
        &self.base
    }
    /// Exact candidate text, path, identity, and unique opaque revision.
    pub fn candidate(&self) -> &DocumentSnapshot {
        &self.candidate
    }
    /// Language-neutral origin information supplied by the caller.
    pub fn provenance(&self) -> &str {
        &self.provenance
    }
}

/// Immutable review of one candidate, including caller-supplied diagnostics.
/// Diagnostics describe the candidate, never the base. The caller is responsible
/// for computing them from `EditProposal::candidate`; this API does not run analysis.
#[derive(Debug, Clone)]
pub struct EditReview {
    proposal: EditProposal,
    diagnostics: Vec<Diagnostic>,
    diff: String,
}
impl EditReview {
    /// The exact proposal reviewed.
    pub fn proposal(&self) -> &EditProposal {
        &self.proposal
    }
    /// Diagnostics retained with this exact candidate revision.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    /// Human-readable full-replacement diff (not a patch format).
    pub fn diff(&self) -> &str {
        &self.diff
    }
}

/// Explicit host/human decision. Model operation output must not choose this decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditDecision {
    /// Permit applying this exact reviewed candidate.
    Allow,
    /// Refuse applying this exact reviewed candidate.
    Deny,
}

/// Unforgeable decision receipt bound to one reviewed candidate.
#[derive(Debug, Clone)]
pub struct EditAuthorization {
    candidate: Revision,
    decision: EditDecision,
}

/// Typed failures in the reviewable editing lifecycle.
#[derive(Debug)]
pub enum EditError {
    /// Source is stale, unavailable, closed, or belongs to another session.
    Document(DocumentError),
    /// Authorization was issued for a different candidate.
    WrongAuthorization,
    /// The explicit decision was Deny.
    Denied,
    /// Checked capability save failed; the candidate was not accepted by the session.
    Save(WorkspaceError),
}
impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Document(e) => e.fmt(f),
            Self::WrongAuthorization => f.write_str("authorization belongs to another candidate"),
            Self::Denied => f.write_str("candidate was denied"),
            Self::Save(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Document(e) => Some(e),
            Self::Save(e) => Some(e),
            _ => None,
        }
    }
}
impl From<DocumentError> for EditError {
    fn from(error: DocumentError) -> Self {
        Self::Document(error)
    }
}

impl DocumentSession {
    /// Propose a full replacement at an expected source revision without writing.
    /// Accepts language-neutral text and provenance, including validated structured
    /// operation results supplied by a separate bridge. Each call gets a fresh candidate.
    pub fn propose(
        &mut self,
        expected: &Revision,
        replacement: impl Into<String>,
        provenance: impl Into<String>,
    ) -> Result<EditProposal, EditError> {
        let base = self.snapshot_at(expected)?;
        let candidate = DocumentSnapshot {
            document_id: base.document_id,
            path: base.path.clone(),
            revision: Revision(Arc::new(())),
            text: replacement.into(),
        };
        Ok(EditProposal {
            base,
            candidate,
            provenance: provenance.into(),
        })
    }

    /// Review an exact candidate without writing. Diagnostics are supplied for this
    /// candidate by the host; they and provenance remain attached to the review.
    pub fn review(
        &mut self,
        proposal: &EditProposal,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<EditReview, EditError> {
        self.snapshot_at(&proposal.base.revision)?;
        let mut diff = format!(
            "--- {} (base)\n+++ {} (candidate)\n",
            proposal.base.path, proposal.candidate.path
        );
        if proposal.base.text == proposal.candidate.text {
            diff.push_str("(no changes)\n");
        } else {
            for (prefix, text) in [("-", &proposal.base.text), ("+", &proposal.candidate.text)] {
                for line in text.split_inclusive('\n') {
                    diff.push_str(prefix);
                    diff.push_str(line);
                    if !line.ends_with('\n') {
                        diff.push_str("\n\\ No newline at end of file\n");
                    }
                }
            }
        }
        Ok(EditReview {
            proposal: proposal.clone(),
            diagnostics,
            diff,
        })
    }

    /// Record an explicit host/human decision for the reviewed candidate. This is
    /// not a model tool, authentication system, or durable permission grant.
    pub fn authorize(
        &mut self,
        review: &EditReview,
        decision: EditDecision,
    ) -> Result<EditAuthorization, EditError> {
        self.snapshot_at(&review.proposal.base.revision)?;
        Ok(EditAuthorization {
            candidate: review.proposal.candidate.revision.clone(),
            decision,
        })
    }

    /// Apply only the exact authorized candidate, returning its accepted snapshot.
    /// Rechecks the source and uses `Workspace::save_file_checked`: confinement and
    /// best-effort conflict detection, NOT atomic compare-and-swap. External writers
    /// can race the check/write; save failures may have filesystem side effects.
    /// Failed/denied applies do not accept candidate state; observational source
    /// invalidation by `snapshot_at` remains possible. Receipts can be retried after
    /// a save failure while the base remains current. Retain the review for metadata.
    pub fn apply(
        &mut self,
        review: &EditReview,
        authorization: &EditAuthorization,
    ) -> Result<DocumentSnapshot, EditError> {
        if authorization.candidate != review.proposal.candidate.revision {
            return Err(EditError::WrongAuthorization);
        }
        if authorization.decision == EditDecision::Deny {
            return Err(EditError::Denied);
        }
        self.snapshot_at(&review.proposal.base.revision)?;
        let candidate = &review.proposal.candidate;
        let disk_revision = self
            .workspace
            .as_ref()
            .ok_or(DocumentError::Closed)?
            .save_file_checked(&candidate.path, &candidate.text, &self.disk_revision)
            .map_err(EditError::Save)?;
        self.disk_revision = disk_revision;
        self.snapshot = Some(candidate.clone());
        Ok(candidate.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::DocumentId, test_support::TempWorkspace, workspace::Workspace};

    fn session(f: &TempWorkspace, readonly: bool) -> DocumentSession {
        let workspace = if readonly {
            Workspace::read_only(f.root().to_string_lossy()).unwrap()
        } else {
            f.workspace()
        };
        DocumentSession::open(workspace, DocumentId::new(1), "example.txt").unwrap()
    }
    fn review(s: &mut DocumentSession, text: &str) -> EditReview {
        let base = s.snapshot().unwrap();
        let proposal = s
            .propose(&base.revision, text, "structured operation")
            .unwrap();
        s.review(&proposal, vec![]).unwrap()
    }

    #[test]
    fn neutral_review_deny_and_authorized_apply() {
        let f = TempWorkspace::new("editing-apply");
        f.write_file("example.txt", "before\n");
        let mut s = session(&f, false);
        let base = s.snapshot().unwrap();
        let r = review(&mut s, "after 🦀\n");
        assert!(r.diff().contains("-before\n+after 🦀\n"));
        assert_eq!(r.proposal().provenance(), "structured operation");
        assert_eq!(f.read_file("example.txt"), base.text);
        assert_eq!(s.snapshot().unwrap(), base);
        let denied = s.authorize(&r, EditDecision::Deny).unwrap();
        assert!(matches!(s.apply(&r, &denied), Err(EditError::Denied)));
        assert_eq!(s.snapshot().unwrap(), base);
        assert_eq!(f.read_file("example.txt"), base.text);
        let allowed = s.authorize(&r, EditDecision::Allow).unwrap();
        let applied = s.apply(&r, &allowed).unwrap();
        assert_eq!(&applied, r.proposal().candidate());
        assert_eq!(s.snapshot().unwrap(), applied);
        assert_eq!(f.read_file("example.txt"), applied.text);
        assert!(matches!(
            s.apply(&r, &allowed),
            Err(EditError::Document(DocumentError::StaleRevision))
        ));
    }

    #[test]
    fn candidate_bound_diagnostics_and_foreign_receipts() {
        let f = TempWorkspace::new("editing-binding");
        f.write_file("example.txt", "base");
        let mut s = session(&f, false);
        let base = s.snapshot().unwrap();
        let p = s.propose(&base.revision, "candidate", "origin").unwrap();
        let diagnostic = Diagnostic::warning(
            scope_language::TextRange::new(
                scope_language::Position::new(0, 0),
                scope_language::Position::new(0, 1),
            ),
            "candidate warning",
        );
        let r = s.review(&p, vec![diagnostic.clone()]).unwrap();
        assert_eq!(r.diagnostics(), &[diagnostic]);
        assert_eq!(r.proposal().candidate().revision, p.candidate().revision);
        let other = review(&mut s, "candidate");
        assert_ne!(
            other.proposal().candidate().revision,
            p.candidate().revision
        );
        let auth = s.authorize(&r, EditDecision::Allow).unwrap();
        assert!(matches!(
            s.apply(&other, &auth),
            Err(EditError::WrongAuthorization)
        ));
        let mut foreign = session(&f, false);
        assert!(foreign.propose(&base.revision, "x", "x").is_err());
        assert!(foreign.review(&p, vec![]).is_err());
        assert!(foreign.authorize(&r, EditDecision::Allow).is_err());
        assert!(foreign.apply(&r, &auth).is_err());
        assert_eq!(s.snapshot().unwrap(), base);
        assert_eq!(f.read_file("example.txt"), "base");
    }

    #[test]
    fn applying_one_candidate_invalidates_competing_proposals() {
        let f = TempWorkspace::new("editing-competing");
        f.write_file("example.txt", "base");
        let mut s = session(&f, false);
        let first = review(&mut s, "first");
        let second = review(&mut s, "second");
        let first_auth = s.authorize(&first, EditDecision::Allow).unwrap();
        let second_auth = s.authorize(&second, EditDecision::Allow).unwrap();
        let accepted = s.apply(&first, &first_auth).unwrap();

        assert!(matches!(
            s.apply(&second, &second_auth),
            Err(EditError::Document(DocumentError::StaleRevision))
        ));
        assert_eq!(s.snapshot().unwrap(), accepted);
        assert_eq!(f.read_file("example.txt"), "first");
    }

    #[test]
    fn conflict_requires_reload_and_fresh_proposal() {
        let f = TempWorkspace::new("editing-conflict-retry");
        f.write_file("example.txt", "base");
        let mut s = session(&f, false);
        let old = review(&mut s, "old candidate");
        let old_auth = s.authorize(&old, EditDecision::Allow).unwrap();
        f.write_file("example.txt", "external");

        assert!(matches!(
            s.apply(&old, &old_auth),
            Err(EditError::Document(DocumentError::StaleRevision))
        ));
        assert_eq!(f.read_file("example.txt"), "external");
        let reloaded = s.reload().unwrap();
        assert!(matches!(
            s.propose(&old.proposal().base().revision, "retry", "retry"),
            Err(EditError::Document(DocumentError::StaleRevision))
        ));
        assert_eq!(s.snapshot().unwrap(), reloaded);

        let fresh = review(&mut s, "fresh candidate");
        let fresh_auth = s.authorize(&fresh, EditDecision::Allow).unwrap();
        let accepted = s.apply(&fresh, &fresh_auth).unwrap();
        assert_eq!(s.snapshot().unwrap(), accepted);
        assert_eq!(f.read_file("example.txt"), "fresh candidate");
    }

    #[test]
    fn replacement_diff_preserves_empty_unicode_and_line_endings() {
        let f = TempWorkspace::new("editing-diff");
        for (base, candidate, body) in [
            ("", "", "(no changes)\n"),
            ("", "🦀", "+🦀\n\\ No newline at end of file\n"),
            ("gone\n", "", "-gone\n"),
            ("before\r\n", "after\r\n", "-before\r\n+after\r\n"),
            ("same", "same", "(no changes)\n"),
        ] {
            f.write_file("example.txt", base);
            let mut s = session(&f, false);
            let snapshot = s.snapshot().unwrap();
            let r = review(&mut s, candidate);
            assert_eq!(
                r.diff(),
                format!("--- example.txt (base)\n+++ example.txt (candidate)\n{body}")
            );
            assert_eq!(s.snapshot().unwrap(), snapshot);
            assert_eq!(f.read_file("example.txt"), base);
        }
    }

    #[test]
    fn stale_source_and_readonly_retry() {
        let f = TempWorkspace::new("editing-stale");
        f.write_file("example.txt", "base");
        let mut s = session(&f, false);
        let r = review(&mut s, "replacement");
        let auth = s.authorize(&r, EditDecision::Allow).unwrap();
        f.write_file("example.txt", "external");
        assert!(s.review(r.proposal(), vec![]).is_err());
        assert!(s.authorize(&r, EditDecision::Allow).is_err());
        assert!(s.apply(&r, &auth).is_err());
        assert_eq!(f.read_file("example.txt"), "external");
        s.reload().unwrap();
        assert!(s.apply(&r, &auth).is_err());
        let mut ro = session(&f, true);
        let base = ro.snapshot().unwrap();
        let r = review(&mut ro, "replacement");
        let auth = ro.authorize(&r, EditDecision::Allow).unwrap();
        for _ in 0..2 {
            assert!(matches!(ro.apply(&r, &auth), Err(EditError::Save(_))));
            assert_eq!(ro.snapshot().unwrap(), base);
            assert_eq!(f.read_file("example.txt"), "external");
        }
        ro.close();
        assert!(ro.apply(&r, &auth).is_err());
    }
}
