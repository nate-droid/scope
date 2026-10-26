use crossterm::event::KeyEvent;
use ratatui::layout::Rect;

use crate::app::App;

/// Read-only context exposed to TUI panes while rendering.
pub struct TuiContext<'a> {
    app: &'a App,
    sources: Option<&'a crate::scratch::ScratchBuffers>,
    focused: bool,
    scroll: u16,
}

impl<'a> TuiContext<'a> {
    pub fn new(app: &'a App) -> Self {
        Self {
            app,
            sources: None,
            focused: false,
            scroll: 0,
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

    /// Scratch IDs and source identity/revision descriptions for dirty drafts.
    /// Metadata may be unavailable for custom panes even when `has_dirty_draft`
    /// returns `true`; an empty list does not imply that execution is safe.
    fn dirty_draft_sources(&self) -> Vec<(u64, String)> {
        Vec::new()
    }

    fn id(&self) -> &'static str;

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>);

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
