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
#[test]
fn document_actions_reuse_command_bar_and_existing_selection() {
    let root = TempWorkspace::new("document-actions");
    let mut app = App::new(root.workspace()).unwrap();
    let (mut ui, _, _) = fixture();
    let mut sources = crate::scratch::ScratchBuffers::default();
    let first = sources.create("scope-script", "1");
    sources.create("scope-script", "2");
    let actions = ui.actions(&sources, false);
    for (id, expected) in [
        ("document.open", ":scope open "),
        ("document.save", ":scope save"),
        ("document.ask", ":scope document-ask "),
        ("document.propose", ":scope document-propose "),
    ] {
        let action = actions.iter().find(|a| a.id == id).unwrap().action.clone();
        ui.invoke_action(action, &mut app, &mut sources, None)
            .unwrap();
        assert_eq!(ui.command_line.as_deref(), Some(expected));
    }
    ui.invoke_action(
        WorkspaceAction::SelectDocument(first),
        &mut app,
        &mut sources,
        None,
    )
    .unwrap();
    assert_eq!(sources.selected().unwrap().id(), first);
    assert_eq!(sources.selected().unwrap().text(), "1");
}

#[test]
fn document_pending_is_scoped_to_selected_committed_revision_and_visible() {
    let root = TempWorkspace::new("document-pending");
    let app = App::new(root.workspace()).unwrap();
    let (mut ui, _, _) = fixture();
    let mut sources = crate::scratch::ScratchBuffers::default();
    assert!(
        ui.document_pending(":scope document-ask explain", &sources)
            .is_none()
    );
    sources.create("scope-script", "old");
    let selected = sources.create("scope-script", "current");
    for name in ["document-ask", "document-propose"] {
        let status = ui
            .document_pending(&format!(":scope {name} explain"), &sources)
            .unwrap();
        assert!(status.contains(&format!("source {selected:?}, revision 0 (committed)")));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 20)).unwrap();
        terminal
            .draw(|frame| ui.render_document_pending(frame, &app, Some(&sources), &status))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains(&format!("Pending {name}")));
        assert!(text.contains("UI waits synchronously"));
        assert!(text.contains("input and cancellation unavailable"));
        assert!(text.contains("No progress updates"));
        assert_eq!(sources.selected().unwrap().text(), "current");
    }
    for line in [
        ":scope document-ask-extra x",
        ":scope ask x",
        ":scope save",
        "help",
    ] {
        assert!(ui.document_pending(line, &sources).is_none(), "{line}");
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
    assert_eq!(
        a.borrow().operations.as_slice(),
        std::slice::from_ref(&original)
    );

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
fn new_script_focuses_visible_authorized_editor_without_binding_or_running() {
    let root = TempWorkspace::new("new-script-focus");
    let mut h = EditableWorkspace::new(&root, true);
    let bound = h.sources.bound_source();
    h.ui.focus_named("observer").unwrap();
    h.ui.invoke_action(WorkspaceAction::NewScript, &mut h.app, &mut h.sources, None)
        .unwrap();
    assert_eq!(h.ui.focused(), Some("editor"));
    assert_eq!(h.sources.selected().unwrap().language_id(), "scope-script");
    assert_eq!(h.sources.selected().unwrap().text(), "");
    assert_eq!(h.sources.bound_source(), bound);
    assert!(h.ui.results.is_empty());
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

#[cfg(unix)]
#[test]
fn save_as_new_file_refuses_selected_and_parked_dirty_drafts() {
    for parked in [false, true] {
        let root = TempWorkspace::new("save-as-draft-guard");
        let mut h = EditableWorkspace::new(&root, true);
        h.event(Event::Paste("unpublished".into()));
        if parked {
            h.sources.create("text", "other committed");
            h.render();
        }
        let selected = h.sources.selected().unwrap().id();
        let committed = h.sources.selected().unwrap().text().to_owned();
        h.ui.submit_command(
            ":scope save-as new.txt",
            &mut h.app,
            &mut h.session,
            None,
            &mut h.sources,
        );
        assert!(!root.host_path_exists("new.txt"));
        assert!(!h.ui.output_errors.is_empty());
        assert_eq!(h.sources.selected().unwrap().id(), selected);
        assert_eq!(h.sources.selected().unwrap().text(), committed);
        assert!(h.sources.selected().unwrap().destination_path().is_none());
    }
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
    assert!(
        h.ui.actions(&h.sources, true)
            .iter()
            .find(|action| action.action == WorkspaceAction::RunScript)
            .unwrap()
            .disabled_reason
            .is_none()
    );
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

struct MenuLanguage(Vec<scope_language::SemanticOperationDescription>);
impl scope_language::LanguageService for MenuLanguage {
    fn language_id(&self) -> &'static str {
        "menu-fixture"
    }
    fn language_name(&self) -> &'static str {
        "Menu fixture"
    }
    fn matches_path(&self, _: &scope_language::LanguagePath<'_>) -> bool {
        true
    }
    fn analyze(&self, _: scope_language::LanguageDocument<'_>) -> scope_language::LanguageAnalysis {
        scope_language::LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<scope_language::SemanticOperationDescription> {
        self.0.clone()
    }
}

#[test]
fn primary_menu_discloses_only_contextual_actions_and_keeps_advanced_searchable() {
    use scope_language::*;
    let root = TempWorkspace::new("primary-menu");
    root.write_file("program.fixture", "program");
    let candidate = SemanticOperationDescription {
        id: "evaluate".into(),
        description: "Evaluate".into(),
        arguments: vec![],
        effect: SemanticOperationEffect::Read,
        target: SemanticTargetRequirement::Document,
        constraints: None,
    }
    .with_purpose(SemanticOperationPurpose::CandidateEvaluation)
    .unwrap();
    let exploration = candidate
        .clone()
        .with_purpose(SemanticOperationPurpose::Exploration)
        .unwrap();
    let mut argument_candidate = candidate.clone();
    argument_candidate.arguments.push(SemanticArgument {
        name: "input".into(),
        kind: SemanticArgumentKind::String,
        required: false,
        description: "Input".into(),
    });
    let mut proposal = candidate.clone();
    proposal.effect = SemanticOperationEffect::ProposeChanges;
    let mut duplicate = candidate.clone();
    duplicate.id = "second".into();
    for (operations, runnable) in [
        (vec![], false),
        (vec![exploration], false),
        (vec![argument_candidate], false),
        (vec![proposal], false),
        (vec![candidate.clone(), duplicate], false),
        (vec![candidate], true),
    ] {
        let service = Box::leak(Box::new(MenuLanguage(operations)));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        let mut ui = WorkspaceTui::new();
        ui.register_editable("editor", crate::tui::EditableScratchPane::new())
            .unwrap();
        ui.layout("main", View::leaf("editor")).unwrap();
        ui.geometry(Rect::new(0, 0, 80, 20));
        let mut sources = crate::scratch::ScratchBuffers::default();
        ui.refresh_action_context(&app);
        ui.picker = Some(actions::Picker::default());
        let labels = |ui: &WorkspaceTui, sources: &crate::scratch::ScratchBuffers| {
            ui.picker_items(sources, true)
                .into_iter()
                .map(|item| item.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(&ui, &sources), ["Open", "More actions"]);
        ui.open_app_document(&mut app, "program.fixture").unwrap();
        let expected = if runnable {
            vec!["Run", "Check", "Open", "Save", "Inspect", "More actions"]
        } else {
            vec!["Check", "Open", "Save", "Inspect", "More actions"]
        };
        assert_eq!(labels(&ui, &sources), expected);
        assert_eq!(
            ui.actions(&sources, true)
                .iter()
                .any(|item| item.action == WorkspaceAction::RunCurrentDocument),
            runnable
        );
        assert_eq!(
            ui.primary_f5_action(event::KeyModifiers::NONE, &sources),
            runnable.then_some(WorkspaceAction::RunCurrentDocument)
        );
        // Empty-query More exposes the catalog, and search does so without an extra step.
        ui.invoke_action(WorkspaceAction::MoreActions, &mut app, &mut sources, None)
            .unwrap();
        assert!(
            ui.picker_items(&sources, true)
                .iter()
                .any(|item| item.action == WorkspaceAction::NewScript)
        );
        ui.picker = Some(actions::Picker::default());
        for ch in "scratch.new-script".chars() {
            ui.picker_event(
                &Event::Key(KeyEvent::new(KeyCode::Char(ch), event::KeyModifiers::NONE)),
                &sources,
                true,
            );
        }
        assert_eq!(ui.picker_items(&sources, true).len(), 1);
        assert_eq!(
            ui.picker_items(&sources, true)[0].action,
            WorkspaceAction::NewScript
        );
        sources.create("other-language", "not executable");
        ui.app_editor = false;
        ui.picker = Some(actions::Picker::default());
        assert_eq!(labels(&ui, &sources), ["Open", "More actions"]);
        assert_eq!(
            ui.primary_f5_action(event::KeyModifiers::NONE, &sources),
            None
        );
        sources.create("scope-script", "1");
        assert_eq!(labels(&ui, &sources), ["Run", "Open", "More actions"]);
        assert_eq!(
            ui.primary_f5_action(event::KeyModifiers::NONE, &sources),
            Some(WorkspaceAction::RunScript)
        );
        assert!(
            !ui.picker_items(&sources, false)
                .iter()
                .any(|item| item.label == "Run")
        );
        assert_eq!(
            ui.primary_f5_action(event::KeyModifiers::SHIFT, &sources),
            None
        );
        // A previous availability hint cannot authorize a different/missing App source.
        let mut no_document = App::new(root.workspace()).unwrap();
        ui.app_editor = true;
        assert!(
            ui.invoke_action(
                WorkspaceAction::RunCurrentDocument,
                &mut no_document,
                &mut sources,
                None
            )
            .is_err()
        );
        assert_eq!(labels(&ui, &sources), ["Open", "More actions"]);
        assert!(ui.semantic_inspection.is_none());
        assert_eq!(sources.selected().unwrap().text(), "1");
        assert_eq!(root.read_file("program.fixture"), "program");
    }
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
    prompt: bool,
    #[expect(
        clippy::type_complexity,
        reason = "Record downstream invocation arguments directly in this test fixture"
    )]
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
    fn downstream_action_prompt(
        &mut self,
        _: &str,
        _: &crate::scratch::ScratchBuffer,
    ) -> Result<Option<DownstreamPrompt>, String> {
        Ok(self.prompt.then(|| DownstreamPrompt {
            label: "Enter candidate".into(),
            choices: vec![
                DownstreamPromptChoice {
                    label: "First goal".into(),
                    args: vec!["goal-1".into()],
                },
                DownstreamPromptChoice {
                    label: "Second goal".into(),
                    args: vec!["goal-2".into()],
                },
            ],
        }))
    }

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

#[test]
fn downstream_prompt_selects_goal_and_preserves_exact_text() {
    let root = TempWorkspace::new("downstream-prompt-exact");
    let mut h = EditableWorkspace::new(&root, true);
    h.ui.register_downstream_action(downstream_test_action())
        .unwrap();
    let mut host = DownstreamActionHost {
        prompt: true,
        ..Default::default()
    };
    h.ui.invoke_action(
        discovered_downstream(&h).action,
        &mut h.app,
        &mut h.sources,
        Some(&mut host),
    )
    .unwrap();
    assert!(host.calls.is_empty());
    h.ui.command_event(&Event::Key(KeyEvent::new(
        KeyCode::Down,
        event::KeyModifiers::NONE,
    )));
    assert_eq!(h.ui.pending_prompt.as_ref().unwrap().selected, 1);
    let text = "  lambda x => x\n \"quoted\" \\  ";
    h.ui.command_event(&Event::Paste(text.into()));
    let line =
        h.ui.command_event(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            event::KeyModifiers::NONE,
        )))
        .unwrap();
    h.ui.submit_prompt(line, &mut h.sources, Some(&mut host))
        .unwrap();
    assert_eq!(
        host.calls[0].2,
        vec!["summary", "--readonly", "goal-2", text]
    );
    assert!(h.ui.pending_prompt.is_none());
}

#[test]
fn downstream_prompt_cancel_empty_dirty_and_stale_never_dispatch() {
    for change in [
        "escape",
        "control-c",
        "empty",
        "dirty",
        "revision",
        "selection",
        "closure",
    ] {
        let root = TempWorkspace::new("downstream-prompt-guard");
        let mut h = EditableWorkspace::new(&root, true);
        h.render();
        h.ui.register_downstream_action(downstream_test_action())
            .unwrap();
        let mut host = DownstreamActionHost {
            prompt: true,
            ..Default::default()
        };
        h.ui.invoke_action(
            discovered_downstream(&h).action,
            &mut h.app,
            &mut h.sources,
            Some(&mut host),
        )
        .unwrap();
        let bound = h.sources.semantic_target_id();
        match change {
            "escape" | "control-c" => {
                let (code, modifiers) = if change == "escape" {
                    (KeyCode::Esc, event::KeyModifiers::NONE)
                } else {
                    (KeyCode::Char('c'), event::KeyModifiers::CONTROL)
                };
                h.ui.command_event(&Event::Key(KeyEvent::new(code, modifiers)));
                assert!(h.ui.pending_prompt.is_none());
                assert!(h.ui.command_line.is_none());
            }
            _ => {
                match change {
                    "dirty" => {
                        h.event(Event::Paste("draft".into()));
                    }
                    "revision" => {
                        h.event(Event::Paste("new".into()));
                        h.control('s');
                    }
                    "selection" => {
                        h.sources.create("text", "another");
                    }
                    "closure" => {
                        h.sources.close_selected(true).unwrap();
                    }
                    _ => {}
                }
                let text = if change == "empty" {
                    " \n "
                } else {
                    "candidate"
                };
                assert!(
                    h.ui.submit_prompt(text.into(), &mut h.sources, Some(&mut host))
                        .is_err(),
                    "{change}"
                );
            }
        }
        assert!(host.calls.is_empty(), "{change}");
        assert_eq!(h.sources.semantic_target_id(), bound, "{change}");
    }
}

#[test]
fn downstream_prompt_footer_reserves_wrapped_candidate_space() {
    use ratatui::{Terminal, backend::TestBackend};
    for width in [1, 20, 80, 120] {
        let mut ui = WorkspaceTui::new();
        assert_eq!(ui.footer_height(), 1);
        ui.pending_prompt = Some(actions::PendingPrompt {
            action: WorkspaceAction::Help,
            prompt: DownstreamPrompt {
                label: "Long contextual label ".repeat(40),
                choices: vec![DownstreamPromptChoice {
                    label: "Goal context ".repeat(40),
                    args: vec![],
                }],
            },
            selected: 0,
        });
        assert_eq!(ui.footer_height(), 4);
        ui.command_line = Some(format!("{}\nTAIL", "proof ".repeat(80)));
        let mut terminal = Terminal::new(TestBackend::new(width, 4)).unwrap();
        terminal
            .draw(|frame| ui.render_footer(frame, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let candidate = (2..4)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| buffer[(x, y)].symbol())
            .collect::<String>();
        if width >= 20 {
            assert!(candidate.contains("TAIL"), "{width}: {candidate}");
        }
        assert!(!candidate.contains("context"));
        // Resizing to a tiny viewport must neither panic nor let metadata take input rows.
        let mut tiny = Terminal::new(TestBackend::new(width, 1)).unwrap();
        tiny.draw(|frame| ui.render_footer(frame, frame.area()))
            .unwrap();
        ui.pending_prompt = None;
        ui.command_line = None;
        assert_eq!(ui.footer_height(), 1);
    }
}

/// A deterministic authorized host: no backend or live agent is involved.
#[derive(Default)]
struct ProposalHandoffHost {
    calls: usize,
}
impl ReadOnlyCommand for ProposalHandoffHost {
    fn invoke_downstream_with_proposals(
        &mut self,
        _: &str,
        _: &str,
        _: &[&str],
        target: Option<&crate::scratch::ScratchBuffer>,
        proposals: &mut crate::source_edit::SourceEditController,
        output: &mut dyn std::io::Write,
    ) -> Result<crate::cli::DownstreamOutcome, crate::cli::DownstreamError> {
        self.calls += 1;
        let id = proposals
            .propose(
                target.unwrap(),
                vec![crate::text::TextEdit::insert(
                    crate::text::Position::new(0, 0),
                    "proof ",
                )],
                Some("authorized test proposal".into()),
            )
            .unwrap();
        writeln!(
            output,
            "Review with :scope source-proposal {}; accept with :scope source-accept {}",
            id.0, id.0
        )
        .unwrap();
        Ok(crate::cli::DownstreamOutcome::Handled)
    }
    fn invoke(
        &mut self,
        _: &crate::document::Document,
        _: crate::text::TextRange,
        _: &str,
        _: &mut dyn std::io::Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("no document authority")
    }
}

#[test]
fn downstream_proposal_handoff_retains_pending_for_explicit_review_and_accept() {
    use crate::proposal::{ProposalId, ProposalStatus};
    let root = TempWorkspace::new("downstream-proposal-handoff");
    let mut h = EditableWorkspace::new(&root, true);
    h.ui.register_downstream_action(downstream_test_action())
        .unwrap();
    let action = discovered_downstream(&h).action;
    let original = h.sources.bound_source();
    let mut host = ProposalHandoffHost::default();
    h.ui.invoke_action(action, &mut h.app, &mut h.sources, Some(&mut host))
        .unwrap();
    assert_eq!(host.calls, 1);
    assert_eq!(h.sources.bound_source(), original);
    let controller = h.ui.source_edit_controller();
    assert_eq!(
        controller.get(ProposalId(1)).unwrap().status(),
        ProposalStatus::Pending
    );
    assert_eq!(
        controller
            .preview(ProposalId(1), h.sources.selected().unwrap())
            .unwrap(),
        "proof original"
    );
    h.ui.submit_command(
        ":scope source-proposal 1",
        &mut h.app,
        &mut h.session,
        None,
        &mut h.sources,
    );
    assert_eq!(h.sources.bound_source(), original);
    h.ui.submit_command(
        ":scope source-accept 1",
        &mut h.app,
        &mut h.session,
        None,
        &mut h.sources,
    );
    assert_eq!(h.sources.selected().unwrap().text(), "proof original");
    assert_eq!(
        h.ui.source_edit_controller()
            .get(ProposalId(1))
            .unwrap()
            .status(),
        ProposalStatus::Applied
    );
}

#[test]
fn downstream_proposal_handoff_blocks_dirty_drafts_and_stale_acceptance() {
    use crate::proposal::{ProposalId, ProposalStatus};
    let root = TempWorkspace::new("downstream-proposal-freshness");
    let mut h = EditableWorkspace::new(&root, true);
    h.ui.register_downstream_action(downstream_test_action())
        .unwrap();
    let action = discovered_downstream(&h).action;
    let mut host = ProposalHandoffHost::default();
    h.event(Event::Paste("draft ".into()));
    assert!(
        h.ui.invoke_action(action, &mut h.app, &mut h.sources, Some(&mut host))
            .is_err()
    );
    assert_eq!(host.calls, 0);
    assert!(h.ui.source_edit_controller().list().next().is_none());
    h.render();
    assert!(h.pane_text("editor").contains("draft original"));
    h.control('s');
    let action = discovered_downstream(&h).action;
    h.ui.invoke_action(action, &mut h.app, &mut h.sources, Some(&mut host))
        .unwrap();
    h.event(Event::Paste("new ".into()));
    h.control('s');
    let current = h.sources.bound_source();
    h.ui.submit_command(
        ":scope source-accept 1",
        &mut h.app,
        &mut h.session,
        None,
        &mut h.sources,
    );
    assert_eq!(h.sources.bound_source(), current);
    assert_eq!(
        h.ui.source_edit_controller()
            .get(ProposalId(1))
            .unwrap()
            .status(),
        ProposalStatus::Pending
    );
    assert!(
        h.ui.source_edit_controller()
            .preview(ProposalId(1), h.sources.selected().unwrap())
            .is_err()
    );
}

struct DocumentRequester(String, Rc<Cell<usize>>);
impl Pane for DocumentRequester {
    fn id(&self) -> &'static str {
        "document-requester"
    }
    fn render(&self, _: &mut ratatui::Frame<'_>, _: Rect, _: &TuiContext<'_>) {}
    fn workspace_document_request(&mut self, _: &Event, ctx: &TuiContext<'_>) -> Option<String> {
        assert!(ctx.is_focused());
        self.1.set(self.1.get() + 1);
        Some(self.0.clone())
    }
    fn handle_source_event(&mut self, _: Event, _: &mut crate::tui::ScratchEditAccess<'_>) -> bool {
        panic!("open must not invoke editing");
    }
    fn handle_key(&mut self, _: KeyEvent, _: &mut TuiContextMut<'_>) -> bool {
        panic!("open must not invoke mutable callback");
    }
}
fn document_requester(h: &mut EditableWorkspace, path: &str) -> Rc<Cell<usize>> {
    let calls = Rc::new(Cell::new(0));
    h.ui.register_editable("requester", DocumentRequester(path.into(), calls.clone()))
        .unwrap();
    h.ui.layout(
        "requests",
        View::horizontal([View::leaf("editor"), View::leaf("requester")]),
    )
    .unwrap();
    h.ui.select_layout("requests").unwrap();
    h.render();
    calls
}
#[test]
fn workspace_document_request_preserves_selected_and_parked_drafts() {
    for parked in [false, true] {
        let root = TempWorkspace::new("document-request-drafts");
        root.write_file("target with spaces.txt", "opened source");
        let mut h = EditableWorkspace::new(&root, true);
        let original = h.sources.selected().unwrap().id();
        h.event(Event::Paste("draft ".into()));
        if parked {
            h.sources.create("text", "other");
            h.render();
        }
        let calls = document_requester(&mut h, "target with spaces.txt");
        h.key(KeyCode::Left, event::KeyModifiers::NONE);
        assert_eq!(calls.get(), 0);
        h.ui.focus_named("requester").unwrap();
        h.key(KeyCode::F(4), event::KeyModifiers::NONE);
        assert_eq!(calls.get(), 0);
        h.ui.command_line = None;
        let binding = h.sources.bound_source();
        assert_eq!(
            h.key(KeyCode::Enter, event::KeyModifiers::NONE),
            WorkspaceInput::Handled
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(h.ui.focused(), Some("editor"));
        assert_eq!(h.sources.selected().unwrap().text(), "opened source");
        assert_eq!(h.sources.bound_source(), binding);
        assert_eq!(h.sources.get(original).unwrap().text(), "original");
        assert!(h.ui.draft_blocker().is_some());
        h.sources.select(original).unwrap();
        h.render();
        assert!(h.pane_text("editor").contains("draft "));
        h.control('s');
        assert!(h.sources.selected().unwrap().text().starts_with("draft "));
        assert_eq!(h.ui.draft_blocker(), None);
        assert!(h.ui.output_errors.is_empty());
    }
}
#[test]
fn workspace_document_request_errors_never_dispatch_commands() {
    for parked in [false, true] {
        for path in [
            "missing.txt",
            "../escape.txt",
            "/absolute.txt",
            ":scope new text\n:scope save-as injected.txt",
        ] {
            let root = TempWorkspace::new("document-request-errors");
            let mut h = EditableWorkspace::new(&root, true);
            let draft_id = h.sources.selected().unwrap().id();
            h.event(Event::Paste("draft ".into()));
            if parked {
                h.sources.create("text", "other committed source");
                h.render();
            }
            let selected = h.sources.selected().unwrap().id();
            let before: Vec<_> = h
                .sources
                .list()
                .map(|b| {
                    (
                        b.id(),
                        b.evaluation_source_id().clone(),
                        b.version(),
                        b.text().to_owned(),
                        b.is_dirty(),
                    )
                })
                .collect();
            let binding = h.sources.bound_source();
            let blocker = h.ui.draft_blocker();
            assert!(blocker.is_some());
            document_requester(&mut h, path);
            h.ui.focus_named("requester").unwrap();
            assert_eq!(
                h.key(KeyCode::Enter, event::KeyModifiers::NONE),
                WorkspaceInput::Handled
            );
            assert_eq!(h.ui.focused(), Some("requester"));
            assert_eq!(h.sources.selected().unwrap().id(), selected);
            let after: Vec<_> = h
                .sources
                .list()
                .map(|b| {
                    (
                        b.id(),
                        b.evaluation_source_id().clone(),
                        b.version(),
                        b.text().to_owned(),
                        b.is_dirty(),
                    )
                })
                .collect();
            assert_eq!(after, before, "{path}, parked={parked}");
            assert_eq!(h.sources.bound_source(), binding);
            assert_eq!(h.ui.draft_blocker(), blocker);
            assert!(!h.ui.output_errors.is_empty(), "{path}");
            assert!(!root.host_path_exists("injected.txt"));
            // Recover the exact unpublished draft, not merely its dirty marker.
            h.sources.select(draft_id).unwrap();
            h.ui.focus_named("editor").unwrap();
            h.render();
            assert!(h.pane_text("editor").contains("draft original"));
            h.control('s');
            assert_eq!(h.sources.selected().unwrap().text(), "draft original");
            assert_eq!(h.ui.draft_blocker(), None);
        }
    }
}

#[test]
fn workspace_document_request_reuses_destination_without_rereading_or_losing_draft() {
    let root = TempWorkspace::new("document-request-repeat");
    root.write_file("target.txt", "disk source");
    let mut h = EditableWorkspace::new(&root, true);
    document_requester(&mut h, "target.txt");
    h.ui.focus_named("requester").unwrap();
    h.key(KeyCode::Enter, event::KeyModifiers::NONE);
    let id = h.sources.selected().unwrap().id();
    let identity = h.sources.selected().unwrap().evaluation_source_id().clone();
    h.render();
    h.event(Event::Paste("draft ".into()));
    h.render();
    let draft_text = h.pane_text("editor");
    let version = h.sources.selected().unwrap().version();
    let count = h.sources.list().count();
    let blocker = h.ui.draft_blocker();
    assert!(blocker.is_some());
    root.write_file("target.txt", "changed disk source must not be reread");
    for parked in [false, true] {
        if parked {
            h.sources.create("text", "other");
            h.render();
        }
        let count = if parked { count + 1 } else { count };
        h.ui.focus_named("requester").unwrap();
        h.key(KeyCode::Enter, event::KeyModifiers::NONE);
        assert_eq!(h.ui.focused(), Some("editor"));
        let buffer = h.sources.selected().unwrap();
        assert_eq!(buffer.id(), id);
        assert_eq!(buffer.evaluation_source_id(), &identity);
        assert_eq!(buffer.version(), version);
        assert_eq!(buffer.text(), "disk source");
        assert_eq!(h.sources.list().count(), count);
        assert_eq!(h.ui.draft_blocker(), blocker);
        h.render();
        assert_eq!(h.pane_text("editor"), draft_text);
    }
    h.control('s');
    assert_eq!(h.sources.selected().unwrap().text(), "draft disk source");
    assert_eq!(h.ui.draft_blocker(), None);
    assert!(h.ui.output_errors.is_empty());
}

fn path_f1_workspace(root: &TempWorkspace) -> EditableWorkspace {
    let mut h = EditableWorkspace::new(root, true);
    let id = h.sources.create("language.path", "original");
    h.sources.select(id).unwrap();
    let mut action = downstream_test_action();
    action.language = "language.path".into();
    h.ui.register_downstream_action(action).unwrap();
    h.render();
    h
}

fn path_f1_propose(h: &mut EditableWorkspace) {
    let action = discovered_downstream(h).action;
    h.ui.invoke_action(
        action,
        &mut h.app,
        &mut h.sources,
        Some(&mut ProposalHandoffHost::default()),
    )
    .unwrap();
}

fn path_f1_actions(h: &EditableWorkspace) -> Vec<ActionInfo> {
    h.ui.actions(&h.sources, false)
        .into_iter()
        .filter(|item| item.id.starts_with("path."))
        .collect()
}

#[test]
fn path_f1_unique_pending_prefills_exact_commands_without_mutation_until_submission() {
    use crate::proposal::{ProposalId, ProposalStatus};
    let root = TempWorkspace::new("path-f1-prefill");
    let mut h = path_f1_workspace(&root);
    path_f1_propose(&mut h);
    let original = h.sources.bound_source();
    let actions = path_f1_actions(&h);
    assert_eq!(actions.len(), 4);
    for (item, (id, command)) in actions.into_iter().zip([
        ("path.review", "source-proposal"),
        ("path.diff", "source-diff"),
        ("path.accept", "source-accept"),
        ("path.reject", "source-reject"),
    ]) {
        assert_eq!(item.id, id);
        assert_eq!(item.disabled_reason, None);
        let command = format!(":scope {command} 1");
        assert_eq!(item.action, WorkspaceAction::CommandPrompt(command.clone()));
        h.ui.invoke_action(item.action, &mut h.app, &mut h.sources, None)
            .unwrap();
        assert_eq!(h.ui.command_line.as_deref(), Some(command.as_str()));
        assert_eq!(h.sources.bound_source(), original);
        assert_eq!(
            h.ui.source_edit_controller()
                .get(ProposalId(1))
                .unwrap()
                .status(),
            ProposalStatus::Pending
        );
    }
    h.ui.submit_command(
        ":scope source-accept 1",
        &mut h.app,
        &mut h.session,
        None,
        &mut h.sources,
    );
    assert_eq!(h.sources.selected().unwrap().text(), "proof original");
    assert_eq!(
        h.ui.source_edit_controller()
            .get(ProposalId(1))
            .unwrap()
            .status(),
        ProposalStatus::Applied
    );
    assert!(path_f1_actions(&h).is_empty());
}

#[test]
fn path_f1_equal_text_other_scratch_has_no_actions_and_cannot_retarget_token() {
    let root = TempWorkspace::new("path-f1-identity");
    let mut h = path_f1_workspace(&root);
    path_f1_propose(&mut h);
    let token = path_f1_actions(&h)
        .into_iter()
        .find(|a| a.id == "path.accept")
        .unwrap()
        .action;
    let original_id = h.sources.selected().unwrap().id();
    let other = h.sources.create("language.path", "original");
    h.sources.select(other).unwrap();
    path_f1_propose(&mut h);
    let before = h.sources.bound_source();
    assert_eq!(path_f1_actions(&h).len(), 4);
    assert!(
        h.ui.invoke_action(token, &mut h.app, &mut h.sources, None)
            .is_err()
    );
    assert_eq!(h.sources.bound_source(), before);
    // An equal-text document without its own pending proposal has no handoffs.
    let third = h.sources.create("language.path", "original");
    h.sources.select(third).unwrap();
    assert!(path_f1_actions(&h).is_empty());
    assert_eq!(h.sources.get(original_id).unwrap().text(), "original");
    assert_eq!(h.sources.get(other).unwrap().text(), "original");
}

#[test]
fn path_f1_multiple_pending_removes_actions_and_invalidates_unique_token() {
    let root = TempWorkspace::new("path-f1-ambiguous");
    let mut h = path_f1_workspace(&root);
    path_f1_propose(&mut h);
    let token = path_f1_actions(&h)[0].action.clone();
    path_f1_propose(&mut h);
    let before = h.sources.bound_source();
    assert!(path_f1_actions(&h).is_empty());
    assert!(
        h.ui.invoke_action(token, &mut h.app, &mut h.sources, None)
            .is_err()
    );
    assert_eq!(h.sources.bound_source(), before);
    assert_eq!(h.ui.source_edit_controller().list().count(), 2);
}

#[test]
fn path_f1_repeated_prefill_and_consumed_token_never_retarget_new_pending() {
    use crate::proposal::{ProposalId, ProposalStatus};
    let root = TempWorkspace::new("path-f1-repeated");
    let mut h = path_f1_workspace(&root);
    path_f1_propose(&mut h);
    let token = path_f1_actions(&h)
        .into_iter()
        .find(|a| a.id == "path.reject")
        .unwrap()
        .action;
    let before = h.sources.bound_source();
    for _ in 0..2 {
        h.ui.invoke_action(token.clone(), &mut h.app, &mut h.sources, None)
            .unwrap();
        assert_eq!(h.ui.command_line.as_deref(), Some(":scope source-reject 1"));
        assert_eq!(h.sources.bound_source(), before);
        assert_eq!(
            h.ui.source_edit_controller()
                .get(ProposalId(1))
                .unwrap()
                .status(),
            ProposalStatus::Pending
        );
    }
    h.ui.submit_command(
        ":scope source-reject 1",
        &mut h.app,
        &mut h.session,
        None,
        &mut h.sources,
    );
    assert_eq!(
        h.ui.source_edit_controller()
            .get(ProposalId(1))
            .unwrap()
            .status(),
        ProposalStatus::Rejected
    );
    assert!(path_f1_actions(&h).is_empty());
    assert!(
        h.ui.invoke_action(token.clone(), &mut h.app, &mut h.sources, None)
            .is_err()
    );
    path_f1_propose(&mut h);
    assert_eq!(path_f1_actions(&h).len(), 4);
    assert!(
        h.ui.invoke_action(token, &mut h.app, &mut h.sources, None)
            .is_err()
    );
    assert_eq!(h.sources.bound_source(), before);
    assert_eq!(
        h.ui.source_edit_controller()
            .get(ProposalId(2))
            .unwrap()
            .status(),
        ProposalStatus::Pending
    );
}

#[test]
fn path_f1_dirty_draft_disables_all_handoffs_and_rechecks_captured_tokens() {
    use crate::proposal::{ProposalId, ProposalStatus};
    let root = TempWorkspace::new("path-f1-dirty");
    let mut h = path_f1_workspace(&root);
    path_f1_propose(&mut h);
    let tokens = path_f1_actions(&h);
    h.event(Event::Paste("draft ".into()));
    let inspect =
        h.ui.actions(&h.sources, false)
            .into_iter()
            .find(|a| a.action == WorkspaceAction::InspectDocument)
            .unwrap();
    assert!(inspect.disabled_reason.is_some());
    assert!(
        h.ui.invoke_action(inspect.action, &mut h.app, &mut h.sources, None)
            .is_err()
    );
    assert!(h.ui.semantic_inspection.is_none());
    let diagnostics =
        h.ui.actions(&h.sources, false)
            .into_iter()
            .find(|a| a.id == "document.diagnostics")
            .unwrap();
    assert_eq!(diagnostics.label, "Check document diagnostics");
    assert!(diagnostics.disabled_reason.is_some());
    assert!(
        h.ui.invoke_action(diagnostics.action, &mut h.app, &mut h.sources, None)
            .is_err()
    );
    let before = h.sources.bound_source();
    let disabled = path_f1_actions(&h);
    assert_eq!(disabled.len(), 4);
    assert!(disabled.iter().all(|a| {
        a.disabled_reason
            .as_deref()
            .is_some_and(|r| r.contains("draft"))
    }));
    for item in tokens {
        assert!(
            h.ui.invoke_action(item.action, &mut h.app, &mut h.sources, None)
                .is_err()
        );
        assert_eq!(h.sources.bound_source(), before);
    }
    assert_eq!(
        h.ui.source_edit_controller()
            .get(ProposalId(1))
            .unwrap()
            .status(),
        ProposalStatus::Pending
    );
    h.render();
    assert!(h.pane_text("editor").contains("draft original"));
}

struct DocumentStatusProbe;
impl Pane for DocumentStatusProbe {
    fn id(&self) -> &'static str {
        "document-status-probe"
    }
    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        frame.render_widget(
            Paragraph::new(format!(
                "status={:?}; drafts={}",
                ctx.selected_document_status(),
                ctx.has_editor_drafts()
            )),
            area,
        );
    }
}

#[test]
fn read_only_document_status_reports_selected_name_and_committed_dirty_state() {
    let root = TempWorkspace::new("read-only-document-status");
    root.write_file("notes.txt", "disk source");
    let mut h = EditableWorkspace::new(&root, true);
    assert_eq!(TuiContext::new(&h.app).selected_document_status(), None);
    assert!(!TuiContext::new(&h.app).has_editor_drafts());
    let empty = ScratchBuffers::default();
    assert_eq!(
        TuiContext::with_view_sources(&h.app, false, 0, Some(&empty)).selected_document_status(),
        None
    );
    let scratch = h.sources.selected().unwrap().id();
    let file = h
        .sources
        .open(h.app.workspace(), h.app.languages(), "notes.txt")
        .unwrap();
    for dirty in [false, true] {
        if dirty {
            h.sources
                .selected_mut()
                .unwrap()
                .apply_edit(crate::text::TextEdit::insert(
                    crate::text::Position::new(0, 0),
                    "committed ",
                ))
                .unwrap();
        }
        let before = format!("{:?}", h.sources);
        let bound = h.sources.bound_source();
        root.write_file(
            "notes.txt",
            "external change must not be read or overwritten",
        );
        for _ in 0..2 {
            let ctx = TuiContext::with_view_sources(&h.app, false, 0, Some(&h.sources));
            assert_eq!(
                ctx.selected_document_status(),
                Some(("notes.txt".into(), dirty))
            );
            assert!(!ctx.has_editor_drafts());
        }
        assert_eq!(format!("{:?}", h.sources), before);
        assert_eq!(h.sources.bound_source(), bound);
        assert_eq!(h.sources.selected().unwrap().id(), file);
        assert_eq!(
            root.read_file("notes.txt"),
            "external change must not be read or overwritten"
        );
    }
    h.sources.select(scratch).unwrap();
    let before = format!("{:?}", h.sources);
    assert_eq!(
        TuiContext::with_view_sources(&h.app, false, 0, Some(&h.sources))
            .selected_document_status(),
        Some(("Untitled document".into(), true))
    );
    assert_eq!(format!("{:?}", h.sources), before);
}

#[test]
fn render_context_reports_global_visible_and_hidden_drafts_without_mutation() {
    let root = TempWorkspace::new("render-global-draft-status");
    root.write_file("selected.txt", "clean selected source");
    let mut h = EditableWorkspace::new(&root, true);
    h.ui.register("status", DocumentStatusProbe).unwrap();
    h.ui.layout(
        "status-editor",
        View::horizontal([View::leaf("editor"), View::leaf("status")]),
    )
    .unwrap();
    h.ui.layout("status-only", View::leaf("status")).unwrap();
    h.ui.select_layout("status-editor").unwrap();
    h.render();
    assert!(h.pane_text("status").contains("drafts=false"));
    h.ui.focus_named("editor").unwrap();
    let draft_id = h.sources.selected().unwrap().id();
    h.event(Event::Paste("unpublished ".into()));
    h.render();
    let draft_text = h.pane_text("editor");
    assert!(draft_text.contains("unpublished original"));
    assert!(h.pane_text("status").contains("drafts=true"));
    h.sources
        .open(h.app.workspace(), h.app.languages(), "selected.txt")
        .unwrap();
    h.render();
    for layout in ["status-editor", "status-only"] {
        h.ui.select_layout(layout).unwrap();
        let before = format!("{:?}", h.sources);
        let bound = h.sources.bound_source();
        let blocker = h.ui.draft_blocker();
        assert!(blocker.is_some());
        for _ in 0..2 {
            h.render();
            let text = h.pane_text("status");
            assert!(
                text.contains("status=Some((\"selected.txt\", false))"),
                "{text}"
            );
            assert!(text.contains("drafts=true"), "{text}");
            assert_eq!(format!("{:?}", h.sources), before);
            assert_eq!(h.sources.bound_source(), bound);
            assert_eq!(h.ui.draft_blocker(), blocker);
        }
    }
    h.sources.select(draft_id).unwrap();
    h.ui.select_layout("status-editor").unwrap();
    h.render();
    h.ui.focus_named("editor").unwrap();
    h.render();
    assert_eq!(h.pane_text("editor"), draft_text);
    assert_eq!(h.sources.selected().unwrap().text(), "original");
    assert_eq!(root.read_file("selected.txt"), "clean selected source");
    h.ui.focus_named("editor").unwrap();
    h.control('s');
    h.render();
    assert!(h.pane_text("status").contains("drafts=false"));
    assert_eq!(h.sources.selected().unwrap().text(), "unpublished original");
}

#[test]
fn inspection_action_requires_live_app_document_and_modal_captures_escape() {
    let root = TempWorkspace::new("workspace-inspection-modal");
    root.write_file("sample.txt", "saved");
    let mut app = App::new(root.workspace()).unwrap();
    let mut ui = WorkspaceTui::new();
    let mut sources = crate::scratch::ScratchBuffers::default();
    sources.create("scope-script", "unrelated scratch");
    let action = ui
        .actions(&sources, false)
        .into_iter()
        .find(|a| a.id == "document.evaluate")
        .unwrap()
        .action;
    assert_eq!(action, WorkspaceAction::InspectDocument);
    assert!(
        ui.invoke_action(action.clone(), &mut app, &mut sources, None)
            .is_err()
    );
    assert!(ui.semantic_inspection.is_none());
    app.open_path("sample.txt").unwrap();
    ui.invoke_action(action, &mut app, &mut sources, None)
        .unwrap();
    assert!(ui.semantic_inspection.is_some());
    let key = KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE);
    assert_eq!(ui.handle_key(key, &mut app), WorkspaceInput::Handled);
    assert!(ui.semantic_inspection.is_none());
    assert_eq!(ui.handle_key(key, &mut app), WorkspaceInput::Shell);
}

#[test]
fn f1_editor_focus_and_document_diagnostics_have_deterministic_labels() {
    let root = TempWorkspace::new("f1-editor-diagnostics");
    root.write_file("sample.txt", "App source");
    let mut app = App::new(root.workspace()).unwrap();
    let mut ui = WorkspaceTui::new();
    ui.register_editable("editor", crate::tui::EditableScratchPane::new())
        .unwrap();
    ui.layout("main", View::leaf("editor")).unwrap();
    ui.geometry(Rect::new(0, 0, 80, 20));
    ui.output_area = Some(Rect::new(0, 20, 80, 5));
    let mut sources = crate::scratch::ScratchBuffers::default();
    sources.create("scope-script", "unrelated scratch");
    let actions = ui.actions(&sources, false);
    let focus = actions.iter().find(|a| a.id == "focus.editor").unwrap();
    assert_eq!(focus.label, "Focus editor");
    ui.focus_named("output").unwrap();
    ui.invoke_action(focus.action.clone(), &mut app, &mut sources, None)
        .unwrap();
    assert_eq!(ui.focused(), Some("editor"));
    let check = actions
        .iter()
        .find(|a| a.id == "document.diagnostics")
        .unwrap();
    assert_eq!(check.label, "Check document diagnostics");
    assert!(
        ui.invoke_action(check.action.clone(), &mut app, &mut sources, None)
            .is_err()
    );
    app.open_path("sample.txt").unwrap();
    let before = app.open_document().unwrap().clone();
    ui.invoke_action(check.action.clone(), &mut app, &mut sources, None)
        .unwrap();
    assert!(ui.output_text().contains("App sample.txt — revision 0"));
    assert!(ui.output_text().contains("diagnostics: clean"));
    assert_eq!(ui.focused(), Some("editor"));
    assert!(ui.command_line.is_none());
    assert_eq!(app.open_document().unwrap(), &before);
    assert_eq!(sources.selected().unwrap().text(), "unrelated scratch");
}

#[test]
fn workspace_open_and_editor_publish_do_not_replace_app_document() {
    let root = TempWorkspace::new("workspace-app-ownership");
    root.write_file("app.txt", "App source");
    root.write_file("editor.txt", "editor source");
    let mut app = App::new(root.workspace()).unwrap();
    app.open_path("app.txt").unwrap();
    let before = app.open_document().unwrap().clone();
    let mut ui = WorkspaceTui::new();
    ui.register_editable("editor", crate::tui::EditableScratchPane::new())
        .unwrap();
    ui.layout("main", View::leaf("editor")).unwrap();
    ui.geometry(Rect::new(0, 0, 80, 20));
    let mut sources = crate::scratch::ScratchBuffers::default();
    let mut session = InteractiveSession::new();
    ui.submit_command(
        ":scope open editor.txt",
        &mut app,
        &mut session,
        None,
        &mut sources,
    );
    assert_eq!(sources.selected().unwrap().text(), "editor source");
    ui.handle_event_with_sources(Event::Paste("changed ".into()), &mut app, &mut sources);
    ui.handle_event_with_sources(
        Event::Key(KeyEvent::new(
            KeyCode::Char('s'),
            event::KeyModifiers::CONTROL,
        )),
        &mut app,
        &mut sources,
    );
    assert!(sources.selected().unwrap().text().contains("changed "));
    assert_eq!(app.open_document().unwrap(), &before);
}

#[test]
fn portable_check_shortcut_and_publication_help_are_explicit() {
    use event::KeyModifiers as M;
    let shortcut =
        |code, modifiers| WorkspaceTui::document_shortcut(KeyEvent::new(code, modifiers));
    assert_eq!(
        shortcut(KeyCode::Enter, M::ALT),
        Some(WorkspaceAction::CheckCurrentDocument)
    );
    assert_eq!(
        shortcut(KeyCode::Char('r'), M::CONTROL),
        Some(WorkspaceAction::RunCurrentDocument)
    );
    for (code, modifiers) in [
        (KeyCode::Char('r'), M::CONTROL | M::SHIFT),
        (KeyCode::Char('R'), M::CONTROL | M::SHIFT),
        (KeyCode::Char('t'), M::CONTROL),
        (KeyCode::Enter, M::NONE),
        (KeyCode::Enter, M::ALT | M::CONTROL),
        (KeyCode::Char('d'), M::CONTROL),
    ] {
        assert_eq!(shortcut(code, modifiers), None);
    }
    let mut ui = WorkspaceTui::new();
    let mut sources = crate::scratch::ScratchBuffers::default();
    assert_eq!(ui.primary_f5_action(M::SHIFT, &sources), None);
    ui.app_editor = true;
    assert_eq!(
        ui.primary_f5_action(M::SHIFT, &sources),
        Some(WorkspaceAction::CheckCurrentDocument)
    );
    let items = ui.actions(&sources, false);
    assert_eq!(
        items
            .iter()
            .find(|item| item.action == WorkspaceAction::CheckCurrentDocument)
            .unwrap()
            .shortcut,
        Some("Alt-Enter / Shift-F5")
    );
    let root = TempWorkspace::new("publication-help");
    let mut app = App::new(root.workspace()).unwrap();
    ui.invoke_action(WorkspaceAction::Help, &mut app, &mut sources, None)
        .unwrap();
    assert!(ui.output_errors.contains("Ctrl-S publishes in memory only"));
    assert!(
        ui.output_errors
            .contains("explicit Save action to write to disk")
    );
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 2)).unwrap();
    terminal
        .draw(|frame| ui.render_footer(frame, frame.area()))
        .unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(screen.contains("Ctrl-O Open · Ctrl-R Run · Ctrl-Q Quit · Ctrl-G More"));
    assert!(!screen.contains("Alt-Enter check"));
    assert!(!screen.contains("Ctrl-S publish (memory)"));
}

#[test]
fn current_draft_check_publishes_in_memory_and_rejects_stale_baseline() {
    let root = TempWorkspace::new("current-draft-check");
    root.write_file("document.txt", "baseline");
    let mut app = App::new(root.workspace()).unwrap();
    let mut ui = WorkspaceTui::new();
    ui.register_editable("editor", crate::tui::EditableScratchPane::new())
        .unwrap();
    ui.layout("main", View::leaf("editor")).unwrap();
    ui.geometry(Rect::new(0, 0, 80, 20));
    let mut sources = crate::scratch::ScratchBuffers::default();
    ui.open_app_document(&mut app, "document.txt").unwrap();
    ui.handle_event_with_sources(Event::Paste("draft ".into()), &mut app, &mut sources);
    assert_eq!(
        ui.handle_event_with_sources(
            Event::Key(KeyEvent::new(
                KeyCode::Char('d'),
                event::KeyModifiers::CONTROL
            )),
            &mut app,
            &mut sources
        ),
        WorkspaceInput::Handled
    );
    assert!(ui.draft_blocker().is_some());
    assert_eq!(
        ui.handle_event_with_sources(
            Event::Key(KeyEvent::new(KeyCode::F(2), event::KeyModifiers::NONE)),
            &mut app,
            &mut sources
        ),
        WorkspaceInput::Shell
    );
    assert!(ui.draft_blocker().is_some());
    ui.invoke_action(
        WorkspaceTui::document_shortcut(KeyEvent::new(KeyCode::Enter, event::KeyModifiers::ALT))
            .unwrap(),
        &mut app,
        &mut sources,
        None,
    )
    .unwrap();
    assert_eq!(app.open_document().unwrap().text(), "draft baseline");
    assert!(ui.draft_blocker().is_none());
    assert_eq!(root.read_file("document.txt"), "baseline");
    assert!(sources.selected().is_err());
    ui.handle_event_with_sources(Event::Paste("parked ".into()), &mut app, &mut sources);
    let document = app.open_document_mut().unwrap();
    let range = document
        .buffer()
        .range_for_span(crate::text::Span::new(0, 0))
        .unwrap();
    document
        .apply_edit(crate::text::TextEdit::replace(range, "external "))
        .unwrap();
    let before = app.open_document().unwrap().clone();
    for action in [
        WorkspaceAction::CheckCurrentDocument,
        WorkspaceAction::RunCurrentDocument,
    ] {
        assert!(
            ui.invoke_action(action, &mut app, &mut sources, None)
                .is_err()
        );
        assert_eq!(app.open_document().unwrap(), &before);
        assert!(ui.semantic_inspection.is_none());
        assert!(ui.draft_blocker().is_some());
    }
    assert_eq!(root.read_file("document.txt"), "baseline");
}

#[test]
fn primary_run_never_falls_back_to_selected_script() {
    let root = TempWorkspace::new("primary-run-no-fallback");
    let mut app = App::new(root.workspace()).unwrap();
    let mut ui = WorkspaceTui::new();
    let mut sources = crate::scratch::ScratchBuffers::default();
    sources.create("scope-script", "1");
    assert!(
        ui.invoke_action(
            WorkspaceAction::RunCurrentDocument,
            &mut app,
            &mut sources,
            None
        )
        .is_err()
    );
    assert!(ui.results.is_empty());
    assert!(ui.semantic_inspection.is_none());
}

#[test]
fn open_document_suggestions_spaces_ambiguity_navigation_and_no_matches() {
    let root = TempWorkspace::new("open-suggestions");
    root.write_file("a file.txt", "first");
    root.write_file("b file.txt", "second");
    let mut app = App::new(root.workspace()).unwrap();
    // Suggestions must use the authorized snapshot, not rescan the filesystem.
    root.write_file("c file.txt", "not in snapshot");
    let mut ui = WorkspaceTui::new();
    ui.register_editable("editor", crate::tui::EditableScratchPane::new())
        .unwrap();
    ui.layout("main", View::leaf("editor")).unwrap();
    ui.geometry(Rect::new(0, 0, 80, 20));
    let mut sources = crate::scratch::ScratchBuffers::default();
    let mut session = InteractiveSession::new();
    ui.invoke_action(
        WorkspaceAction::OpenAppDocument,
        &mut app,
        &mut sources,
        None,
    )
    .unwrap();
    let key = |code| Event::Key(KeyEvent::new(code, event::KeyModifiers::NONE));
    ui.open_document_event(&Event::Paste("file".into()), &app);
    for code in [
        KeyCode::End,
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::Home,
        KeyCode::Down,
    ] {
        assert!(ui.open_document_event(&key(code), &app).is_none());
    }
    assert!(ui.open_document_event(&key(KeyCode::Tab), &app).is_none());
    assert_eq!(ui.command_line.as_deref(), Some("b file.txt"));
    assert!(app.open_document().is_none());
    let path = ui.open_document_event(&key(KeyCode::Enter), &app).unwrap();
    ui.submit_command(&path, &mut app, &mut session, None, &mut sources);
    assert_eq!(app.open_document().unwrap().text(), "second");
    ui.invoke_action(
        WorkspaceAction::OpenAppDocument,
        &mut app,
        &mut sources,
        None,
    )
    .unwrap();
    ui.open_document_event(&Event::Paste("missing".into()), &app);
    for code in [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Tab,
        KeyCode::Enter,
    ] {
        assert!(ui.open_document_event(&key(code), &app).is_none());
        assert_eq!(ui.command_line.as_deref(), Some("missing"));
    }
    assert!(ui.open_app_prompt);
    ui.open_document_event(&key(KeyCode::Esc), &app);
    assert!(!ui.open_app_prompt);
    assert_eq!(app.open_document().unwrap().text(), "second");
}

#[test]
fn f1_app_open_prompt_uses_literal_authorized_path_and_cancel_is_inert() {
    let root = TempWorkspace::new("app-open-prompt");
    root.write_file("file with spaces.txt", "App file");
    let mut app = App::new(root.workspace()).unwrap();
    let mut ui = WorkspaceTui::new();
    ui.register_editable("editor", crate::tui::EditableScratchPane::new())
        .unwrap();
    ui.layout("main", View::leaf("editor")).unwrap();
    ui.geometry(Rect::new(0, 0, 80, 20));
    let mut sources = crate::scratch::ScratchBuffers::default();
    let mut session = InteractiveSession::new();
    let action = ui
        .actions(&sources, false)
        .into_iter()
        .find(|a| a.id == "document.open-app")
        .unwrap();
    assert_eq!(action.label, "Open");
    ui.invoke_action(action.action.clone(), &mut app, &mut sources, None)
        .unwrap();
    assert!(ui.command_line.as_ref().unwrap().is_empty());
    assert!(
        ui.command_event(&Event::Key(KeyEvent::new(
            KeyCode::Esc,
            event::KeyModifiers::NONE
        )))
        .is_none()
    );
    assert!(!ui.open_app_prompt);
    assert!(app.open_document().is_none());
    ui.invoke_action(action.action.clone(), &mut app, &mut sources, None)
        .unwrap();
    ui.command_event(&Event::Paste("file with spaces.txt".into()));
    let path = ui
        .command_event(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            event::KeyModifiers::NONE,
        )))
        .unwrap();
    ui.submit_command(&path, &mut app, &mut session, None, &mut sources);
    assert_eq!(app.open_document().unwrap().text(), "App file");
    assert!(ui.app_editor);
    assert_eq!(ui.focused(), Some("editor"));
    assert!(sources.selected().is_err());
    let before = app.open_document().unwrap().clone();
    assert!(ui.open_app_document(&mut app, "../outside.txt").is_err());
    assert_eq!(app.open_document().unwrap(), &before);
    assert!(ui.open_app_document(&mut app, "missing.txt").is_err());
    assert_eq!(app.open_document().unwrap(), &before);
    ui.handle_event_with_sources(Event::Paste("edit ".into()), &mut app, &mut sources);
    ui.handle_event_with_sources(
        Event::Key(KeyEvent::new(
            KeyCode::Char('s'),
            event::KeyModifiers::CONTROL,
        )),
        &mut app,
        &mut sources,
    );
    assert_eq!(app.open_document().unwrap().text(), "edit App file");
    assert_eq!(root.read_file("file with spaces.txt"), "App file");
    assert!(
        ui.open_app_document(&mut app, "file with spaces.txt")
            .is_err()
    );
    ui.invoke_action(
        WorkspaceAction::SaveAppDocument,
        &mut app,
        &mut sources,
        None,
    )
    .unwrap();
    assert_eq!(root.read_file("file with spaces.txt"), "edit App file");
    assert!(!app.open_document().unwrap().is_dirty());
    assert!(sources.selected().is_err());
}

fn minimal_screen(h: &mut EditableWorkspace) -> String {
    h.terminal
        .draw(|frame| {
            h.ui.render_run_frame(frame, &h.app, Some(&h.sources), false);
        })
        .unwrap();
    h.terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn minimal_app_composition_hides_default_inspector_but_preserves_layout_access() {
    let root = TempWorkspace::new("minimal-app-layout");
    root.write_file("document.txt", "App content");
    let mut h = EditableWorkspace::new(&root, true);
    h.ui.register("inspector", Counter(Rc::default())).unwrap();
    h.ui.layout(
        "workspace",
        View::horizontal([View::leaf("editor"), View::leaf("inspector")]),
    )
    .unwrap();
    h.ui.layout("editor", View::leaf("editor")).unwrap();
    h.ui.select_layout("workspace").unwrap();
    h.render();
    h.ui.open_app_document(&mut h.app, "document.txt").unwrap();
    let screen = minimal_screen(&mut h);
    assert_eq!(h.ui.layouts[h.ui.active].0, "editor");
    assert_eq!(h.ui.visible.len(), 1);
    assert!(h.ui.visible[0].1.height > h.ui.output_area.unwrap().height);
    assert!(screen.contains("App content"));
    assert!(!screen.contains("inspector"));
    assert!(screen.contains("Ctrl-O Open · Ctrl-R Run · Ctrl-Q Quit · Ctrl-G More"));
    assert_eq!(h.ui.rendered_output(&h.app, Some(&h.sources)), "");
    h.control('g');
    assert!(h.ui.picker.is_some());
    assert!(
        h.ui.actions(&h.sources, false)
            .iter()
            .any(|item| item.action == WorkspaceAction::InspectDocument)
    );
    h.ui.picker = None;
    h.ui.select_layout("workspace").unwrap();
    assert!(minimal_screen(&mut h).contains("inspector"));
    h.ui.focus_named("inspector").unwrap();
    h.key(KeyCode::Char('x'), event::KeyModifiers::NONE);
    h.ui.select_layout("editor").unwrap();
    minimal_screen(&mut h);
    h.ui.select_layout("workspace").unwrap();
    minimal_screen(&mut h);
    assert!(h.pane_text("inspector").contains('1'));
}

#[test]
fn minimal_app_composition_preserves_custom_layout_and_non_app_footer() {
    let root = TempWorkspace::new("minimal-custom-layout");
    root.write_file("document.txt", "App content");
    let mut h = EditableWorkspace::new(&root, true);
    assert!(minimal_screen(&mut h).contains("Ctrl-S publish (memory)"));
    h.ui.open_app_document(&mut h.app, "document.txt").unwrap();
    minimal_screen(&mut h);
    assert_eq!(h.ui.layouts[h.ui.active].0, "both");
    assert_eq!(h.ui.visible.len(), 2);
}

struct MinimalRunLanguage;
impl scope_language::LanguageService for MinimalRunLanguage {
    fn language_id(&self) -> &'static str {
        "minimal-run"
    }
    fn language_name(&self) -> &'static str {
        "Minimal Run"
    }
    fn matches_path(&self, _: &scope_language::LanguagePath<'_>) -> bool {
        true
    }
    fn analyze(&self, _: scope_language::LanguageDocument<'_>) -> scope_language::LanguageAnalysis {
        Default::default()
    }
    fn semantic_operations(&self) -> Vec<scope_language::SemanticOperationDescription> {
        use scope_language::*;
        vec![
            SemanticOperationDescription {
                id: "evaluate".into(),
                description: "Evaluate".into(),
                arguments: vec![],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            }
            .with_purpose(SemanticOperationPurpose::CandidateEvaluation)
            .unwrap(),
        ]
    }
    fn perform_semantic_operation(
        &self,
        _: scope_language::SemanticOperationRequest<'_>,
    ) -> Result<scope_language::SemanticOperationResult, scope_language::SemanticOperationError>
    {
        Ok(scope_language::SemanticOperationResult::ReadOnly {
            value: serde_json::json!({"secret_evidence": "not ordinary output"}),
            rationale: "fixture".into(),
        })
    }
}

#[test]
fn minimal_app_run_renders_retained_summary_and_unpublished_draft_staleness() {
    let root = TempWorkspace::new("minimal-run-output");
    root.write_file("document.txt", "baseline");
    let mut h = EditableWorkspace::new(&root, true);
    h.app = App::builder(root.workspace())
        .language(&MinimalRunLanguage)
        .build()
        .unwrap();
    h.ui.open_app_document(&mut h.app, "document.txt").unwrap();
    minimal_screen(&mut h);
    h.ui.invoke_action(
        WorkspaceAction::RunCurrentDocument,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert!(h.ui.semantic_inspection.is_none());
    assert!(h.ui.inspection_result().is_some());
    let screen = minimal_screen(&mut h);
    assert!(screen.contains("Run completed; inspect for results and diagnostics."));
    assert!(!screen.contains("secret_evidence"));
    assert!(!screen.contains("STALE"));
    h.event(Event::Paste("draft ".into()));
    assert_eq!(h.app.open_document().unwrap().text(), "baseline");
    assert!(minimal_screen(&mut h).contains("STALE (unpublished editor draft)"));
    h.control('s');
    assert!(minimal_screen(&mut h).contains("STALE (source changed or no longer open)"));
    h.ui.invoke_action(
        WorkspaceAction::InspectDocument,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert!(h.ui.semantic_inspection.is_some());
}

#[test]
fn minimal_app_run_resets_old_scroll_errors_and_history_presentation() {
    let root = TempWorkspace::new("minimal-run-reset");
    root.write_file("document.txt", "baseline");
    let mut h = EditableWorkspace::new(&root, true);
    h.app = App::builder(root.workspace())
        .language(&MinimalRunLanguage)
        .build()
        .unwrap();
    h.ui.open_app_document(&mut h.app, "document.txt").unwrap();
    minimal_screen(&mut h);
    h.ui.output_scroll = 100;
    h.ui.output_errors = "old failure\n".into();
    h.ui.show_result = true;
    h.ui.invoke_action(
        WorkspaceAction::RunCurrentDocument,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert_eq!(h.ui.output_scroll, 0);
    assert!(h.ui.output_errors.is_empty());
    assert!(!h.ui.show_result);
    let screen = minimal_screen(&mut h);
    assert!(screen.contains("Run completed; inspect for results and diagnostics."));
    assert!(!screen.contains("old failure"));
}

#[test]
fn minimal_app_rejected_run_prioritizes_failure_and_retains_inspection_evidence() {
    let root = TempWorkspace::new("minimal-run-rejection");
    root.write_file("document.txt", "baseline");
    let mut h = EditableWorkspace::new(&root, true);
    h.app = App::builder(root.workspace())
        .language(&MinimalRunLanguage)
        .build()
        .unwrap();
    h.ui.open_app_document(&mut h.app, "document.txt").unwrap();
    minimal_screen(&mut h);
    h.ui.invoke_action(
        WorkspaceAction::RunCurrentDocument,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert!(minimal_screen(&mut h).contains("Run completed"));
    let evidence = format!("{:?}", h.ui.inspection_result().unwrap());
    h.ui.focus_named("output").unwrap();
    h.ui.output_scroll = 100;
    h.ui.output_errors = "old failure\n".into();
    let error =
        h.ui.invoke_action(
            WorkspaceAction::RunCurrentDocument,
            &mut h.app,
            &mut h.sources,
            None,
        )
        .unwrap_err();
    assert_eq!(h.ui.output_scroll, 0);
    assert!(h.ui.output_errors.is_empty());
    // Match the terminal loop's rejected-action reporting path.
    h.ui.output_errors.push_str(&format!("{error}\n"));
    let screen = minimal_screen(&mut h);
    assert!(screen.contains(&error));
    assert!(!screen.contains("Run completed"));
    assert!(!screen.contains("old failure"));
    assert_eq!(
        h.ui.rendered_output(&h.app, Some(&h.sources)),
        format!("{error}\n")
    );
    assert_eq!(format!("{:?}", h.ui.inspection_result().unwrap()), evidence);
    h.ui.invoke_action(
        WorkspaceAction::InspectDocument,
        &mut h.app,
        &mut h.sources,
        None,
    )
    .unwrap();
    assert!(h.ui.semantic_inspection.is_some());
    assert_eq!(format!("{:?}", h.ui.inspection_result().unwrap()), evidence);
}
