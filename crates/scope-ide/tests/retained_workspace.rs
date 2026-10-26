#![cfg(feature = "tui")]

use ratatui::{Terminal, backend::TestBackend, layout::Rect, widgets::Paragraph};
use scope_ide::{
    app::App,
    cli::{self, DownstreamError, DownstreamOutcome, ReadOnlyCommand},
    document::Document,
    language::InteractiveSession,
    scratch::{BoundSource, ScratchBuffer, ScratchBuffers},
    text::TextRange,
    tui::{Pane, TuiContext, View, WorkspaceTui},
    workspace::Workspace,
};
use scope_language::{RevisionId, SourceSnapshot};
use std::{
    cell::RefCell,
    error::Error,
    io::{self, BufReader, Read, Write},
    rc::Rc,
};

struct SnapshotPane(Rc<RefCell<Vec<Option<BoundSource>>>>);
impl Pane for SnapshotPane {
    fn id(&self) -> &'static str {
        "snapshot"
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        let source = ctx.bound_semantic_source();
        let text = match &source {
            None => "no owner".to_owned(),
            Some(BoundSource::Unbound) => "unbound".to_owned(),
            Some(BoundSource::Unavailable { scratch_id }) => format!("unavailable {scratch_id}"),
            Some(BoundSource::Available(snapshot)) => snapshot.text.to_string(),
        };
        self.0.borrow_mut().push(source);
        frame.render_widget(Paragraph::new(text), area);
    }
}

#[derive(Default)]
struct Host(Vec<Option<SourceSnapshot>>);
impl ReadOnlyCommand for Host {
    fn downstream_namespaces(&self) -> &[&str] {
        &["snapshot"]
    }

    fn invoke_downstream(
        &mut self,
        namespace: &str,
        command: &str,
        args: &[&str],
        target: Option<&ScratchBuffer>,
        _: &mut dyn Write,
    ) -> Result<DownstreamOutcome, DownstreamError> {
        assert_eq!((namespace, command, args), ("snapshot", "read", &[][..]));
        self.0.push(target.map(|source| SourceSnapshot {
            source_id: source.evaluation_source_id().clone(),
            revision_id: RevisionId::from_version(source.version()),
            text: source.text().into(),
        }));
        Ok(DownstreamOutcome::Handled)
    }

    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn Error>> {
        panic!("snapshot commands must use downstream dispatch")
    }
}

struct Harness {
    app: App,
    session: InteractiveSession,
    sources: ScratchBuffers,
    host: Host,
    tui: WorkspaceTui,
    terminal: Terminal<TestBackend>,
    observed: Rc<RefCell<Vec<Option<BoundSource>>>>,
}
impl Harness {
    fn new() -> Self {
        Self::with_workspace(Workspace::read_only(env!("CARGO_MANIFEST_DIR")).unwrap())
    }

    fn with_workspace(workspace: Workspace) -> Self {
        let observed = Rc::new(RefCell::new(Vec::new()));
        let mut tui = WorkspaceTui::new();
        tui.register("snapshot", SnapshotPane(observed.clone()))
            .unwrap();
        tui.layout("snapshot", View::leaf("snapshot")).unwrap();
        tui.select_layout("snapshot").unwrap();
        Self {
            app: App::builder(workspace).build().unwrap(),
            session: InteractiveSession::new(),
            sources: ScratchBuffers::default(),
            host: Host::default(),
            tui,
            terminal: Terminal::new(TestBackend::new(80, 12)).unwrap(),
            observed,
        }
    }

    // Each call ends at EOF and hands the same owner back to the workspace.
    fn shell(&mut self, input: &str) -> String {
        let mut output = Vec::new();
        cli::run_borrowed_with_sources(
            &mut self.app,
            &mut self.session,
            Some(&mut self.host),
            &mut self.sources,
            input.as_bytes(),
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("error:"), "{output}");
        output
    }

    fn render(&mut self, expected_text: &str) -> BoundSource {
        self.terminal
            .draw(|frame| {
                self.tui
                    .render_with_sources(frame, frame.area(), &self.app, &self.sources);
            })
            .unwrap();
        let state = self.observed.borrow().last().unwrap().clone().unwrap();
        assert_eq!(state, self.sources.bound_source());
        let buffer = self.terminal.backend().buffer();
        let visible: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(
            visible.contains(expected_text),
            "missing {expected_text:?}: {visible:?}"
        );
        state
    }

    fn snapshot(&mut self, text: &str) -> SourceSnapshot {
        match self.render(text) {
            BoundSource::Available(snapshot) => {
                assert_eq!(&*snapshot.text, text);
                snapshot
            }
            other => panic!("expected available source, got {other:?}"),
        }
    }
}

struct ReadFailure;
impl Read for ReadFailure {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("injected retained-owner read failure"))
    }
}

#[test]
fn shell_handoffs_render_live_binding_and_preserve_historical_snapshots() {
    let mut h = Harness::new();
    h.terminal
        .draw(|frame| h.tui.render(frame, frame.area(), &h.app))
        .unwrap();
    assert_eq!(h.observed.borrow().as_slice(), &[None]);
    assert_eq!(h.render("unbound"), BoundSource::Unbound);

    h.shell(":scope scratch new text\n:scope line 1 \"α original\"\n:scope semantic-target 0\n:scope downstream snapshot read\n");
    let original = h.snapshot("α original");
    assert_eq!(h.host.0.last(), Some(&Some(original.clone())));

    h.shell(":scope line 1 \"β edited\"\n:scope downstream snapshot read\n");
    let edited = h.snapshot("β edited");
    assert_eq!(edited.source_id, original.source_id);
    assert_ne!(edited.revision_id, original.revision_id);
    assert_eq!(&*original.text, "α original");
    assert_eq!(h.host.0.last(), Some(&Some(edited.clone())));

    h.shell(
        ":scope scratch new text\n:scope line 1 \"intermediate\"\n:scope line 1 \"β edited\"\n:scope downstream snapshot read\n",
    );
    assert_eq!(h.sources.selected().unwrap().id(), 1);
    assert_eq!(h.snapshot("β edited"), edited);
    assert_eq!(h.host.0.last(), Some(&Some(edited.clone())));

    h.shell(":scope semantic-target 1\n:scope scratch select 0\n:scope downstream snapshot read\n");
    let equal_text = h.snapshot("β edited");
    assert_eq!(equal_text.text, edited.text);
    assert_eq!(equal_text.revision_id, edited.revision_id);
    assert_ne!(equal_text.source_id, edited.source_id);
    assert_eq!(h.sources.selected().unwrap().id(), 0);
    assert_eq!(h.host.0.last(), Some(&Some(equal_text.clone())));

    // Editing the selected, unbound scratch cannot change the pane's source.
    h.shell(":scope line 1 \"not bound\"\n");
    assert_eq!(h.snapshot("β edited"), equal_text);
    h.shell(":scope scratch select 1\n:scope line 1 \"γ retained\"\n:scope quit\n");
    let retained = h.snapshot("γ retained");
    assert_eq!(retained.source_id, equal_text.source_id);
    assert_ne!(retained.revision_id, equal_text.revision_id);
    assert_eq!(&*equal_text.text, "β edited");

    // The reader fails after a complete edit, not just before any input is read.
    let input =
        BufReader::new(io::Cursor::new(b":scope line 1 \"after read error\"\n").chain(ReadFailure));
    let error = cli::run_borrowed_with_sources(
        &mut h.app,
        &mut h.session,
        Some(&mut h.host),
        &mut h.sources,
        input,
        Vec::new(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("injected retained-owner read failure")
    );
    let after_error = h.snapshot("after read error");
    assert_eq!(after_error.source_id, retained.source_id);
    assert_ne!(after_error.revision_id, retained.revision_id);
    assert_eq!(&*retained.text, "γ retained");
    h.shell(":scope downstream snapshot read\n");
    assert_eq!(h.host.0.last(), Some(&Some(after_error.clone())));

    h.shell(":scope discard\n");
    assert_eq!(
        h.render("unavailable 1"),
        BoundSource::Unavailable { scratch_id: 1 }
    );
    let calls = h.host.0.len();
    let mut output = Vec::new();
    cli::run_borrowed_with_sources(
        &mut h.app,
        &mut h.session,
        Some(&mut h.host),
        &mut h.sources,
        &b":scope downstream snapshot read\n"[..],
        &mut output,
    )
    .unwrap();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("target_unavailable")
    );
    assert_eq!(h.host.0.len(), calls);
    assert_eq!(&*after_error.text, "after read error");

    h.shell(":scope semantic-target 0\n:scope downstream snapshot read\n");
    let rebound = h.snapshot("not bound");
    assert_eq!(rebound.source_id, original.source_id);
    assert_eq!(h.host.0.last(), Some(&Some(rebound)));
    h.shell(":scope semantic-target none\n:scope downstream snapshot read\n");
    assert_eq!(h.render("unbound"), BoundSource::Unbound);
    assert_eq!(h.host.0.last(), Some(&None));
    assert_eq!(&*original.text, "α original");
}

struct ReadOnlyFixture(std::path::PathBuf);
impl ReadOnlyFixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scope-retained-workspace-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("source.txt"), "disk baseline").unwrap();
        Self(path)
    }

    fn workspace(&self) -> Workspace {
        Workspace::read_only(self.0.to_str().unwrap()).unwrap()
    }

    fn disk_text(&self) -> String {
        std::fs::read_to_string(self.0.join("source.txt")).unwrap()
    }
}
impl Drop for ReadOnlyFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn observation_does_not_dispatch_install_a_session_or_grant_write_authority() {
    let fixture = ReadOnlyFixture::new();
    let mut h = Harness::with_workspace(fixture.workspace());
    assert_eq!(h.session.language_id(), None);
    h.shell(concat!(
        ":scope scratch new text\n",
        ":scope line 1 \":scope downstream snapshot read\"\n",
        ":scope scratch destination source.txt\n",
        ":scope semantic-target 0\n",
    ));
    let before = h.snapshot(":scope downstream snapshot read");
    assert_eq!(h.snapshot(":scope downstream snapshot read"), before);
    assert!(h.host.0.is_empty());
    assert_eq!(h.session.language_id(), None);
    assert_eq!(fixture.disk_text(), "disk baseline");
    let baseline = h
        .sources
        .selected()
        .unwrap()
        .destination_revision()
        .unwrap()
        .to_owned();

    let mut output = Vec::new();
    cli::run_borrowed_with_sources(
        &mut h.app,
        &mut h.session,
        Some(&mut h.host),
        &mut h.sources,
        &b":scope scratch save\n"[..],
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("error:"), "{output}");
    assert_eq!(fixture.disk_text(), "disk baseline");
    assert_eq!(h.snapshot(":scope downstream snapshot read"), before);
    let source = h.sources.selected().unwrap();
    assert!(source.is_dirty());
    assert_eq!(source.destination_path().unwrap().as_str(), "source.txt");
    assert_eq!(source.destination_revision(), Some(baseline.as_str()));
    assert!(h.host.0.is_empty());
    assert_eq!(h.session.language_id(), None);

    h.shell(":scope downstream snapshot read\n");
    assert_eq!(h.host.0, vec![Some(before)]);
    assert_eq!(fixture.disk_text(), "disk baseline");
    assert_eq!(h.session.language_id(), None);
}

#[test]
fn identical_destination_paths_do_not_alias_scratch_source_identities() {
    let fixture = ReadOnlyFixture::new();
    let mut h = Harness::with_workspace(fixture.workspace());
    let first = h.sources.create("text", "same unsaved text");
    let second = h.sources.create("text", "same unsaved text");
    for id in [first, second] {
        h.sources
            .get_mut(id)
            .unwrap()
            .bind_destination(h.app.workspace(), "source.txt")
            .unwrap();
    }
    let a = h.sources.get(first).unwrap();
    let b = h.sources.get(second).unwrap();
    assert_ne!(a.id(), b.id());
    assert_eq!(a.destination_path(), b.destination_path());
    assert_eq!(a.destination_revision(), b.destination_revision());
    assert_ne!(a.evaluation_source_id(), b.evaluation_source_id());

    h.sources.bind_semantic_target(Some(first)).unwrap();
    let first_snapshot = h.snapshot("same unsaved text");
    assert_eq!(h.sources.selected().unwrap().id(), second);
    h.sources.bind_semantic_target(Some(second)).unwrap();
    let second_snapshot = h.snapshot("same unsaved text");
    assert_eq!(first_snapshot.text, second_snapshot.text);
    assert_eq!(first_snapshot.revision_id, second_snapshot.revision_id);
    assert_ne!(first_snapshot.source_id, second_snapshot.source_id);
    assert_eq!(fixture.disk_text(), "disk baseline");
    assert!(h.host.0.is_empty());
    assert_eq!(h.session.language_id(), None);
}

struct InstalledSession(Rc<RefCell<usize>>);
impl scope_language::InteractiveLanguageSession for InstalledSession {
    fn language_id(&self) -> &str {
        "retained-test"
    }
    fn evaluate(
        &mut self,
        _: &str,
    ) -> Result<scope_language::EvaluationResult, scope_language::SessionError> {
        panic!("source observation must not evaluate")
    }
    fn reset(&mut self) -> Result<(), scope_language::SessionError> {
        panic!("shell failure must not reset the retained session")
    }
    fn close(&mut self) -> Result<(), scope_language::SessionError> {
        *self.0.borrow_mut() += 1;
        Ok(())
    }
}

struct InvalidRegistration;
impl ReadOnlyCommand for InvalidRegistration {
    fn downstream_namespaces(&self) -> &[&str] {
        &["Invalid"]
    }
    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn Error>> {
        panic!("invalid registration must fail before dispatch")
    }
}

struct OutputFailure {
    fail_write: bool,
    flushes_before_failure: usize,
}
impl Write for OutputFailure {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.fail_write {
            Err(io::Error::other("injected retained-owner output failure"))
        } else {
            Ok(bytes.len())
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.flushes_before_failure == 0 {
            Err(io::Error::other("injected retained-owner flush failure"))
        } else {
            self.flushes_before_failure -= 1;
            Ok(())
        }
    }
}

#[test]
fn registration_and_output_failures_retain_sources_and_installed_session() {
    let mut h = Harness::new();
    h.shell(
        ":scope scratch new text\n:scope line 1 \"before failures\"\n:scope semantic-target 0\n",
    );
    let before = h.snapshot("before failures");
    let closes = Rc::new(RefCell::new(0));
    h.session
        .install("retained-test", Box::new(InstalledSession(closes.clone())))
        .unwrap();

    let mut input = io::Cursor::new(b":scope line 1 \"must not run\"\n");
    let mut output = Vec::new();
    let error = cli::run_borrowed_with_sources(
        &mut h.app,
        &mut h.session,
        Some(&mut InvalidRegistration),
        &mut h.sources,
        &mut input,
        &mut output,
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid_namespace"), "{error}");
    assert_eq!(input.position(), 0);
    assert!(output.is_empty());
    assert_eq!(h.snapshot("before failures"), before);
    assert_eq!(h.session.language_id(), Some("retained-test"));
    assert_eq!(*closes.borrow(), 0);

    let error = cli::run_borrowed_with_sources(
        &mut h.app,
        &mut h.session,
        Some(&mut h.host),
        &mut h.sources,
        &mut input,
        OutputFailure {
            fail_write: true,
            flushes_before_failure: 0,
        },
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("injected retained-owner output failure")
    );
    assert_eq!(input.position(), 0);
    assert_eq!(h.snapshot("before failures"), before);
    assert_eq!(h.session.language_id(), Some("retained-test"));
    assert_eq!(*closes.borrow(), 0);

    // Fail the next prompt's flush, after one command has committed its edit.
    let error = cli::run_borrowed_with_sources(
        &mut h.app,
        &mut h.session,
        Some(&mut h.host),
        &mut h.sources,
        &b":scope line 1 \"committed before flush failure\"\n"[..],
        OutputFailure {
            fail_write: false,
            flushes_before_failure: 1,
        },
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("injected retained-owner flush failure")
    );
    let after = h.snapshot("committed before flush failure");
    assert_eq!(after.source_id, before.source_id);
    assert_ne!(after.revision_id, before.revision_id);
    assert_eq!(&*before.text, "before failures");
    assert_eq!(h.sources.selected().unwrap().id(), 0);
    assert!(h.sources.selected().unwrap().is_dirty());
    assert_eq!(h.session.language_id(), Some("retained-test"));
    assert_eq!(*closes.borrow(), 0);
    assert!(h.host.0.is_empty());

    h.shell(":scope downstream snapshot read\n");
    assert_eq!(h.host.0, vec![Some(after)]);
    assert_eq!(h.session.language_id(), Some("retained-test"));
    assert_eq!(*closes.borrow(), 0);
}
