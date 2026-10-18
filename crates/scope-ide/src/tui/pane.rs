use crossterm::event::KeyEvent;
use ratatui::layout::Rect;

use crate::app::App;

/// Read-only context exposed to TUI panes while rendering.
pub struct TuiContext<'a> {
    app: &'a App,
    focused: bool,
    scroll: u16,
}

impl<'a> TuiContext<'a> {
    pub fn new(app: &'a App) -> Self {
        Self {
            app,
            focused: false,
            scroll: 0,
        }
    }

    pub(super) fn with_view(app: &'a App, focused: bool, scroll: u16) -> Self {
        Self {
            app,
            focused,
            scroll,
        }
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
    fn id(&self) -> &'static str;

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>);

    /// Handle a pressed key while focused. Return `true` when consumed.
    fn handle_key(&mut self, _key: KeyEvent, _ctx: &mut TuiContextMut<'_>) -> bool {
        false
    }
}
