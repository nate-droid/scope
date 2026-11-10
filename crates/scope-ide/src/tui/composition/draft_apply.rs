#[cfg(test)]
#[path = "draft_apply_tests.rs"]
mod tests;

use super::*;
use crate::{
    scratch::ScratchBuffers,
    tui::{DraftApplyDecision, DraftApplyOutcome, EditorDraftSnapshot},
};

impl<P: TrustedClosureProvider> WorkspaceTui<P> {
    /// Apply an explicitly reviewed whole-document replacement to the unpublished
    /// selected editor only. Full snapshot equality includes owner/baseline,
    /// editor generation, selection, local edit identity, cursor and anchor.
    /// Validation and the editor's single undo transaction share exclusive owner
    /// access; no source publication, disk write, execution or approval occurs.
    /// Cancel and exact no-ops do not change local history or edit identity.
    pub fn apply_reviewed_editor_draft(
        &mut self,
        app: &AppState<P>,
        sources: Option<&ScratchBuffers>,
        pinned: &EditorDraftSnapshot,
        replacement: &str,
        decision: DraftApplyDecision,
    ) -> Result<DraftApplyOutcome, String> {
        if decision == DraftApplyDecision::Cancel {
            return Ok(DraftApplyOutcome::Cancelled);
        }
        self.validate_editor_draft(app, sources, pinned)
            .map_err(|_| {
                "Stale proposal: selected editor draft changed; preview again".to_string()
            })?;
        if replacement == pinned.text {
            return Ok(DraftApplyOutcome::Unchanged);
        }
        let instance = self
            .instances
            .get_mut(&pinned.editor)
            .ok_or("Stale proposal: editor no longer exists")?;
        instance
            .pane
            .apply_editor_draft(pinned, replacement, self.app_editor)
    }
}
