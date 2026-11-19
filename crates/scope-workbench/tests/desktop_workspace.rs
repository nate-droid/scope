#![cfg(feature = "desktop")]

use std::{
    fs,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use scope_ide::tui::desktop::{
    self, DesktopFrame, DesktopInput, DesktopUpdate, Event, KeyCode, KeyEvent, KeyModifiers,
};

struct Workspace {
    input: mpsc::Sender<DesktopInput>,
    output: mpsc::Receiver<DesktopUpdate>,
    worker: thread::JoinHandle<Result<(), String>>,
}

impl Workspace {
    fn start(path: &Path, writable: bool, initial_path: Option<&str>) -> Self {
        let (input, read) = mpsc::channel();
        let (write, output) = mpsc::channel();
        let path = path.display().to_string();
        let initial_path = initial_path.map(str::to_owned);
        let worker = thread::spawn(move || {
            let mut host = scope_script::workbench::WorkbenchRunner::default();
            desktop::run(
                path,
                writable,
                initial_path.as_deref(),
                &mut host,
                read,
                move |update| {
                    let _ = write.send(update);
                },
            )
            .map_err(|error| error.to_string())
        });
        Self {
            input,
            output,
            worker,
        }
    }

    fn receive(&self) -> DesktopUpdate {
        self.output
            .recv_timeout(Duration::from_secs(10))
            .expect("workspace stopped producing output")
    }

    fn frame(&self) -> DesktopFrame {
        loop {
            if let DesktopUpdate::Frame(frame) = self.receive() {
                return frame;
            }
        }
    }

    fn send(&self, event: DesktopInput) -> DesktopFrame {
        self.input.send(event).unwrap();
        self.frame()
    }

    fn key(&self, key: KeyCode, modifiers: KeyModifiers) -> DesktopFrame {
        self.send(DesktopInput::Event(Event::Key(KeyEvent::new(
            key, modifiers,
        ))))
    }

    fn action(&self, query: &str) -> DesktopFrame {
        self.key(KeyCode::F(1), KeyModifiers::NONE);
        for ch in query.chars() {
            self.key(KeyCode::Char(ch), KeyModifiers::NONE);
        }
        self.key(KeyCode::Enter, KeyModifiers::NONE)
    }

    fn paste(&self, text: &str) -> DesktopFrame {
        self.send(DesktopInput::Event(Event::Paste(text.into())))
    }

    fn exit(self, action: DesktopInput) {
        self.input.send(action).unwrap();
        assert!(matches!(
            self.output.recv_timeout(Duration::from_secs(10)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        self.worker.join().unwrap().unwrap();
    }
}

fn screen(frame: &DesktopFrame) -> String {
    frame
        .cells
        .content
        .chunks(usize::from(frame.cells.area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn opens_edits_and_saves_through_workspace_actions_without_losing_a_draft() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("main.scope"), "let value = 41\n").unwrap();
    fs::write(root.path().join("other.scope"), "let other = 2\n").unwrap();
    let ws = Workspace::start(root.path(), true, None);
    assert!(
        matches!(ws.receive(), DesktopUpdate::Files(Ok(paths)) if paths == ["main.scope", "other.scope"])
    );
    ws.frame();
    let frame = ws.send(DesktopInput::Open("main.scope".into()));
    assert_eq!(frame.document.as_deref(), Some("main.scope"));
    assert!(screen(&frame).contains("1  let value = 41"));
    let resized = ws.send(DesktopInput::Resize(84, 24));
    assert_eq!(
        (resized.cells.area.width, resized.cells.area.height),
        (84, 24)
    );
    assert!(screen(&resized).contains("1  let value = 41"));
    ws.key(KeyCode::Right, KeyModifiers::SHIFT);
    ws.input.send(DesktopInput::Copy { cut: false }).unwrap();
    assert!(matches!(ws.receive(), DesktopUpdate::Clipboard(text) if text == "l"));
    ws.frame();
    let frame = ws.paste("L");
    assert!(frame.unsaved && frame.draft);
    assert!(frame.cursor.is_some());
    assert!(screen(&frame).contains("Ln 1, Col 2"));
    let frame = ws.send(DesktopInput::Open("other.scope".into()));
    assert_eq!(frame.document.as_deref(), Some("other.scope"));
    assert!(frame.unsaved && frame.tabs[0].dirty && !frame.tabs[1].dirty);
    let frame = ws.send(DesktopInput::CloseDocument("main.scope".into()));
    assert_eq!(frame.tabs.len(), 2, "dirty tab must not close");
    let frame = ws.send(DesktopInput::CycleDocument { backwards: true });
    assert_eq!(frame.document.as_deref(), Some("main.scope"));
    assert!(screen(&frame).contains("Ln 1, Col 2"));
    assert!(screen(&frame).contains("Let value"));
    assert!(ws.send(DesktopInput::Close).confirm_close);
    assert!(ws.send(DesktopInput::Close).confirm_close);
    assert!(!ws.key(KeyCode::Esc, KeyModifiers::NONE).confirm_close);
    assert!(screen(&ws.key(KeyCode::Char('z'), KeyModifiers::CONTROL)).contains("let value"));
    assert!(screen(&ws.key(KeyCode::Char('y'), KeyModifiers::CONTROL)).contains("Let value"));
    let published = ws.action("editor.commit");
    assert!(
        !published.draft && published.published_unsaved,
        "{}",
        screen(&published)
    );
    assert_eq!(
        fs::read_to_string(root.path().join("main.scope")).unwrap(),
        "let value = 41\n"
    );
    let frame = ws.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(!frame.unsaved, "{}", screen(&frame));
    assert_eq!(
        fs::read_to_string(root.path().join("main.scope")).unwrap(),
        "Let value = 41\n"
    );
    assert_eq!(
        ws.send(DesktopInput::Open("other.scope".into()))
            .document
            .as_deref(),
        Some("other.scope")
    );
    let frame = ws.send(DesktopInput::CloseDocument("other.scope".into()));
    assert_eq!(frame.document.as_deref(), Some("main.scope"));
    assert_eq!(frame.tabs.len(), 1);
    ws.exit(DesktopInput::Close);
}

#[test]
fn read_only_mount_denies_explicit_save() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("main.scope"), "41").unwrap();
    let ws = Workspace::start(root.path(), false, Some("main.scope"));
    assert_eq!(ws.frame().document.as_deref(), Some("main.scope"));
    ws.paste("!");
    let published = ws.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(published.published_unsaved && !published.draft);
    let frame = ws.action("document.save-app");
    assert!(
        screen(&frame).contains("PermissionDenied"),
        "{}",
        screen(&frame)
    );
    assert_eq!(
        fs::read_to_string(root.path().join("main.scope")).unwrap(),
        "41"
    );
    assert!(ws.send(DesktopInput::Close).confirm_close);
    ws.exit(DesktopInput::ConfirmClose);
}

#[test]
fn actual_script_producer_retains_distinct_results() {
    let root = tempfile::tempdir().unwrap();
    let ws = Workspace::start(root.path(), false, None);
    ws.frame();
    ws.action("New Script");
    ws.paste("40 + 2");
    ws.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    let first = ws.key(KeyCode::F(5), KeyModifiers::NONE);
    assert!(screen(&first).contains("final: 42"), "{}", screen(&first));
    ws.paste(" + 1");
    ws.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    let second = ws.key(KeyCode::F(5), KeyModifiers::NONE);
    assert!(screen(&second).contains("final: 43"), "{}", screen(&second));
    let previous = ws.key(KeyCode::Left, KeyModifiers::ALT);
    assert!(
        screen(&previous).contains("final: 42"),
        "{}",
        screen(&previous)
    );
    assert!(ws.send(DesktopInput::Close).confirm_close);
    ws.exit(DesktopInput::ConfirmClose);
}

#[test]
fn desktop_save_publishes_but_does_not_overwrite_an_external_revision() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("main.scope");
    fs::write(&path, "41").unwrap();
    fs::write(root.path().join("other.scope"), "42").unwrap();
    let ws = Workspace::start(root.path(), true, Some("main.scope"));
    ws.frame();
    ws.paste("!");
    ws.send(DesktopInput::Open("other.scope".into()));
    fs::write(&path, "external edit").unwrap();
    ws.send(DesktopInput::CycleDocument { backwards: true });
    let frame = ws.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(frame.unsaved && frame.published_unsaved && !frame.draft);
    assert!(screen(&frame).contains("!41"));
    assert!(screen(&frame).contains("Save:"));
    assert_eq!(fs::read_to_string(path).unwrap(), "external edit");
    assert!(ws.send(DesktopInput::Close).confirm_close);
    ws.exit(DesktopInput::ConfirmClose);
}

#[test]
fn sprout_indentation_and_coloring_are_local_edits() {
    use scope_ide::tui::desktop::Color;
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("main.sprout"), "    let value = 42").unwrap();
    let ws = Workspace::start(root.path(), false, Some("main.sprout"));
    let frame = ws.frame();
    assert!(
        frame
            .cells
            .content
            .iter()
            .any(|cell| cell.symbol() == "l" && matches!(cell.fg, Color::Rgb(..)))
    );
    ws.key(KeyCode::End, KeyModifiers::NONE);
    let frame = ws.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(screen(&frame).contains("Ln 2, Col 5"));
    let frame = ws.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(!frame.unsaved);
    assert!(screen(&frame).contains("Ln 1, Col 19"));
    ws.exit(DesktopInput::Close);
}

#[test]
#[ignore = "requires maintained Loom, Path and Sprout sibling checkouts"]
fn ecosystem_editing_transport() {
    use scope_ide::tui::desktop::Color;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    for (project, path, alternate) in [
        ("loom", "loom.sprout", Some("common.sprout")),
        (
            "path",
            "examples/sprout/reflexivity.sprout",
            Some("README.md"),
        ),
        (
            "sprout",
            "examples/village_expedition/main.sprout",
            Some("examples/village_expedition/policy.sprout"),
        ),
    ] {
        let start = Instant::now();
        let ws = Workspace::start(&root.join(project), false, None);
        let DesktopUpdate::Files(Ok(files)) = ws.receive() else {
            panic!("project listing unavailable");
        };
        let browse = start.elapsed();
        assert!(files.iter().any(|file| file == path));
        if project == "loom" {
            assert!(
                files
                    .iter()
                    .filter(|file| file.ends_with(".sprout") && !file.contains('/'))
                    .count()
                    > 16
            );
        }
        assert!(ws.frame().tabs.is_empty());
        let start = Instant::now();
        let first = ws.send(DesktopInput::Open(path.into()));
        let open = start.elapsed();
        assert_eq!(first.document.as_deref(), Some(path));
        assert!(
            first
                .cells
                .content
                .iter()
                .any(|cell| matches!(cell.fg, Color::Rgb(..)))
        );
        let start = Instant::now();
        let edited = ws.paste("let incomplete = \"\n");
        let edit = start.elapsed();
        assert!(edited.tabs[0].dirty);
        assert!(screen(&edited).contains("Ln 2, Col 1"));
        let switch = alternate.map(|alternate| {
            assert_eq!(
                ws.send(DesktopInput::Open(alternate.into()))
                    .document
                    .as_deref(),
                Some(alternate)
            );
            if project == "loom" {
                assert_eq!(
                    ws.send(DesktopInput::Open("coherent.sprout".into()))
                        .tabs
                        .len(),
                    3
                );
            }
            let start = Instant::now();
            let restored = ws.send(DesktopInput::Open(path.into()));
            let switch = start.elapsed();
            assert_eq!(restored.document.as_deref(), Some(path));
            assert!(screen(&restored).contains("let incomplete = \""));
            assert_eq!(restored.cursor, edited.cursor);
            switch
        });
        let frame = ws.send(DesktopInput::CloseDocument(path.into()));
        assert!(frame.tabs.iter().any(|tab| tab.path == path && tab.dirty));
        assert!(!ws.key(KeyCode::Char('z'), KeyModifiers::CONTROL).unsaved);
        assert!(
            ws.send(DesktopInput::CloseDocument(path.into()))
                .tabs
                .iter()
                .all(|tab| tab.path != path)
        );
        eprintln!(
            "{project}: {} files; browse {browse:?}; open {open:?}; edit + color {edit:?}; switch {switch:?}",
            files.len()
        );
        ws.exit(DesktopInput::Close);
    }
}
