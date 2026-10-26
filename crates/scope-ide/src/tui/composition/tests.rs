use super::*;
use crate::test_support::TempWorkspace;
use std::{cell::Cell, rc::Rc};
struct Counter(Rc<Cell<usize>>);
impl Pane for Counter {
    fn id(&self) -> &'static str {
        "counter.type"
    }
    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, _: &TuiContext<'_>) {
        frame.render_widget(Paragraph::new(self.0.get().to_string()), area);
    }
    fn handle_key(&mut self, _: KeyEvent, _: &mut TuiContextMut<'_>) -> bool {
        self.0.set(self.0.get() + 1);
        true
    }
}
fn fixture() -> (WorkspaceTui, Rc<Cell<usize>>, Rc<Cell<usize>>) {
    let mut ui = WorkspaceTui::new();
    let a = Rc::new(Cell::new(0));
    let b = Rc::new(Cell::new(0));
    ui.register("a", Counter(a.clone())).unwrap();
    ui.register("b", Counter(b.clone())).unwrap();
    ui.layout(
        "both",
        View::vertical([View::horizontal([View::leaf("a"), View::leaf("b")])]),
    )
    .unwrap();
    ui.layout("one", View::leaf("a")).unwrap();
    (ui, a, b)
}
#[test]
fn command_cancel_routing_and_visible_focus() {
    let root = TempWorkspace::new("command-bar-routing");
    let mut app = App::new(root.workspace()).unwrap();
    let (mut ui, a, _) = fixture();
    ui.geometry(Rect::new(0, 0, 40, 10));
    ui.output_area = Some(Rect::new(0, 10, 40, 4));
    ui.focus_named("2").unwrap();
    assert_eq!(ui.focused(), Some("b"));
    ui.focus_named("last").unwrap();
    assert_eq!(ui.focused(), Some("a"));
    ui.focus_named("3").unwrap();
    assert_eq!(ui.focused(), Some("output"));
    assert!(ui.focus_named("4").is_err());
    assert!(ui.focus_named("editor").is_err());
    ui.handle_key(
        KeyEvent::new(KeyCode::F(6), event::KeyModifiers::NONE),
        &mut app,
    );
    assert_eq!(ui.focused(), Some("a"));
    ui.handle_key(
        KeyEvent::new(KeyCode::Char('x'), event::KeyModifiers::NONE),
        &mut app,
    );
    assert_eq!(a.get(), 1);
    ui.handle_key(
        KeyEvent::new(KeyCode::F(4), event::KeyModifiers::NONE),
        &mut app,
    );
    ui.command_event(&Event::Paste(":scope script run".into()));
    assert!(
        ui.command_event(&Event::Key(KeyEvent::new(
            KeyCode::Esc,
            event::KeyModifiers::NONE
        )))
        .is_none()
    );
    assert!(ui.command_line.is_none());
    assert_eq!(a.get(), 1);
    ui.command_line = Some(":scope context".into());
    assert_eq!(
        ui.command_event(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            event::KeyModifiers::NONE
        ))),
        Some(":scope context".into())
    );
    let mut session = InteractiveSession::new();
    let mut sources = crate::scratch::ScratchBuffers::default();
    ui.submit_command(":scope context", &mut app, &mut session, None, &mut sources);
    assert!(!ui.output.bytes().is_empty());
    ui.submit_command("not a command", &mut app, &mut session, None, &mut sources);
    assert!(ui.output_text().contains("explicit :scope"));
    ui.select_layout("one").unwrap();
    ui.geometry(Rect::new(0, 0, 10, 2));
    assert!(ui.focus_named("b").is_err());
    assert!(ui.focus_named("0").is_err());
    ui.focus_named("2").unwrap();
    assert_eq!(ui.focused(), Some("output"));
    ui.output_area = None;
    assert!(ui.focus_named("2").is_err());
    ui.submit_command("help", &mut app, &mut session, None, &mut sources);
    assert!(ui.output_text().contains(":scope help"));
}

#[test]
fn output_paging_uses_inner_height_and_retains_scroll_across_focus() {
    let root = TempWorkspace::new("output-paging");
    let mut app = App::new(root.workspace()).unwrap();
    let (mut ui, a, _) = fixture();
    ui.geometry(Rect::new(0, 0, 40, 10));
    ui.output_area = Some(Rect::new(0, 10, 40, 7));
    ui.output_errors = (0..20).map(|n| format!("line {n}\n")).collect();
    ui.focus_named("output").unwrap();
    let down = KeyEvent::new(KeyCode::PageDown, event::KeyModifiers::NONE);
    let up = KeyEvent::new(KeyCode::PageUp, event::KeyModifiers::NONE);
    assert_eq!(ui.handle_key(down, &mut app), WorkspaceInput::Handled);
    assert_eq!(ui.output_scroll, 5);
    ui.focus_named("a").unwrap();
    ui.handle_key(down, &mut app);
    assert_eq!(a.get(), 1);
    assert_eq!(ui.output_scroll, 5);
    ui.focus_named("last").unwrap();
    ui.output_area = Some(Rect::new(0, 10, 40, 4));
    ui.handle_key(down, &mut app);
    assert_eq!(ui.output_scroll, 7);
    ui.handle_key(up, &mut app);
    assert_eq!(ui.output_scroll, 5);
    ui.output_area = Some(Rect::new(0, 10, 1, 1));
    ui.handle_key(down, &mut app);
    assert_eq!(ui.output_scroll, 6);
    for _ in 0..30 {
        ui.handle_key(down, &mut app);
    }
    assert_eq!(ui.output_scroll, 19);
    for _ in 0..30 {
        ui.handle_key(up, &mut app);
    }
    assert_eq!(ui.output_scroll, 0);
    ui.output_errors.clear();
    ui.handle_key(down, &mut app);
    assert_eq!(ui.output_scroll, 0);
}

#[test]
fn registration_validates_identity_and_tree() {
    let (mut ui, _, _) = fixture();
    assert!(ui.register("a", Counter(Rc::default())).is_err());
    assert!(ui.layout("bad", View::leaf("missing")).is_err());
    assert!(
        ui.layout(
            "duplicate",
            View::horizontal([View::leaf("a"), View::leaf("a")])
        )
        .is_err()
    );
    assert!(
        ui.layout(
            "tabs",
            View::Tabs {
                selected: 2,
                children: vec![View::leaf("a")]
            }
        )
        .is_err()
    );
    assert!(ui.layout("empty", View::vertical([])).is_err());
    assert!(ui.select_layout("missing").is_err());
}
#[test]
fn focus_input_and_instance_state_survive_layout_switches() {
    let root = TempWorkspace::new("composable-focus");
    let mut app = App::new(root.workspace()).unwrap();
    let (mut ui, a, b) = fixture();
    ui.geometry(Rect::new(0, 0, 80, 24));
    let key = |code| KeyEvent::new(code, event::KeyModifiers::NONE);
    assert_eq!(ui.focused(), Some("a"));
    ui.handle_key(key(KeyCode::Char('q')), &mut app);
    ui.handle_key(key(KeyCode::F(6)), &mut app);
    ui.handle_key(key(KeyCode::Tab), &mut app);
    assert_eq!((a.get(), b.get()), (1, 1));
    assert_eq!(
        ui.handle_key(key(KeyCode::Esc), &mut app),
        WorkspaceInput::Shell
    );
    assert_eq!((a.get(), b.get()), (1, 1));
    ui.select_layout("one").unwrap();
    // Stale geometry must not route input after changing layouts.
    assert_eq!(
        ui.handle_key(key(KeyCode::Char('x')), &mut app),
        WorkspaceInput::Ignored
    );
    ui.geometry(Rect::new(0, 0, 80, 24));
    assert_eq!(ui.focused(), Some("a"));
    ui.select_layout("both").unwrap();
    ui.geometry(Rect::new(0, 0, 80, 24));
    ui.handle_key(
        KeyEvent::new(KeyCode::F(6), event::KeyModifiers::SHIFT),
        &mut app,
    );
    assert_eq!(ui.focused(), Some("b"));
    assert_eq!((a.get(), b.get()), (1, 1));
}
#[test]
fn constraints_tabs_and_tiny_sizes_are_bounded_and_deterministic() {
    let view = View::split(
        Direction::Horizontal,
        [
            (Constraint::Length(8), View::leaf("a")),
            (
                Constraint::Min(2),
                View::Tabs {
                    selected: 1,
                    children: vec![View::leaf("hidden"), View::leaf("b")],
                },
            ),
        ],
    );
    for width in 0..30 {
        for height in 0..5 {
            let area = Rect::new(3, 4, width, height);
            let mut first = vec![];
            let mut second = vec![];
            view.place(area, &mut first);
            view.place(area, &mut second);
            assert_eq!(first, second);
            for (id, rect) in first {
                assert_ne!(id, "hidden");
                assert!(rect.x >= area.x && rect.y >= area.y);
                assert!(rect.right() <= area.right() && rect.bottom() <= area.bottom());
            }
        }
    }
    let mut out = vec![];
    view.place(Rect::new(0, 0, 20, 4), &mut out);
    assert_eq!(out[0].1.width, 8);
}
#[test]
fn test_backend_renders_nested_views_and_resizes() {
    let root = TempWorkspace::new("composable-render");
    let app = App::new(root.workspace()).unwrap();
    let (mut ui, _, _) = fixture();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 10)).unwrap();
    terminal.draw(|f| ui.render(f, f.area(), &app)).unwrap();
    assert_eq!(ui.visible.len(), 2);
    terminal.backend_mut().resize(1, 1);
    terminal.draw(|f| ui.render(f, f.area(), &app)).unwrap();
    assert!(ui.visible.len() <= 2);
}

use crate::{
    language::InteractiveSession,
    scratch::{BoundSource, ScratchBuffers},
    tui::EditableScratchPane,
};
use std::cell::RefCell;

#[derive(Default)]
struct ViewEventLog {
    events: Vec<Event>,
    operations: Vec<BoundSource>,
    legacy: Vec<KeyEvent>,
}

struct ViewEventObserver(Rc<RefCell<ViewEventLog>>);
impl Pane for ViewEventObserver {
    fn id(&self) -> &'static str {
        "view-event-observer"
    }
    fn render(&self, _: &mut ratatui::Frame<'_>, _: Rect, _: &TuiContext<'_>) {}
    fn handle_view_event(&mut self, event: Event, ctx: &TuiContext<'_>) -> bool {
        assert!(ctx.is_focused());
        let mut log = self.0.borrow_mut();
        log.events.push(event.clone());
        if matches!(event, Event::Key(key) if key.code == KeyCode::F(5)) {
            log.operations.push(ctx.bound_semantic_source().unwrap());
            true
        } else {
            false
        }
    }
    fn handle_key(&mut self, key: KeyEvent, _: &mut TuiContextMut<'_>) -> bool {
        self.0.borrow_mut().legacy.push(key);
        false
    }
}

fn view_event_workspace(
    root: &TempWorkspace,
) -> (EditableWorkspace, [Rc<RefCell<ViewEventLog>>; 2]) {
    let mut h = EditableWorkspace::new(root, true);
    let logs = std::array::from_fn(|_| Rc::new(RefCell::new(ViewEventLog::default())));
    for (id, log) in ["view-a", "view-b"].into_iter().zip(&logs) {
        h.ui.register(id, ViewEventObserver(log.clone())).unwrap();
    }
    h.ui.layout(
        "view-events",
        View::horizontal([
            View::leaf("editor"),
            View::leaf("view-a"),
            View::leaf("view-b"),
        ]),
    )
    .unwrap();
    h.ui.select_layout("view-events").unwrap();
    h.render();
    assert_eq!(h.ui.focused(), Some("editor"));
    (h, logs)
}

#[test]
fn view_event_f5_reaches_only_focused_view_and_bypasses_legacy_hook() {
    let root = TempWorkspace::new("composition-view-focus");
    let (mut h, [a, b]) = view_event_workspace(&root);
    assert_eq!(
        h.key(KeyCode::Char('x'), event::KeyModifiers::NONE),
        WorkspaceInput::Handled
    );
    assert_eq!(
        h.key(KeyCode::F(5), event::KeyModifiers::NONE),
        WorkspaceInput::Handled
    );
    assert!(a.borrow().events.is_empty());
    assert!(b.borrow().events.is_empty());

    for (id, active, inactive) in [("view-a", &a, &b), ("view-b", &b, &a)] {
        h.key(KeyCode::F(6), event::KeyModifiers::NONE);
        assert_eq!(h.ui.focused(), Some(id));
        let inactive_events = inactive.borrow().events.len();
        assert_eq!(
            h.key(KeyCode::F(5), event::KeyModifiers::NONE),
            WorkspaceInput::Handled
        );
        assert_eq!(active.borrow().operations.len(), 1);
        assert_eq!(active.borrow().events.len(), 1);
        assert!(active.borrow().legacy.is_empty());
        assert_eq!(inactive.borrow().events.len(), inactive_events);
    }
    // Unconsumed keys still reach the legacy callback.
    assert_eq!(
        h.key(KeyCode::Char('z'), event::KeyModifiers::NONE),
        WorkspaceInput::Ignored
    );
    assert_eq!(b.borrow().legacy.len(), 1);
    assert_eq!(b.borrow().legacy[0].code, KeyCode::Char('z'));
}

#[test]
fn view_event_operation_reads_committed_revision_and_paste_is_not_a_command() {
    let root = TempWorkspace::new("composition-view-snapshot");
    let (mut h, [a, b]) = view_event_workspace(&root);
    let original = h.sources.bound_source();
    h.event(Event::Paste("draft ".into()));
    h.key(KeyCode::F(6), event::KeyModifiers::NONE);
    h.key(KeyCode::F(5), event::KeyModifiers::NONE);
    assert_eq!(a.borrow().operations.as_slice(), &[original.clone()]);

    let paste = Event::Paste(":scope line 1 \"not a command\"\nF5\u{1b}[15~".into());
    assert_eq!(h.event(paste.clone()), WorkspaceInput::Ignored);
    assert_eq!(a.borrow().events.last(), Some(&paste));
    assert_eq!(a.borrow().operations.len(), 1);
    assert!(a.borrow().legacy.is_empty());
    assert_eq!(h.sources.bound_source(), original);

    h.key(KeyCode::F(6), event::KeyModifiers::SHIFT);
    assert_eq!(h.ui.focused(), Some("editor"));
    assert_eq!(h.control('s'), WorkspaceInput::Handled);
    let committed = h.sources.bound_source();
    let (BoundSource::Available(before), BoundSource::Available(after)) = (&original, &committed)
    else {
        panic!("expected committed snapshots")
    };
    assert_eq!(after.text.as_ref(), "draft original");
    assert_eq!(after.source_id, before.source_id);
    assert_ne!(after.revision_id, before.revision_id);
    h.event(Event::Paste("uncommitted ".into()));
    h.key(KeyCode::F(6), event::KeyModifiers::NONE);
    h.key(KeyCode::F(5), event::KeyModifiers::NONE);
    assert_eq!(a.borrow().operations.as_slice(), &[original, committed]);
    assert!(b.borrow().events.is_empty());
}

struct OrderedEventPane(Rc<RefCell<Vec<&'static str>>>);
impl Pane for OrderedEventPane {
    fn id(&self) -> &'static str {
        "ordered-event-pane"
    }
    fn render(&self, _: &mut ratatui::Frame<'_>, _: Rect, _: &TuiContext<'_>) {}
    fn handle_source_event(
        &mut self,
        event: Event,
        _: &mut crate::tui::ScratchEditAccess<'_>,
    ) -> bool {
        self.0.borrow_mut().push("source");
        matches!(event, Event::Paste(_))
    }
    fn handle_view_event(&mut self, event: Event, ctx: &TuiContext<'_>) -> bool {
        assert!(ctx.is_focused());
        assert!(ctx.bound_semantic_source().is_some());
        self.0.borrow_mut().push("view");
        matches!(event, Event::Key(key) if key.code == KeyCode::F(5))
    }
    fn handle_key(&mut self, _: KeyEvent, _: &mut TuiContextMut<'_>) -> bool {
        self.0.borrow_mut().push("legacy");
        true
    }
}

#[test]
fn view_event_runs_after_source_hook_and_before_legacy_hook() {
    let root = TempWorkspace::new("composition-view-order");
    let mut app = App::new(root.workspace()).unwrap();
    let mut sources = ScratchBuffers::default();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut ui = WorkspaceTui::new();
    ui.register_editable("ordered", OrderedEventPane(log.clone()))
        .unwrap();
    ui.layout("one", View::leaf("ordered")).unwrap();
    ui.geometry(Rect::new(0, 0, 80, 24));
    for (input, expected) in [
        (Event::Paste("F5".into()), vec!["source"]),
        (
            Event::Key(KeyEvent::new(KeyCode::F(5), event::KeyModifiers::NONE)),
            vec!["source", "view"],
        ),
        (
            Event::Key(KeyEvent::new(KeyCode::Char('x'), event::KeyModifiers::NONE)),
            vec!["source", "view", "legacy"],
        ),
    ] {
        log.borrow_mut().clear();
        assert_eq!(
            ui.handle_event_with_sources(input, &mut app, &mut sources),
            WorkspaceInput::Handled
        );
        assert_eq!(*log.borrow(), expected);
    }
}

struct SourceObserver(Rc<RefCell<Vec<BoundSource>>>);
impl Pane for SourceObserver {
    fn id(&self) -> &'static str {
        "independent-source-observer"
    }
    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        let source = ctx.bound_semantic_source().expect("source owner supplied");
        let text = match &source {
            BoundSource::Available(snapshot) => snapshot.text.to_string(),
            other => format!("{other:?}"),
        };
        self.0.borrow_mut().push(source);
        frame.render_widget(Paragraph::new(text), area);
    }
}

struct EditableWorkspace {
    app: App,
    session: InteractiveSession,
    sources: ScratchBuffers,
    ui: WorkspaceTui,
    terminal: ratatui::Terminal<ratatui::backend::TestBackend>,
    observed: Rc<RefCell<Vec<BoundSource>>>,
}
impl EditableWorkspace {
    fn new(root: &TempWorkspace, editable: bool) -> Self {
        let mut sources = ScratchBuffers::default();
        let id = sources.create("text", "original");
        sources.bind_semantic_target(Some(id)).unwrap();
        let mut ui = WorkspaceTui::new();
        if editable {
            ui.register_editable("editor", EditableScratchPane::new())
                .unwrap();
        } else {
            ui.register("editor", EditableScratchPane::new()).unwrap();
        }
        let observed = Rc::new(RefCell::new(Vec::new()));
        ui.register("observer", SourceObserver(observed.clone()))
            .unwrap();
        ui.layout(
            "both",
            View::horizontal([View::leaf("editor"), View::leaf("observer")]),
        )
        .unwrap();
        ui.layout("observer-only", View::leaf("observer")).unwrap();
        let mut h = Self {
            app: App::new(root.workspace()).unwrap(),
            session: InteractiveSession::new(),
            sources,
            ui,
            terminal: ratatui::Terminal::new(ratatui::backend::TestBackend::new(320, 14)).unwrap(),
            observed,
        };
        h.render();
        h
    }
    fn event(&mut self, event: Event) -> WorkspaceInput {
        self.ui
            .handle_event_with_sources(event, &mut self.app, &mut self.sources)
    }
    fn key(&mut self, code: KeyCode, modifiers: event::KeyModifiers) -> WorkspaceInput {
        self.event(Event::Key(KeyEvent::new(code, modifiers)))
    }
    fn control(&mut self, ch: char) -> WorkspaceInput {
        self.key(KeyCode::Char(ch), event::KeyModifiers::CONTROL)
    }
    fn render(&mut self) {
        self.terminal
            .draw(|f| {
                self.ui
                    .render_with_sources(f, f.area(), &self.app, &self.sources)
            })
            .unwrap();
    }
    fn pane_text(&self, id: &str) -> String {
        let area = self
            .ui
            .visible
            .iter()
            .find(|(name, _)| name == id)
            .unwrap()
            .1;
        let buffer = self.terminal.backend().buffer();
        (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn assert_observer(&mut self, text: &str) -> scope_language::SourceSnapshot {
        self.render();
        let source = self.observed.borrow().last().unwrap().clone();
        assert_eq!(source, self.sources.bound_source());
        let BoundSource::Available(snapshot) = source else {
            panic!("expected snapshot")
        };
        assert_eq!(snapshot.text.as_ref(), text);
        assert!(self.pane_text("observer").contains(text));
        snapshot
    }
    fn shell(&mut self, input: &str) {
        let mut output = Vec::new();
        crate::cli::run_borrowed_with_sources(
            &mut self.app,
            &mut self.session,
            None,
            &mut self.sources,
            input.as_bytes(),
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("error:"), "{output}");
    }
}

#[test]
fn editable_commit_updates_independent_observer_snapshot_and_backend_text() {
    let root = TempWorkspace::new("composition-edit-observer");
    let mut h = EditableWorkspace::new(&root, true);
    let original = h.assert_observer("original");
    assert_eq!(
        h.event(Event::Paste("é draft ".into())),
        WorkspaceInput::Handled
    );
    assert_eq!(h.assert_observer("original"), original);
    assert!(h.pane_text("editor").contains("é draft original"));
    assert!(!h.pane_text("observer").contains("é draft"));
    assert_eq!(h.control('s'), WorkspaceInput::Handled);
    let committed = h.assert_observer("é draft original");
    assert_eq!(committed.source_id, original.source_id);
    assert_ne!(committed.revision_id, original.revision_id);
    assert_eq!(original.text.as_ref(), "original");
}

#[test]
fn ordinary_editor_registration_never_grants_source_edit_authority() {
    let root = TempWorkspace::new("composition-edit-opt-in");
    let mut h = EditableWorkspace::new(&root, false);
    let original = h.assert_observer("original");
    for input in [
        Event::Paste("unauthorized".into()),
        Event::Key(KeyEvent::new(KeyCode::Char('x'), event::KeyModifiers::NONE)),
    ] {
        assert_eq!(h.event(input), WorkspaceInput::Ignored);
    }
    assert_eq!(h.control('s'), WorkspaceInput::Ignored);
    assert_eq!(h.assert_observer("original"), original);
    assert!(!h.pane_text("editor").contains("LOCAL DRAFT"));
    assert!(!h.pane_text("editor").contains("unauthorized"));
}

#[test]
fn host_shortcuts_precede_editor_and_focus_layout_changes_retain_draft() {
    let root = TempWorkspace::new("composition-edit-host-keys");
    let mut h = EditableWorkspace::new(&root, true);
    let original = h.sources.bound_source();
    h.event(Event::Paste("retained ".into()));
    for code in [KeyCode::Esc, KeyCode::F(2)] {
        assert_eq!(
            h.key(code, event::KeyModifiers::NONE),
            WorkspaceInput::Shell
        );
    }
    assert_eq!(h.control('q'), WorkspaceInput::Quit);
    assert_eq!(
        h.key(KeyCode::F(6), event::KeyModifiers::NONE),
        WorkspaceInput::Handled
    );
    assert_eq!(h.ui.focused(), Some("observer"));
    assert_eq!(
        h.event(Event::Paste("not routed".into())),
        WorkspaceInput::Ignored
    );
    assert_eq!(
        h.key(KeyCode::F(6), event::KeyModifiers::SHIFT),
        WorkspaceInput::Handled
    );
    assert_eq!(h.ui.focused(), Some("editor"));
    assert_eq!(
        h.key(KeyCode::F(7), event::KeyModifiers::NONE),
        WorkspaceInput::Handled
    );
    h.render();
    assert_eq!(h.ui.focused(), Some("observer"));
    assert_eq!(
        h.key(KeyCode::F(7), event::KeyModifiers::NONE),
        WorkspaceInput::Handled
    );
    h.render();
    h.key(KeyCode::F(6), event::KeyModifiers::NONE);
    assert_eq!(h.ui.focused(), Some("editor"));
    assert_eq!(h.sources.bound_source(), original);
    assert!(h.pane_text("editor").contains("retained original"));
    h.control('s');
    h.assert_observer("retained original");
}

#[test]
fn borrowed_shell_handoffs_retain_conflicting_draft_until_explicit_cancel() {
    let root = TempWorkspace::new("composition-edit-shell");
    let mut h = EditableWorkspace::new(&root, true);
    let original = h.assert_observer("original");
    h.event(Event::Paste("local ".into()));
    for (shortcut, text, ending) in [
        (KeyCode::Esc, "shell first", ":scope quit\n"),
        (KeyCode::F(2), "shell second", ""),
    ] {
        assert_eq!(
            h.key(shortcut, event::KeyModifiers::NONE),
            WorkspaceInput::Shell
        );
        h.shell(&format!(":scope line 1 \"{text}\"\n{ending}"));
        let current = h.assert_observer(text);
        assert_eq!(current.source_id, original.source_id);
        assert_ne!(current.revision_id, original.revision_id);
        assert!(h.pane_text("editor").contains("local original"));
        assert!(h.pane_text("editor").contains("CONFLICT"));
        h.control('s');
        assert_eq!(h.assert_observer(text), current);
        assert!(h.pane_text("editor").contains("local original"));
    }
    h.control('c');
    h.assert_observer("shell second");
    assert!(!h.pane_text("editor").contains("local original"));
    h.event(Event::Paste("fresh ".into()));
    h.control('s');
    h.assert_observer("fresh shell second");
    assert_eq!(original.text.as_ref(), "original");
}

#[derive(Default)]
struct CommandGuardHost {
    entries: Vec<crate::cli::composition::Composition>,
    calls: Vec<String>,
    completed: usize,
}
impl ReadOnlyCommand for CommandGuardHost {
    fn compositions(&self) -> &[crate::cli::composition::Composition] {
        &self.entries
    }
    fn downstream_namespaces(&self) -> &[&str] {
        &["native"]
    }
    fn composition_completed(&mut self, _: &crate::cli::composition::CompositionReport) {
        self.completed += 1;
    }
    fn invoke_downstream(
        &mut self,
        _: &str,
        command: &str,
        _: &[&str],
        _: Option<&crate::scratch::ScratchBuffer>,
        _: &mut dyn std::io::Write,
    ) -> Result<crate::cli::DownstreamOutcome, crate::cli::DownstreamError> {
        self.calls.push(command.into());
        Ok(crate::cli::DownstreamOutcome::Handled)
    }
    fn invoke_script(
        &mut self,
        _: &crate::scratch::ScratchBuffer,
        _: &mut dyn std::io::Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.calls.push("script".into());
        Ok(())
    }
    fn invoke(
        &mut self,
        _: &crate::document::Document,
        _: crate::text::TextRange,
        _: &str,
        _: &mut dyn std::io::Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("must not dispatch App document operations")
    }
}

#[test]
fn command_bar_execution_blocks_before_binding_and_recovers_after_draft_resolution() {
    use crate::cli::composition::{Composition, CompositionStep};

    for commit in [false, true] {
        let root = TempWorkspace::new("command-bar-draft-guard");
        let mut h = EditableWorkspace::new(&root, true);
        let original = h.sources.selected().unwrap().id();
        h.event(Event::Paste("draft ".into()));
        let script = h.sources.create("scope-script", "script");
        h.render();
        h.ui.select_layout("observer-only").unwrap();
        h.render();
        let mut host = CommandGuardHost {
            entries: vec![Composition {
                namespace: "workflow".into(),
                command: "run".into(),
                steps: vec![
                    CompositionStep::Create {
                        language: "text".into(),
                    },
                    CompositionStep::BindSelected {
                        expected_language: "text".into(),
                    },
                    CompositionStep::Downstream {
                        namespace: "native".into(),
                        command: "composed".into(),
                        args: vec![],
                    },
                ],
            }],
            calls: vec![],
            completed: 0,
        };
        let bound = h.sources.bound_source();
        for line in [
            ":scope downstream native run arg",
            ":scope   downstream\tnative run",
            ":scope workflow run",
            ":scope  workflow\trun  ",
            ":scope script run",
        ] {
            h.ui.submit_command(
                line,
                &mut h.app,
                &mut h.session,
                Some(&mut host),
                &mut h.sources,
            );
            assert!(h.ui.output_errors.contains("pane editor"));
            assert!(h.ui.output_errors.contains(&format!("scratch {original}")));
            assert!(h.ui.output.bytes().is_empty(), "blocked before shell entry");
            assert_eq!(h.sources.selected().unwrap().id(), script);
            assert_eq!(h.sources.bound_source(), bound);
            assert_eq!(h.sources.get(original).unwrap().text(), "original");
            assert!(host.calls.is_empty());
            assert_eq!(host.completed, 0);
            h.ui.output_errors.clear();
        }
        for line in [
            "help",
            ":scope help",
            ":scope context",
            ":scope workflow unknown",
            ":scope workflow run extra",
        ] {
            h.ui.submit_command(
                line,
                &mut h.app,
                &mut h.session,
                Some(&mut host),
                &mut h.sources,
            );
            assert!(!h.ui.output_errors.contains("commit or cancel"));
            assert!(host.calls.is_empty());
            h.ui.output_errors.clear();
        }
        h.ui.select_layout("both").unwrap();
        h.sources.select(original).unwrap();
        h.render();
        h.ui.focus_named("editor").unwrap();
        h.control(if commit { 's' } else { 'c' });
        assert!(h.ui.draft_blocker().is_none());
        assert_eq!(
            h.sources.get(original).unwrap().text(),
            if commit { "draft original" } else { "original" }
        );
        h.sources.select(script).unwrap();
        for line in [
            ":scope script run",
            ":scope downstream native run",
            ":scope workflow run",
        ] {
            h.ui.submit_command(
                line,
                &mut h.app,
                &mut h.session,
                Some(&mut host),
                &mut h.sources,
            );
            assert!(h.ui.output_errors.is_empty(), "{}", h.ui.output_errors);
        }
        assert_eq!(host.calls, ["script", "run", "composed"]);
        assert_eq!(host.completed, 1);
        assert_ne!(h.sources.selected().unwrap().id(), script);
        assert_eq!(
            h.sources.semantic_target_id(),
            Some(h.sources.selected().unwrap().id())
        );
    }
}

#[test]
fn proposal_acceptance_blocks_selected_parked_and_unrelated_dirty_drafts() {
    for parked in [false, true] {
        let root = TempWorkspace::new("proposal-draft-guard");
        let mut h = EditableWorkspace::new(&root, true);
        let original = h.sources.selected().unwrap().id();
        h.ui.submit_command(
            r#":scope source-propose {"edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":8}},"replacement":"candidate"}]}"#,
            &mut h.app, &mut h.session, None, &mut h.sources,
        );
        h.event(Event::Paste("draft ".into()));
        if parked {
            h.sources.create("text", "unrelated");
            h.render();
            h.ui.select_layout("observer-only").unwrap();
            h.render();
        }
        h.ui.submit_command(
            ":scope source-diff 1",
            &mut h.app,
            &mut h.session,
            None,
            &mut h.sources,
        );
        assert!(h.ui.output_errors.is_empty());
        h.ui.submit_command(
            ":scope source-accept 1",
            &mut h.app,
            &mut h.session,
            None,
            &mut h.sources,
        );
        assert!(h.ui.output_errors.contains("commit or cancel"));
        assert_eq!(h.sources.get(original).unwrap().text(), "original");
        assert_eq!(h.sources.get(original).unwrap().version(), 0);
        h.ui.select_layout("both").unwrap();
        h.sources.select(original).unwrap();
        h.render();
        h.ui.focus_named("editor").unwrap();
        h.control('c');
        h.ui.output_errors.clear();
        h.ui.submit_command(
            ":scope source-accept 1",
            &mut h.app,
            &mut h.session,
            None,
            &mut h.sources,
        );
        assert!(h.ui.output_errors.is_empty());
        assert_eq!(h.sources.get(original).unwrap().text(), "candidate");
        assert_eq!(h.sources.get(original).unwrap().version(), 1);
    }
}

#[derive(Default)]
struct DirectScriptHost {
    supported: bool,
    calls: Vec<crate::cli::ScriptPresentation>,
}
impl ReadOnlyCommand for DirectScriptHost {
    fn supports_direct_script(&self) -> bool {
        self.supported
    }
    fn invoke_script_direct(
        &mut self,
        source: &crate::scratch::ScratchBuffer,
    ) -> Result<crate::cli::ScriptPresentation, Box<dyn std::error::Error>> {
        let result = crate::cli::ScriptPresentation {
            source_identity: source.evaluation_source_id().clone(),
            source_revision: scope_language::RevisionId::from_version(source.version()),
            source_text: source.text().into(),
            prints: vec![format!("printed {}", source.text())],
            final_value: Some("42".into()),
            error: None,
        };
        self.calls.push(result.clone());
        Ok(result)
    }
    fn invoke(
        &mut self,
        _: &crate::document::Document,
        _: crate::text::TextRange,
        _: &str,
        _: &mut dyn std::io::Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("workspace actions must not dispatch document operations")
    }
}

#[test]
fn workspace_action_direct_run_requires_opt_in_and_live_script_selection() {
    let root = TempWorkspace::new("composition-action-authority");
    let mut h = EditableWorkspace::new(&root, true);
    let mut host = DirectScriptHost::default();
    let script = h.sources.create("scope-script", "selected script");
    let bound = h.sources.bound_source();
    assert!(h.ui.actions(&h.sources, false)[0].disabled_reason.is_some());
    assert!(
        h.ui.invoke_action(WorkspaceAction::RunScript, &mut h.app, &mut h.sources, None)
            .is_err()
    );
    assert!(
        h.ui.invoke_action(
            WorkspaceAction::RunScript,
            &mut h.app,
            &mut h.sources,
            Some(&mut host)
        )
        .is_err()
    );
    assert!(host.calls.is_empty());
    host.supported = true;
    h.ui.invoke_action(
        WorkspaceAction::RunScript,
        &mut h.app,
        &mut h.sources,
        Some(&mut host),
    )
    .unwrap();
    assert_eq!(host.calls.len(), 1);
    assert_eq!(
        host.calls[0].source_identity,
        *h.sources.get(script).unwrap().evaluation_source_id()
    );
    assert_eq!(host.calls[0].source_text, "selected script");
    assert_eq!(h.sources.bound_source(), bound);
    h.sources.create("text", "not script");
    let error =
        h.ui.invoke_action(
            WorkspaceAction::RunScript,
            &mut h.app,
            &mut h.sources,
            Some(&mut host),
        )
        .unwrap_err();
    assert!(error.contains("not scope-script"), "{error}");
    let mut empty = ScratchBuffers::default();
    let error =
        h.ui.invoke_action(
            WorkspaceAction::RunScript,
            &mut h.app,
            &mut empty,
            Some(&mut host),
        )
        .unwrap_err();
    assert!(error.contains("no live selected scratch"), "{error}");
    assert_eq!(host.calls.len(), 1);
    assert_eq!(h.ui.results.len(), 1);
}

#[test]
fn workspace_action_dirty_selected_and_parked_unrelated_drafts_block_with_metadata() {
    let root = TempWorkspace::new("composition-action-blockers");
    let mut h = EditableWorkspace::new(&root, true);
    let mut host = DirectScriptHost {
        supported: true,
        ..Default::default()
    };
    let script = h.sources.create("scope-script", "committed");
    h.render();
    assert!(h.ui.actions(&h.sources, true)[0].disabled_reason.is_none());
    h.event(Event::Paste("draft ".into()));
    for unrelated in [false, true] {
        if unrelated {
            h.sources
                .create("scope-script", "unrelated selected script");
            h.render();
            h.ui.select_layout("observer-only").unwrap();
            h.render();
        }
        let actions = h.ui.actions(&h.sources, true);
        let reason = actions
            .iter()
            .find(|i| i.action == WorkspaceAction::RunScript)
            .unwrap()
            .disabled_reason
            .as_ref()
            .unwrap();
        assert!(reason.contains("pane editor"), "{reason}");
        assert!(reason.contains(&format!("scratch {script}")), "{reason}");
        assert!(reason.contains("unrelated"), "{reason}");
        let draft = actions
            .iter()
            .find(|i| {
                i.action
                    == WorkspaceAction::FocusDraft {
                        pane: "editor".into(),
                        scratch: script,
                    }
            })
            .unwrap();
        assert_eq!(draft.id, format!("draft.editor.{script}"));
        assert_eq!(draft.disabled_reason.is_some(), unrelated);
        assert!(
            h.ui.invoke_action(
                WorkspaceAction::RunScript,
                &mut h.app,
                &mut h.sources,
                Some(&mut host)
            )
            .is_err()
        );
        assert!(host.calls.is_empty());
        assert!(h.ui.results.is_empty());
        assert_eq!(h.sources.get(script).unwrap().text(), "committed");
    }
    h.ui.select_layout("both").unwrap();
    h.render();
    h.ui.invoke_action(
        WorkspaceAction::FocusDraft {
            pane: "editor".into(),
            scratch: script,
        },
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert_eq!(h.ui.focused(), Some("editor"));
    assert_eq!(h.sources.selected().unwrap().id(), script);
    h.control('c');
    h.ui.invoke_action(
        WorkspaceAction::RunScript,
        &mut h.app,
        &mut h.sources,
        Some(&mut host),
    )
    .unwrap();
    assert_eq!(host.calls.len(), 1);
}

#[test]
fn workspace_action_results_retain_owned_revisions_and_history_attribution() {
    let root = TempWorkspace::new("composition-action-results");
    let mut h = EditableWorkspace::new(&root, true);
    let mut host = DirectScriptHost {
        supported: true,
        ..Default::default()
    };
    let script = h.sources.create("scope-script", "original script");
    h.render();
    h.ui.invoke_action(
        WorkspaceAction::RunScript,
        &mut h.app,
        &mut h.sources,
        Some(&mut host),
    )
    .unwrap();
    let original = h.ui.results[0].clone();
    let current = h.ui.result_text(Some(&h.sources)).unwrap();
    assert!(current.contains("current committed revision"));
    assert!(current.contains("not semantic approval"));
    h.event(Event::Paste("new ".into()));
    assert_eq!(h.ui.result_text(Some(&h.sources)).unwrap(), current);
    h.ui.invoke_action(
        WorkspaceAction::CommitDraft,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert!(
        h.ui.result_text(Some(&h.sources))
            .unwrap()
            .contains("older committed revision")
    );
    h.ui.invoke_action(
        WorkspaceAction::RunScript,
        &mut h.app,
        &mut h.sources,
        Some(&mut host),
    )
    .unwrap();
    assert_eq!(h.ui.results.len(), 2);
    assert_eq!(h.ui.results[0].source_text, original.source_text);
    assert_eq!(h.ui.results[0].source_revision, original.source_revision);
    assert_eq!(h.ui.results[0].prints, original.prints);
    assert_eq!(h.ui.results[0].final_value, original.final_value);
    assert!(
        h.ui.result_text(Some(&h.sources))
            .unwrap()
            .contains("current committed revision")
    );
    h.ui.invoke_action(
        WorkspaceAction::PreviousResult,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    let older = h.ui.result_text(Some(&h.sources)).unwrap();
    assert!(older.contains("Result 1/2"));
    assert!(older.contains("older committed revision"));
    assert!(older.contains("printed original script"));
    assert!(!older.contains("printed new original script"));
    h.ui.invoke_action(
        WorkspaceAction::NextResult,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert!(
        h.ui.invoke_action(
            WorkspaceAction::NextResult,
            &mut h.app,
            &mut h.sources,
            None
        )
        .is_err()
    );
    h.ui.invoke_action(
        WorkspaceAction::ShowTranscript,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert!(h.ui.result_text(Some(&h.sources)).is_none());
    h.ui.invoke_action(
        WorkspaceAction::ShowResult,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert!(h.ui.result_text(Some(&h.sources)).is_some());
    assert_eq!(h.ui.results.len(), 2);
    h.sources.select(script).unwrap();
    h.sources.close_selected(true).unwrap();
    h.ui.invoke_action(
        WorkspaceAction::PreviousResult,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert!(
        h.ui.result_text(Some(&h.sources))
            .unwrap()
            .contains("source closed or unavailable")
    );
}

#[test]
fn workspace_action_picker_filters_cancels_and_does_not_dispatch_keys() {
    let root = TempWorkspace::new("composition-action-picker");
    let (mut h, [a, b]) = view_event_workspace(&root);
    let before = h.sources.bound_source();
    h.key(KeyCode::F(1), event::KeyModifiers::NONE);
    assert!(h.ui.picker.is_some());
    for ch in "ScRiPt.RuN".chars() {
        assert!(
            h.ui.picker_event(
                &Event::Key(KeyEvent::new(KeyCode::Char(ch), event::KeyModifiers::NONE)),
                &h.sources,
                true
            )
            .is_none()
        );
    }
    let items = h.ui.picker_items(&h.sources, true);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].action, WorkspaceAction::RunScript);
    assert!(items[0].disabled_reason.is_some());
    for code in [KeyCode::Enter, KeyCode::F(5), KeyCode::F(2), KeyCode::F(6)] {
        assert!(
            h.ui.picker_event(
                &Event::Key(KeyEvent::new(code, event::KeyModifiers::NONE)),
                &h.sources,
                true
            )
            .is_none()
        );
        assert!(h.ui.picker.is_some());
    }
    assert!(
        h.ui.picker_event(
            &Event::Key(KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE)),
            &h.sources,
            true
        )
        .is_none()
    );
    assert!(h.ui.picker.is_none());
    assert_eq!(h.ui.focused(), Some("editor"));
    assert_eq!(h.sources.bound_source(), before);
    assert!(h.ui.results.is_empty());
    assert!(a.borrow().events.is_empty());
    assert!(b.borrow().events.is_empty());
    h.key(KeyCode::F(1), event::KeyModifiers::NONE);
    for ch in "scratch.new-script".chars() {
        h.ui.picker_event(
            &Event::Key(KeyEvent::new(KeyCode::Char(ch), event::KeyModifiers::NONE)),
            &h.sources,
            false,
        );
    }
    assert_eq!(
        h.ui.picker_event(
            &Event::Key(KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE)),
            &h.sources,
            false
        ),
        Some(WorkspaceAction::NewScript)
    );
    assert!(h.ui.picker.is_none());
    assert_eq!(h.sources.bound_source(), before);
}

fn downstream_test_action() -> DownstreamAction {
    DownstreamAction {
        id: "inspect".into(),
        label: "Inspect selected text".into(),
        language: "text".into(),
        namespace: "native".into(),
        command: "inspect".into(),
        args: vec!["summary".into(), "--readonly".into()],
    }
}

fn discovered_downstream(h: &EditableWorkspace) -> ActionInfo {
    h.ui.actions(&h.sources, false)
        .into_iter()
        .find(|item| item.id == "downstream.inspect")
        .expect("registered action for selected language")
}

#[derive(Default)]
struct DownstreamActionHost {
    deny: bool,
    calls: Vec<(
        String,
        String,
        Vec<String>,
        u64,
        scope_language::SourceId,
        u64,
        String,
    )>,
}

impl ReadOnlyCommand for DownstreamActionHost {
    fn downstream_namespaces(&self) -> &[&str] {
        &["native"]
    }

    fn invoke_downstream(
        &mut self,
        namespace: &str,
        command: &str,
        args: &[&str],
        source: Option<&crate::scratch::ScratchBuffer>,
        output: &mut dyn std::io::Write,
    ) -> Result<crate::cli::DownstreamOutcome, crate::cli::DownstreamError> {
        let source = source.expect("action binds its pinned source");
        self.calls.push((
            namespace.into(),
            command.into(),
            args.iter().map(|s| (*s).into()).collect(),
            source.id(),
            source.evaluation_source_id().clone(),
            source.version(),
            source.text().into(),
        ));
        if self.deny {
            return Err(crate::cli::DownstreamError::new(
                "host_denied",
                "not authorized",
            ));
        }
        writeln!(output, "inspected committed source: {}", source.text()).unwrap();
        Ok(crate::cli::DownstreamOutcome::Handled)
    }

    fn invoke(
        &mut self,
        _: &crate::document::Document,
        _: crate::text::TextRange,
        _: &str,
        _: &mut dyn std::io::Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("downstream actions must not invoke document operations")
    }
}

#[test]
fn downstream_action_registration_discovery_filters_language_and_preserves_builtins() {
    let root = TempWorkspace::new("downstream-action-discovery");
    let mut h = EditableWorkspace::new(&root, true);
    let before = h.ui.actions(&h.sources, false);
    h.ui.register_downstream_action(downstream_test_action())
        .unwrap();
    assert!(
        h.ui.register_downstream_action(downstream_test_action())
            .is_err()
    );
    for invalid in [
        DownstreamAction {
            id: "bad id".into(),
            ..downstream_test_action()
        },
        DownstreamAction {
            id: "bad-args".into(),
            args: vec!["two words".into()],
            ..downstream_test_action()
        },
        DownstreamAction {
            id: "bad-command".into(),
            command: "run;quit".into(),
            ..downstream_test_action()
        },
        DownstreamAction {
            id: "empty-language".into(),
            language: String::new(),
            ..downstream_test_action()
        },
    ] {
        assert!(h.ui.register_downstream_action(invalid).is_err());
    }
    let actions = h.ui.actions(&h.sources, false);
    assert_eq!(actions.len(), before.len() + 1);
    for builtin in before {
        let current = actions.iter().find(|item| item.id == builtin.id).unwrap();
        assert_eq!(current.action, builtin.action);
        assert_eq!(current.disabled_reason, builtin.disabled_reason);
    }
    let item = discovered_downstream(&h);
    assert_eq!(item.label, "Inspect selected text");
    assert_eq!(item.shortcut, None);
    assert_eq!(item.disabled_reason, None);
    let selected = h.sources.selected().unwrap();
    assert_eq!(
        item.action,
        WorkspaceAction::Downstream {
            id: "inspect".into(),
            scratch: selected.id(),
            source: selected.evaluation_source_id().clone(),
            language: "text".into(),
            version: selected.version(),
        }
    );
    h.sources.create("scope-script", "script");
    assert!(
        !h.ui
            .actions(&h.sources, true)
            .iter()
            .any(|i| i.id.starts_with("downstream."))
    );
    assert!(
        h.ui.actions(&h.sources, true)
            .iter()
            .any(|i| i.action == WorkspaceAction::RunScript && i.disabled_reason.is_none())
    );
    assert!(
        !h.ui
            .actions(&ScratchBuffers::default(), false)
            .iter()
            .any(|i| i.id.starts_with("downstream."))
    );
}

#[test]
fn downstream_action_invokes_callback_with_pinned_source_and_transcript_output() {
    let root = TempWorkspace::new("downstream-action-invoke");
    let mut h = EditableWorkspace::new(&root, true);
    let old_target = h.sources.semantic_target_id();
    let selected = h.sources.create("text", "selected committed text");
    h.ui.register_downstream_action(downstream_test_action())
        .unwrap();
    let action = discovered_downstream(&h).action;
    let source = h.sources.selected().unwrap();
    let identity = source.evaluation_source_id().clone();
    let version = source.version();
    let mut host = DownstreamActionHost::default();
    h.ui.show_result = true;
    h.ui.invoke_action(action, &mut h.app, &mut h.sources, Some(&mut host))
        .unwrap();
    assert_ne!(old_target, Some(selected));
    assert_eq!(h.sources.semantic_target_id(), Some(selected));
    assert_eq!(h.sources.selected().unwrap().id(), selected);
    assert_eq!(
        host.calls,
        vec![(
            "native".into(),
            "inspect".into(),
            vec!["summary".into(), "--readonly".into()],
            selected,
            identity,
            version,
            "selected committed text".into()
        )]
    );
    let output = String::from_utf8_lossy(h.ui.output.bytes());
    assert!(
        output.contains("inspected committed source: selected committed text"),
        "{output}"
    );
    assert!(h.ui.output_errors.contains(&format!("scratch {selected}")));
    assert!(
        h.ui.output_errors
            .contains(&format!("committed revision {version}"))
    );
    assert!(h.ui.output_errors.contains("not semantic approval"));
    assert!(!h.ui.show_result);
    assert!(h.ui.results.is_empty());
}

#[test]
fn downstream_action_rejects_stale_revision_selection_and_closed_source_before_binding() {
    for change in ["revision", "selection", "closure"] {
        let root = TempWorkspace::new("downstream-action-stale");
        let mut h = EditableWorkspace::new(&root, true);
        h.sources.create("text", "selected");
        h.render();
        h.ui.register_downstream_action(downstream_test_action())
            .unwrap();
        let action = discovered_downstream(&h).action;
        match change {
            "revision" => {
                h.event(Event::Paste("new ".into()));
                h.control('s');
            }
            "selection" => {
                h.sources.create("text", "another source");
            }
            "closure" => {
                h.sources.close_selected(true).unwrap();
            }
            _ => unreachable!(),
        }
        let bound = h.sources.bound_source();
        let mut host = DownstreamActionHost::default();
        let error =
            h.ui.invoke_action(action, &mut h.app, &mut h.sources, Some(&mut host))
                .unwrap_err();
        assert!(
            error.contains("not available") || error.contains("source changed"),
            "{change}: {error}"
        );
        assert!(host.calls.is_empty(), "{change}");
        assert_eq!(h.sources.bound_source(), bound, "{change}");
        assert!(h.ui.output.bytes().is_empty());
        assert!(h.ui.output_errors.is_empty());
    }
}

#[test]
fn downstream_action_authorization_denial_is_reported_in_output() {
    let root = TempWorkspace::new("downstream-action-denied");
    let mut h = EditableWorkspace::new(&root, true);
    h.ui.register_downstream_action(downstream_test_action())
        .unwrap();
    let item = discovered_downstream(&h);
    assert!(
        item.disabled_reason.is_none(),
        "registration is not authorization"
    );
    let mut host = DownstreamActionHost {
        deny: true,
        ..Default::default()
    };
    h.ui.invoke_action(item.action, &mut h.app, &mut h.sources, Some(&mut host))
        .unwrap();
    assert_eq!(host.calls.len(), 1);
    let output = String::from_utf8_lossy(h.ui.output.bytes());
    assert!(output.contains("host_denied"), "{output}");
    assert!(output.contains("not authorized"), "{output}");
    assert!(!output.contains("inspected committed source"));
    assert!(h.ui.results.is_empty());
}

#[test]
fn downstream_action_dirty_selected_and_parked_drafts_reject_before_binding() {
    let root = TempWorkspace::new("downstream-action-drafts");
    let mut h = EditableWorkspace::new(&root, true);
    h.ui.register_downstream_action(downstream_test_action())
        .unwrap();
    let original = h.sources.selected().unwrap().id();
    let action = discovered_downstream(&h).action;
    h.event(Event::Paste("draft ".into()));
    let mut host = DownstreamActionHost::default();
    for parked in [false, true] {
        let action = if parked {
            h.sources.create("text", "unrelated selected source");
            h.render();
            h.ui.select_layout("observer-only").unwrap();
            h.render();
            discovered_downstream(&h).action
        } else {
            action.clone()
        };
        let reason = discovered_downstream(&h).disabled_reason.unwrap();
        assert!(reason.contains("pane editor"), "{reason}");
        assert!(reason.contains(&format!("scratch {original}")), "{reason}");
        let bound = h.sources.bound_source();
        let error =
            h.ui.invoke_action(action, &mut h.app, &mut h.sources, Some(&mut host))
                .unwrap_err();
        assert_eq!(error, reason);
        assert!(host.calls.is_empty());
        assert_eq!(h.sources.bound_source(), bound);
        assert_eq!(h.sources.get(original).unwrap().text(), "original");
        assert!(h.ui.output.bytes().is_empty());
        assert!(h.ui.output_errors.is_empty());
    }
}
