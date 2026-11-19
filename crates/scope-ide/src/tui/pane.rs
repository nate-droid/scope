use crossterm::event::KeyEvent;
use ratatui::layout::Rect;

use super::{DefaultProvider, TrustedClosureProvider};
use crate::app::AppState;

/// Immutable editor evidence, not a committed source or execution capability.
/// Equality includes an edit identity: even undoing to identical text invalidates old evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorDraftSnapshot {
    pub baseline: super::ScratchEditSnapshot,
    pub text: String,
    pub edit_identity: u64,
    /// UTF-8 byte cursor and selection anchor in this exact local draft.
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub editor: String,
    pub editor_generation: u64,
    pub selection_identity: u64,
    pub source_selection_identity: u64,
    pub owner_binding_generation: u64,
}

/// Resolution never substitutes another editor for a retained invalid selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorDraftOutcome {
    Available(EditorDraftSnapshot),
    NoMatchingEditor,
    Ambiguous,
    Conflict,
}

/// Host freshness, independent of the downstream mathematical result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckCurrentness {
    Current,
    Historical,
    NoEvidence,
    ValidationUnavailable,
}

/// A host-produced presentation-time validation, not a reusable authorization.
/// Only Scope can populate this view. Retain feedback with the invocation token
/// obtained in `AnalysisContext::invocation_id`, never with draft text alone.
#[derive(Clone, Debug)]
pub struct CheckPresentation {
    pub(super) invocation: Option<crate::analysis::CheckInvocationId>,
    pub(super) status: CheckCurrentness,
}

impl CheckPresentation {
    /// Status of this exact feedback invocation. Superseded feedback is historical.
    pub fn for_invocation(
        &self,
        invocation: &crate::analysis::CheckInvocationId,
    ) -> CheckCurrentness {
        if self.invocation.as_ref() == Some(invocation) {
            self.status
        } else {
            CheckCurrentness::Historical
        }
    }

    /// Status of the host's latest evidence (not of arbitrary pane feedback).
    pub fn status(&self) -> CheckCurrentness {
        self.status
    }
}

/// Read-only context exposed to TUI panes while rendering.
pub struct TuiContext<'a, P: TrustedClosureProvider = DefaultProvider> {
    app: &'a AppState<P>,
    sources: Option<&'a crate::scratch::ScratchBuffers>,
    focused: bool,
    scroll: u16,
    editor_drafts: bool,
    app_editor: bool,
    current_draft: EditorDraftOutcome,
    check: CheckPresentation,
}

impl<'a, P: TrustedClosureProvider> TuiContext<'a, P> {
    pub fn new(app: &'a AppState<P>) -> Self {
        Self {
            app,
            check: CheckPresentation {
                invocation: None,
                status: CheckCurrentness::ValidationUnavailable,
            },
            sources: None,
            focused: false,
            scroll: 0,
            editor_drafts: false,
            app_editor: false,
            current_draft: EditorDraftOutcome::NoMatchingEditor,
        }
    }

    pub(super) fn with_view(app: &'a AppState<P>, focused: bool, scroll: u16) -> Self {
        Self::with_view_sources(app, focused, scroll, None)
    }

    pub(super) fn with_view_sources(
        app: &'a AppState<P>,
        focused: bool,
        scroll: u16,
        sources: Option<&'a crate::scratch::ScratchBuffers>,
    ) -> Self {
        Self {
            app,
            check: CheckPresentation {
                invocation: None,
                status: CheckCurrentness::ValidationUnavailable,
            },
            sources,
            focused,
            scroll,
            editor_drafts: false,
            app_editor: false,
            current_draft: EditorDraftOutcome::NoMatchingEditor,
        }
    }

    /// Host validation for this presentation only; do not cache a Current status.
    pub fn check_presentation(&self) -> &CheckPresentation {
        &self.check
    }

    pub(super) fn with_check_presentation(mut self, check: CheckPresentation) -> Self {
        self.check = check;
        self
    }

    /// Available selected editor evidence. Compare the entire value before reusing results.
    /// Use `current_draft_outcome` to distinguish missing, ambiguous and conflicting editors.
    pub fn current_draft(&self) -> Option<&EditorDraftSnapshot> {
        match &self.current_draft {
            EditorDraftOutcome::Available(draft) => Some(draft),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn with_current_draft(self, draft: Option<EditorDraftSnapshot>) -> Self {
        self.with_draft_outcome(draft.map_or(
            EditorDraftOutcome::NoMatchingEditor,
            EditorDraftOutcome::Available,
        ))
    }

    pub fn current_draft_outcome(&self) -> &EditorDraftOutcome {
        &self.current_draft
    }

    pub(super) fn with_draft_outcome(mut self, outcome: EditorDraftOutcome) -> Self {
        self.current_draft = outcome;
        self
    }

    /// Resolves the supplied owner's binding to a read-only source snapshot.
    /// `None` means no owner was supplied, not that its binding is unbound.
    pub fn bound_semantic_source(&self) -> Option<crate::scratch::BoundSource> {
        self.sources
            .map(crate::scratch::ScratchBuffers::bound_source)
    }

    /// Read-only selected scratch, independent of the semantic target.
    /// Returns `None` when no owner or no selection is supplied.
    pub fn selected_source(&self) -> Option<super::ScratchEditSnapshot> {
        let buffer = self.sources?.selected().ok()?;
        Some(super::ScratchEditSnapshot {
            scratch_id: buffer.id(),
            source: scope_language::SourceSnapshot {
                source_id: buffer.evaluation_source_id().clone(),
                revision_id: scope_language::RevisionId::from_version(buffer.version()),
                text: buffer.text().into(),
            },
            version: buffer.version(),
        })
    }

    pub(super) fn with_app_editor(mut self, enabled: bool) -> Self {
        self.app_editor = enabled;
        self
    }

    /// Whether the workspace editor targets the App document rather than scratch.
    /// This is presentation state only; editable-pane opt-in still controls input.
    pub fn is_app_editor(&self) -> bool {
        self.app_editor
    }

    /// Current editor target, without granting publication authority.
    /// `selected_source` remains the independent scratch selection.
    pub(super) fn editor_source(&self) -> Option<super::ScratchEditSnapshot> {
        if self.app_editor {
            self.app
                .open_document()
                .map(super::ScratchEditAccess::document_snapshot)
        } else {
            self.selected_source()
        }
    }

    /// Presentation metadata from the existing selected buffer; no disk read.
    pub fn selected_document_status(&self) -> Option<(String, bool)> {
        let buffer = self.sources?.selected().ok()?;
        Some((
            buffer
                .destination_path()
                .map(|path| path.to_string())
                .unwrap_or_else(|| "Untitled document".into()),
            buffer.is_dirty(),
        ))
    }

    /// True if any editor draft would block execution, including parked drafts.
    pub fn has_editor_drafts(&self) -> bool {
        self.editor_drafts
    }

    pub(super) fn with_editor_drafts(mut self, dirty: bool) -> Self {
        self.editor_drafts = dirty;
        self
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    pub fn app(&self) -> &'a AppState<P> {
        self.app
    }
}

/// Mutable context exposed to TUI panes while handling input.
pub struct TuiContextMut<'a, P: TrustedClosureProvider = DefaultProvider> {
    app: &'a mut AppState<P>,
    scroll: Option<&'a mut u16>,
    viewport: Rect,
}

impl<'a, P: TrustedClosureProvider> TuiContextMut<'a, P> {
    pub fn new(app: &'a mut AppState<P>) -> Self {
        Self {
            app,
            scroll: None,
            viewport: Rect::default(),
        }
    }

    pub(super) fn with_scroll(
        app: &'a mut AppState<P>,
        scroll: &'a mut u16,
        viewport: Rect,
    ) -> Self {
        Self {
            app,
            scroll: Some(scroll),
            viewport,
        }
    }

    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    pub fn scroll(&self) -> u16 {
        self.scroll.as_deref().copied().unwrap_or(0)
    }

    pub fn set_scroll(&mut self, value: u16) {
        if let Some(scroll) = self.scroll.as_deref_mut() {
            *scroll = value;
        }
    }

    pub fn app(&self) -> &AppState<P> {
        self.app
    }

    pub fn app_mut(&mut self) -> &mut AppState<P> {
        self.app
    }
}

/// A composable TUI surface.
///
/// Downstream consumers can implement this trait to add project-specific views
/// while reusing Scope's terminal runtime, application state, and default panes.
pub trait Pane<P: TrustedClosureProvider = DefaultProvider> {
    /// Read-only local editor snapshot for an exact baseline; no publication authority.
    fn editor_draft(&self, _baseline: &super::ScratchEditSnapshot) -> Option<EditorDraftSnapshot> {
        None
    }

    /// Owner-dispatched local replacement. Implementations must recheck local
    /// identity, record one ordinary undo transaction, and never publish source.
    /// Callers should use `WorkspaceTui::apply_reviewed_editor_draft`, which also
    /// validates editor generation, source selection and owner binding.
    fn apply_editor_draft(
        &mut self,
        _pinned: &EditorDraftSnapshot,
        _replacement: &str,
        _app_document: bool,
    ) -> Result<super::DraftApplyOutcome, String> {
        Err("selected editor does not support reviewed local edits".into())
    }

    /// Owner-validated cursor-only placement. Unsupported panes deny by default.
    fn place_editor_cursor(
        &mut self,
        _pinned: &EditorDraftSnapshot,
        _cursor: usize,
    ) -> Result<(), String> {
        Err("selected editor does not support source navigation".into())
    }

    /// Whether execution would overlook an uncommitted local source draft.
    fn has_dirty_draft(&self) -> bool {
        false
    }

    /// Whether a retained unpublished draft belongs to an `AppState<P>` document.
    fn has_dirty_app_draft(&self) -> bool {
        false
    }

    #[cfg(feature = "desktop")]
    fn has_dirty_source(&self, _source: &scope_language::SourceId) -> bool {
        self.has_dirty_app_draft()
    }

    #[cfg(feature = "desktop")]
    fn forget_closed_source(&mut self, _source: &scope_language::SourceId) {}

    /// Scratch IDs and source identity/revision descriptions for dirty drafts.
    /// Metadata may be unavailable for custom panes even when `has_dirty_draft`
    /// returns `true`; an empty list does not imply that execution is safe.
    fn dirty_draft_sources(&self) -> Vec<(u64, String)> {
        Vec::new()
    }

    fn id(&self) -> &'static str;

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_, P>);

    /// Requests that the host open a literal workspace-relative document path.
    /// Called only while focused, after global shortcuts and before pane input.
    /// The default declines. This is not a command: the host applies ordinary
    /// document-open authorization and draft preservation, then focuses the editor.
    fn workspace_document_request(
        &mut self,
        _event: &crossterm::event::Event,
        _ctx: &TuiContext<'_, P>,
    ) -> Option<String> {
        None
    }

    /// Receives focused source input only after explicit `register_editable` opt-in.
    /// This capability exposes no application, execution, or filesystem access.
    fn handle_source_event(
        &mut self,
        _event: crossterm::event::Event,
        _access: &mut super::ScratchEditAccess<'_>,
    ) -> bool {
        false
    }

    /// Receives focused input with read-only presentation context, before the
    /// legacy mutable key callback. Hosts own authorization and the meaning of
    /// operations; this hook grants no editing, execution, or filesystem access.
    /// Paste remains an event, never a command string.
    fn handle_view_event(
        &mut self,
        _event: crossterm::event::Event,
        _ctx: &TuiContext<'_, P>,
    ) -> bool {
        false
    }

    /// Handle a pressed key while focused. Return `true` when consumed.
    fn handle_key(&mut self, _key: KeyEvent, _ctx: &mut TuiContextMut<'_, P>) -> bool {
        false
    }
}
