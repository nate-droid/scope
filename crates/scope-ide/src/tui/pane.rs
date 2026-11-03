use crossterm::event::KeyEvent;
use ratatui::layout::Rect;

use crate::app::App;

/// Read-only context exposed to TUI panes while rendering.
pub struct TuiContext<'a> {
    app: &'a App,
    sources: Option<&'a crate::scratch::ScratchBuffers>,
    focused: bool,
    scroll: u16,
    editor_drafts: bool,
    app_editor: bool,
}

impl<'a> TuiContext<'a> {
    pub fn new(app: &'a App) -> Self {
        Self {
            app,
            sources: None,
            focused: false,
            scroll: 0,
            editor_drafts: false,
            app_editor: false,
        }
    }

    pub(super) fn with_view(app: &'a App, focused: bool, scroll: u16) -> Self {
        Self::with_view_sources(app, focused, scroll, None)
    }

    pub(super) fn with_view_sources(
        app: &'a App,
        focused: bool,
        scroll: u16,
        sources: Option<&'a crate::scratch::ScratchBuffers>,
    ) -> Self {
        Self {
            app,
            sources,
            focused,
            scroll,
            editor_drafts: false,
            app_editor: false,
        }
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

    /// Whether this editable pane targets App rather than the selected scratch.
    pub(super) fn is_app_editor(&self) -> bool {
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

    pub fn app(&self) -> &'a App {
        self.app
    }
}

/// Mutable context exposed to TUI panes while handling input.
pub struct TuiContextMut<'a> {
    app: &'a mut App,
    scroll: Option<&'a mut u16>,
    viewport: Rect,
}

impl<'a> TuiContextMut<'a> {
    pub fn new(app: &'a mut App) -> Self {
        Self {
            app,
            scroll: None,
            viewport: Rect::default(),
        }
    }

    pub(super) fn with_scroll(app: &'a mut App, scroll: &'a mut u16, viewport: Rect) -> Self {
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

    pub fn app(&self) -> &App {
        self.app
    }

    pub fn app_mut(&mut self) -> &mut App {
        self.app
    }
}

/// A composable TUI surface.
///
/// Downstream consumers can implement this trait to add project-specific views
/// while reusing Scope's terminal runtime, application state, and default panes.
pub trait Pane {
    /// Whether execution would overlook an uncommitted local source draft.
    fn has_dirty_draft(&self) -> bool {
        false
    }

    /// Whether a retained unpublished draft belongs to an App document.
    fn has_dirty_app_draft(&self) -> bool {
        false
    }

    /// Scratch IDs and source identity/revision descriptions for dirty drafts.
    /// Metadata may be unavailable for custom panes even when `has_dirty_draft`
    /// returns `true`; an empty list does not imply that execution is safe.
    fn dirty_draft_sources(&self) -> Vec<(u64, String)> {
        Vec::new()
    }

    fn id(&self) -> &'static str;

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>);

    /// Requests that the host open a literal workspace-relative document path.
    /// Called only while focused, after global shortcuts and before pane input.
    /// The default declines. This is not a command: the host applies ordinary
    /// document-open authorization and draft preservation, then focuses the editor.
    fn workspace_document_request(
        &mut self,
        _event: &crossterm::event::Event,
        _ctx: &TuiContext<'_>,
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
        _ctx: &TuiContext<'_>,
    ) -> bool {
        false
    }

    /// Handle a pressed key while focused. Return `true` when consumed.
    fn handle_key(&mut self, _key: KeyEvent, _ctx: &mut TuiContextMut<'_>) -> bool {
        false
    }
}
