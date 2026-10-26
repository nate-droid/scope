use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

mod composition;
pub use composition::{
    ActionInfo, DownstreamAction, View, WorkspaceAction, WorkspaceInput, WorkspaceTui,
};
mod pane;
mod panes;
mod quick_open;
pub mod scratch_editor;
pub use scratch_editor::{
    EditableScratchPane, ScratchEditAccess, ScratchEditError, ScratchEditSnapshot,
};
mod semantic;
mod session;
#[cfg(feature = "tools")]
mod workbench;
#[cfg(feature = "tools")]
pub use workbench::WorkbenchPane;

pub use pane::{Pane, TuiContext, TuiContextMut};
pub use panes::{DocumentPane, FilesPane, LanguagePane};

use session::TerminalSession;

const HELP_TEXT: &str = "Tab/Shift-Tab focus • Ctrl-P quick-open • F8 semantic inspection • ↑/↓ move/scroll • PgUp/PgDn scroll • Enter open • r refresh • q/Esc quit";

/// Run the default Scope TUI.
///
/// This preserves the original convenience entry point while the composable
/// [`Tui`] and [`TuiBuilder`] APIs allow downstream crates to customize panes.
pub fn run(app: App) -> Result<(), Box<dyn std::error::Error>> {
    Tui::default().run(app)
}

/// The result of dispatching a key through Scope's input handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputOutcome {
    /// The event was not a key press or no handler consumed it.
    Ignored,
    /// Scope consumed the event, including input captured by a modal picker.
    Handled,
    /// A quit key was pressed with no modal open. The caller decides whether to exit.
    QuitRequested,
}

pub struct Tui {
    redraw_interval: Duration,
    panes: Vec<Box<dyn Pane>>,
    focused: usize,
    scroll: Vec<u16>,
    viewports: Vec<Rect>,
    quick_open: Option<quick_open::QuickOpen>,
    status: Option<String>,
    semantic_inspection: Option<semantic::SemanticInspection>,
}

impl Tui {
    pub fn builder() -> TuiBuilder {
        TuiBuilder::new()
    }

    pub fn run(mut self, mut app: App) -> Result<(), Box<dyn std::error::Error>> {
        let mut terminal = TerminalSession::enter()?;

        loop {
            terminal.draw(|frame| {
                self.render(frame, &app);
            })?;

            if event::poll(self.redraw_interval)?
                && let Event::Key(key) = event::read()?
                && self.handle_key(key, &mut app) == InputOutcome::QuitRequested
            {
                break;
            }
        }

        Ok(())
    }

    /// Draw the TUI using a terminal owned by the caller.
    ///
    /// This does not enter raw mode, read input, or manage terminal cleanup.
    /// Call again after changing application or pane state. Ratatui diffs the
    /// complete frame, so unchanged cells are not rewritten to the terminal.
    pub fn draw<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut ratatui::Terminal<B>,
        app: &App,
    ) -> std::io::Result<()> {
        terminal.draw(|frame| self.render(frame, app))?;
        Ok(())
    }

    /// Render into a caller-owned frame, updating pane viewport geometry.
    ///
    /// Use this inside `Terminal::draw` when composing Scope with other UI.
    /// Every pane renders on each frame; individual panes should not draw
    /// directly to the terminal or skip unchanged content.
    pub fn render(&mut self, frame: &mut ratatui::Frame<'_>, app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(3),
            ])
            .split(frame.area());

        let title = Paragraph::new(format!("Scope — {}", app.workspace().root()))
            .block(Block::default().borders(Borders::ALL).title("Workspace"));
        frame.render_widget(title, chunks[0]);

        self.render_body(frame, chunks[1], app);

        let help = Paragraph::new(self.status.as_deref().unwrap_or(HELP_TEXT))
            .block(Block::default().borders(Borders::ALL).title("Keys"));
        frame.render_widget(help, chunks[2]);
        if let Some(picker) = &self.quick_open {
            picker.render(frame, frame.area(), app);
        }
        if let Some(inspection) = &self.semantic_inspection {
            inspection.render(frame, frame.area(), app);
        }
    }

    fn render_body(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
        let Some((first, rest)) = self.panes.split_first() else {
            return;
        };

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(area);

        self.viewports[0] = body[0];
        let ctx = TuiContext::with_view(app, self.focused == 0, self.scroll[0]);
        first.render(frame, body[0], &ctx);

        if rest.len() == 1 {
            self.viewports[1] = body[1];
        }
        match rest {
            [] => {}
            [only] => only.render(
                frame,
                body[1],
                &TuiContext::with_view(app, self.focused == 1, self.scroll[1]),
            ),
            panes => {
                let right = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(right_pane_constraints(panes.len()))
                    .split(body[1]);

                for (index, (pane, area)) in panes.iter().zip(right.iter()).enumerate() {
                    let index = index + 1;
                    self.viewports[index] = *area;
                    pane.render(
                        frame,
                        *area,
                        &TuiContext::with_view(app, self.focused == index, self.scroll[index]),
                    );
                }
            }
        }
    }

    /// Dispatch input in a caller-owned event loop.
    ///
    /// Only key presses are handled. Modal input is captured first: `q` becomes
    /// picker text and Escape dismisses the picker. Otherwise `q` and Escape
    /// return [`InputOutcome::QuitRequested`] before pane dispatch; this method
    /// never exits itself. Draw once before dispatching input so scrolling can
    /// use the current pane viewport sizes.
    pub fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> InputOutcome {
        if key.kind != KeyEventKind::Press {
            return InputOutcome::Ignored;
        }
        if let Some(inspection) = &mut self.semantic_inspection {
            if inspection.handle_key(key, app) {
                self.semantic_inspection = None;
            }
            return InputOutcome::Handled;
        }
        if let Some(picker) = &mut self.quick_open {
            let before = app.open_document().map(|document| document.id());
            if picker.handle_key(key, app) {
                self.quick_open = None;
            }
            if before != app.open_document().map(|document| document.id()) {
                self.scroll.fill(0);
                if let Some(index) = self
                    .panes
                    .iter()
                    .position(|pane| pane.id() == "scope.document")
                {
                    self.focused = index;
                }
            }
            return InputOutcome::Handled;
        }
        if key.code == KeyCode::F(8)
            || (key.code == KeyCode::Char('i') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.semantic_inspection = Some(semantic::SemanticInspection::default());
            return InputOutcome::Handled;
        }
        if should_quit(key) {
            return InputOutcome::QuitRequested;
        }
        self.status = None;
        if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quick_open = Some(quick_open::QuickOpen::default());
            return InputOutcome::Handled;
        }
        if self.panes.is_empty() {
            return InputOutcome::Ignored;
        }
        match key.code {
            KeyCode::Tab => {
                self.focused = (self.focused + 1) % self.panes.len();
                return InputOutcome::Handled;
            }
            KeyCode::BackTab => {
                self.focused = (self.focused + self.panes.len() - 1) % self.panes.len();
                return InputOutcome::Handled;
            }
            _ => {}
        }
        if self.panes[self.focused].id() == "scope.files" {
            match key.code {
                KeyCode::Enter | KeyCode::Char('o') => {
                    if app
                        .open_document()
                        .is_some_and(|document| document.is_dirty())
                    {
                        self.status = Some(
                            "Cannot open another file: the current document has unsaved changes."
                                .into(),
                        );
                    } else {
                        match app.open_selected() {
                            Ok(_) => {
                                self.scroll.fill(0);
                                if let Some(index) = self
                                    .panes
                                    .iter()
                                    .position(|pane| pane.id() == "scope.document")
                                {
                                    self.focused = index;
                                }
                            }
                            Err(error) => self.status = Some(error.to_string()),
                        }
                    }
                    return InputOutcome::Handled;
                }
                KeyCode::Char('r') => {
                    if let Err(error) = app.refresh_files() {
                        self.status = Some(error.to_string());
                    }
                    return InputOutcome::Handled;
                }
                _ => {}
            }
        }
        let mut ctx = TuiContextMut::with_scroll(
            app,
            &mut self.scroll[self.focused],
            self.viewports[self.focused],
        );
        if self.panes[self.focused].handle_key(key, &mut ctx) {
            InputOutcome::Handled
        } else {
            InputOutcome::Ignored
        }
    }
}

impl Default for Tui {
    fn default() -> Self {
        TuiBuilder::new().with_default_panes().build()
    }
}

pub struct TuiBuilder {
    panes: Vec<Box<dyn Pane>>,
    redraw_interval: Duration,
}

impl TuiBuilder {
    pub fn new() -> Self {
        Self {
            panes: Vec::new(),
            redraw_interval: Duration::from_millis(250),
        }
    }

    /// Set the built-in runtime's redraw/polling target (default: 250 ms).
    ///
    /// This is the input polling timeout, not a guaranteed frame cadence or a
    /// simulation clock. Input can trigger earlier redraws; rendering and input
    /// handling can delay them. Zero enables non-blocking polling and may use
    /// substantial CPU. Caller-owned event loops are unaffected.
    pub fn redraw_interval(mut self, interval: Duration) -> Self {
        self.redraw_interval = interval;
        self
    }

    pub fn with_default_panes(mut self) -> Self {
        self.panes.push(Box::new(FilesPane));
        self.panes.push(Box::new(DocumentPane));
        self.panes.push(Box::new(LanguagePane));
        self
    }

    pub fn pane(mut self, pane: impl Pane + 'static) -> Self {
        self.panes.push(Box::new(pane));
        self
    }

    pub fn build(self) -> Tui {
        Tui {
            redraw_interval: self.redraw_interval,
            scroll: vec![0; self.panes.len()],
            viewports: vec![Rect::default(); self.panes.len()],
            panes: self.panes,
            focused: 0,
            quick_open: None,
            status: None,
            semantic_inspection: None,
        }
    }
}

impl Default for TuiBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn should_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
}

fn right_pane_constraints(count: usize) -> Vec<Constraint> {
    if count == 2 {
        return vec![Constraint::Percentage(70), Constraint::Percentage(30)];
    }

    let percentage = 100 / count.max(1) as u16;
    let mut constraints = vec![Constraint::Percentage(percentage); count];

    if let Some(last) = constraints.last_mut() {
        *last = Constraint::Min(1);
    }

    constraints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_outcomes_capture_modal_input_before_quit() {
        let root = crate::test_support::TempWorkspace::new("tui_input_outcomes");
        root.write_file("q.txt", "match");
        let mut app = App::new(root.workspace()).unwrap();
        let mut tui = Tui::default();
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            assert_eq!(
                tui.handle_key(key(code), &mut app),
                InputOutcome::QuitRequested
            );
        }
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let mut event = key(KeyCode::Esc);
            event.kind = kind;
            assert_eq!(tui.handle_key(event, &mut app), InputOutcome::Ignored);
        }
        assert_eq!(
            tui.handle_key(key(KeyCode::F(12)), &mut app),
            InputOutcome::Ignored
        );
        assert_eq!(
            tui.handle_key(key(KeyCode::Down), &mut app),
            InputOutcome::Handled
        );
        let open = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(tui.handle_key(open, &mut app), InputOutcome::Handled);
        assert_eq!(
            tui.handle_key(key(KeyCode::Char('q')), &mut app),
            InputOutcome::Handled
        );
        assert_eq!(
            tui.handle_key(key(KeyCode::Enter), &mut app),
            InputOutcome::Handled
        );
        assert!(app.open_document().is_some());
        assert_eq!(tui.handle_key(open, &mut app), InputOutcome::Handled);
        assert_eq!(
            tui.handle_key(key(KeyCode::Esc), &mut app),
            InputOutcome::Handled
        );
        assert_eq!(
            tui.handle_key(key(KeyCode::Esc), &mut app),
            InputOutcome::QuitRequested
        );

        let mut empty = Tui::builder().build();
        assert_eq!(
            empty.handle_key(key(KeyCode::Tab), &mut app),
            InputOutcome::Ignored
        );
        assert_eq!(
            empty.handle_key(key(KeyCode::Char('q')), &mut app),
            InputOutcome::QuitRequested
        );
    }

    #[test]
    fn redraw_interval_defaults_and_builder_override() {
        assert_eq!(Tui::default().redraw_interval, Duration::from_millis(250));
        let tui = Tui::builder()
            .redraw_interval(Duration::from_millis(100))
            .build();
        assert_eq!(tui.redraw_interval, Duration::from_millis(100));
        assert_eq!(
            Tui::builder()
                .redraw_interval(Duration::ZERO)
                .build()
                .redraw_interval,
            Duration::ZERO
        );
    }

    #[test]
    fn navigation_focus_scrolling_and_picker_are_isolated() {
        let root = crate::test_support::TempWorkspace::new("tui_navigation");
        root.write_file("a.txt", "line\n".repeat(100));
        root.write_file("b.txt", "second");
        let mut app = App::new(root.workspace()).unwrap();
        let mut tui = Tui::default();
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        tui.handle_key(key(KeyCode::Enter), &mut app);
        assert_eq!(tui.focused, 1);
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| tui.render(frame, &app)).unwrap();
        tui.handle_key(key(KeyCode::PageDown), &mut app);
        assert!(tui.scroll[1] > 1);
        assert_eq!(app.selected_index(), Some(0));
        tui.handle_key(key(KeyCode::End), &mut app);
        let bottom = tui.scroll[1];
        tui.handle_key(key(KeyCode::Down), &mut app);
        assert_eq!(tui.scroll[1], bottom);
        tui.handle_key(key(KeyCode::BackTab), &mut app);
        assert_eq!(tui.focused, 0);
        tui.handle_key(
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            &mut app,
        );
        tui.handle_key(key(KeyCode::Char('q')), &mut app);
        assert!(tui.quick_open.is_some());
        tui.handle_key(key(KeyCode::Esc), &mut app);
        assert!(tui.quick_open.is_none());
        tui.handle_key(
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            &mut app,
        );
        tui.handle_key(key(KeyCode::Char('b')), &mut app);
        tui.handle_key(key(KeyCode::Enter), &mut app);
        assert_eq!(app.open_document().unwrap().text(), "second");
        assert_eq!(tui.focused, 1);
        assert_eq!(tui.scroll[1], 0);
    }

    #[test]
    fn empty_tui_handles_focus_and_rendering() {
        let root = crate::test_support::TempWorkspace::new("empty_tui");
        let mut app = App::new(root.workspace()).unwrap();
        let mut tui = Tui::builder().build();
        tui.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), &mut app);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(10, 4)).unwrap();
        terminal.draw(|frame| tui.render(frame, &app)).unwrap();
    }

    #[test]
    fn caller_owned_terminal_redraws_custom_pane_and_clears_old_content() {
        struct CustomPane;

        impl Pane for CustomPane {
            fn id(&self) -> &'static str {
                "example.redraw"
            }

            fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
                let text = if ctx.scroll() == 0 {
                    "long content"
                } else {
                    "short"
                };
                frame.render_widget(Paragraph::new(text), area);
            }

            fn handle_key(&mut self, _key: KeyEvent, ctx: &mut TuiContextMut<'_>) -> bool {
                ctx.set_scroll(1);
                true
            }
        }

        let root = crate::test_support::TempWorkspace::new("tui_redraw");
        let mut app = App::new(root.workspace()).unwrap();
        let mut tui = Tui::builder().pane(CustomPane).build();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        tui.draw(&mut terminal, &app).unwrap();
        let area = tui.viewports[0];
        assert_eq!(
            terminal.backend().buffer()[(area.x + 5, area.y)].symbol(),
            "c"
        );

        tui.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &mut app);
        tui.draw(&mut terminal, &app).unwrap();
        assert_eq!(terminal.backend().buffer()[(area.x, area.y)].symbol(), "s");
        assert_eq!(
            terminal.backend().buffer()[(area.x + 5, area.y)].symbol(),
            " "
        );

        terminal.backend_mut().resize(60, 20);
        tui.draw(&mut terminal, &app).unwrap();
        assert_ne!(tui.viewports[0], area);
        let area = tui.viewports[0];
        assert_eq!(terminal.backend().buffer()[(area.x, area.y)].symbol(), "s");
    }

    #[test]
    fn default_tui_contains_the_builtin_panes() {
        let tui = Tui::default();
        let pane_ids = tui.panes.iter().map(|pane| pane.id()).collect::<Vec<_>>();

        assert_eq!(
            pane_ids,
            vec!["scope.files", "scope.document", "scope.language"]
        );
    }

    #[test]
    fn builder_can_append_custom_panes() {
        struct CustomPane;

        impl Pane for CustomPane {
            fn id(&self) -> &'static str {
                "example.custom"
            }

            fn render(&self, _frame: &mut ratatui::Frame<'_>, _area: Rect, _ctx: &TuiContext<'_>) {}
        }

        let tui = Tui::builder().with_default_panes().pane(CustomPane).build();
        let pane_ids = tui.panes.iter().map(|pane| pane.id()).collect::<Vec<_>>();

        assert_eq!(
            pane_ids,
            vec![
                "scope.files",
                "scope.document",
                "scope.language",
                "example.custom"
            ]
        );
    }
}
