//! Public embedding contract. The runtime deliberately is neither Send nor Sync.
use scope_ide::{app::App, cli, language::InteractiveSession, workspace::Workspace};
use scope_language::{
    EvaluationResult, EvaluationStatus, InteractiveLanguageSession, SessionError,
};
use std::{
    cell::RefCell,
    io::{self, BufReader, Read, Write},
    rc::Rc,
};

#[derive(Debug, Default)]
struct Runtime {
    events: Vec<String>,
    values: Vec<String>,
    fail_close: bool,
    fail_reset: bool,
    closed: bool,
}
struct Fake {
    id: String,
    runtime: Rc<RefCell<Runtime>>,
}
fn failure() -> SessionError {
    SessionError::Infrastructure {
        code: "fake_failure".into(),
        message: "injected failure".into(),
    }
}
impl InteractiveLanguageSession for Fake {
    fn language_id(&self) -> &str {
        &self.id
    }
    fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
        let mut runtime = self.runtime.borrow_mut();
        runtime.events.push(format!("evaluate:{source}"));
        if source == "poison" {
            return Err(SessionError::ResetRequired);
        }
        if source == "error" {
            return Err(failure());
        }
        runtime.values.push(source.into());
        Ok(EvaluationResult {
            output: vec![format!("value:{source}")],
            diagnostics: vec![],
            status: EvaluationStatus::Success,
        })
    }
    fn reset(&mut self) -> Result<(), SessionError> {
        let mut runtime = self.runtime.borrow_mut();
        runtime.events.push("reset".into());
        if runtime.fail_reset {
            return Err(failure());
        }
        runtime.values.clear();
        runtime.closed = false;
        Ok(())
    }
    fn close(&mut self) -> Result<(), SessionError> {
        let mut runtime = self.runtime.borrow_mut();
        runtime.events.push("close".into());
        runtime.closed = true;
        if runtime.fail_close {
            Err(failure())
        } else {
            Ok(())
        }
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.runtime.borrow_mut().events.push("drop".into());
    }
}
fn candidate(id: &str) -> (Rc<RefCell<Runtime>>, Box<dyn InteractiveLanguageSession>) {
    let runtime = Rc::new(RefCell::new(Runtime::default()));
    (
        runtime.clone(),
        Box::new(Fake {
            id: id.into(),
            runtime,
        }),
    )
}
fn installed(id: &str) -> (Rc<RefCell<Runtime>>, InteractiveSession) {
    let (runtime, candidate) = candidate(id);
    let mut session = InteractiveSession::new();
    session.install(id, candidate).unwrap();
    (runtime, session)
}
fn app() -> App {
    // A read-only grant prevents the test shell from modifying this checkout.
    App::builder(Workspace::read_only(env!("CARGO_MANIFEST_DIR")).unwrap())
        .build()
        .unwrap()
}
fn shell(session: InteractiveSession, input: &str) -> String {
    let mut output = Vec::new();
    cli::run_with_session(app(), session, input.as_bytes(), &mut output).unwrap();
    String::from_utf8(output).unwrap()
}
fn events(runtime: &Rc<RefCell<Runtime>>) -> Vec<String> {
    runtime.borrow().events.clone()
}

#[test]
fn arbitrary_owned_identity_needs_no_registry_and_drop_closes() {
    let id = String::from("host/custom-runtime");
    let (runtime, mut session) = installed(&id);
    assert_eq!(session.language_id(), Some(id.as_str()));
    session.evaluate("seed").unwrap();
    assert_eq!(runtime.borrow().values, ["seed"]);
    drop(session);
    assert_eq!(events(&runtime), ["evaluate:seed", "close", "drop"]);
    assert!(runtime.borrow().closed);
    assert_eq!(Rc::strong_count(&runtime), 1);
}

#[test]
fn invalid_candidates_are_closed_and_dropped_without_touching_active() {
    let (active, mut session) = installed("active");
    for (requested, actual) in [("wanted", "wrong"), ("", ""), ("", "other"), ("wanted", "")] {
        let (rejected, candidate) = candidate(actual);
        // Cleanup failures must not prevent dropping a rejected candidate.
        rejected.borrow_mut().fail_close = true;
        assert!(session.install(requested, candidate).is_err());
        assert_eq!(events(&rejected), ["close", "drop"]);
        assert_eq!(Rc::strong_count(&rejected), 1);
        assert_eq!(session.language_id(), Some("active"));
        assert!(events(&active).is_empty());
    }
    session.evaluate("still active").unwrap();
}

#[test]
fn successful_replacement_closes_old_even_for_same_identity() {
    for id in ["old", "new"] {
        let (old, mut session) = installed("old");
        session.evaluate("poison").unwrap_err();
        let (new, replacement) = candidate(id);
        session.install(id, replacement).unwrap();
        assert_eq!(events(&old), ["evaluate:poison", "close", "drop"]);
        assert_eq!(session.language_id(), Some(id));
        session.evaluate("fresh").unwrap();
        drop(session);
        assert_eq!(events(&new), ["evaluate:fresh", "close", "drop"]);
    }
}

#[test]
fn failed_replacement_retains_old_and_requires_successful_reset() {
    let (old, mut session) = installed("old");
    old.borrow_mut().fail_close = true;
    let (new, replacement) = candidate("new");
    new.borrow_mut().fail_close = true;
    assert!(session.install("new", replacement).is_err());
    assert_eq!(session.language_id(), Some("old"));
    assert_eq!(events(&new), ["close", "drop"]);
    assert_eq!(events(&old), ["close"]);
    assert!(matches!(
        session.evaluate("blocked"),
        Err(SessionError::ResetRequired)
    ));
    old.borrow_mut().fail_reset = true;
    assert!(session.reset().is_err());
    assert!(matches!(
        session.evaluate("still blocked"),
        Err(SessionError::ResetRequired)
    ));
    old.borrow_mut().fail_reset = false;
    session.reset().unwrap();
    session.evaluate("recovered").unwrap();
    assert!(session.clear().is_err());
    assert_eq!(session.language_id(), None);
    drop(session);
    assert_eq!(
        events(&old),
        [
            "close",
            "reset",
            "reset",
            "evaluate:recovered",
            "close",
            "drop"
        ]
    );
}

#[test]
fn rejected_candidate_does_not_clear_existing_reset_requirement() {
    let (runtime, mut session) = installed("host");
    assert!(matches!(
        session.evaluate("poison"),
        Err(SessionError::ResetRequired)
    ));
    let (_, wrong) = candidate("wrong");
    assert!(session.install("host", wrong).is_err());
    assert!(matches!(
        session.evaluate("blocked"),
        Err(SessionError::ResetRequired)
    ));
    assert_eq!(events(&runtime), ["evaluate:poison"]);
}

#[test]
fn redirected_shell_routes_commands_and_quit_consumes_controller() {
    let (runtime, session) = installed("host");
    let output = shell(
        session,
        ":scope session\r\n  source  \r\nhelp\n:scope session missing\nnext\n:scope quit\nignored\n",
    );
    assert!(output.contains("Active language: host"));
    assert!(output.contains("value:  source  "));
    assert!(output.contains("value:help"));
    assert_eq!(
        events(&runtime),
        [
            "evaluate:  source  ",
            "evaluate:help",
            "evaluate:next",
            "close",
            "drop"
        ]
    );
    assert!(!output.contains('\u{1b}'));
}

#[test]
fn reset_preserves_captured_buffer_and_evaluation_is_explicit() {
    let (runtime, session) = installed("host");
    let output = shell(
        session,
        "seed\n:scope scratch capture\npoison\nblocked\n:scope session-reset\n:scope scratch show\n:scope scratch eval\n:scope quit\n",
    );
    assert!(output.contains("session reset"));
    assert!(output.contains("seed"));
    assert_eq!(
        events(&runtime),
        [
            "evaluate:seed",
            "evaluate:poison",
            "reset",
            "evaluate:seed",
            "close",
            "drop"
        ]
    );
    assert_eq!(runtime.borrow().values, ["seed"]);
}

#[test]
fn repl_and_edited_buffer_use_one_runtime_without_replay() {
    let (runtime, session) = installed("host");
    let output = shell(
        session,
        concat!(
            "from-repl\n",
            ":scope scratch new host\n",
            ":scope scratch replace 1 1 1 1 from-buffer\n",
            ":scope buffer eval\n",
            "after-buffer\n",
            ":scope session-reset\n",
            ":scope buffer eval\n",
            ":scope quit\n",
        ),
    );
    assert!(!output.contains("error:"), "{output}");
    assert_eq!(
        events(&runtime),
        [
            "evaluate:from-repl",
            "evaluate:from-buffer",
            "evaluate:after-buffer",
            "reset",
            "evaluate:from-buffer",
            "close",
            "drop",
        ]
    );
    assert_eq!(runtime.borrow().values, ["from-buffer"]);
}

#[test]
fn separate_shells_isolate_runtime_and_scratch_buffers() {
    let (first, session) = installed("host");
    shell(session, "one\n:scope scratch capture\n:scope quit\n");
    let (second, session) = installed("host");
    let output = shell(session, ":scope scratch eval\ntwo\n:scope quit\n");
    assert!(output.contains("error:"));
    assert_eq!(first.borrow().values, ["one"]);
    assert_eq!(second.borrow().values, ["two"]);
}

#[test]
fn eof_evaluation_error_and_clear_cleanup() {
    for input in [
        "",
        "final without newline",
        "error\nafter\n",
        ":scope session none\n:scope quit\n",
    ] {
        let (runtime, session) = installed("host");
        shell(session, input);
        let events = events(&runtime);
        assert_eq!(&events[events.len() - 2..], ["close", "drop"]);
        assert_eq!(events.iter().filter(|event| *event == "close").count(), 1);
        if input.starts_with("error") {
            assert_eq!(runtime.borrow().values, ["after"]);
        }
        if input.starts_with("final") {
            assert_eq!(runtime.borrow().values, [input]);
        }
    }
}

#[derive(Default)]
struct RecordingCommand {
    calls: Vec<(
        scope_ide::document::Document,
        scope_ide::text::TextRange,
        String,
    )>,
}
impl cli::ReadOnlyCommand for RecordingCommand {
    fn invoke(
        &mut self,
        document: &scope_ide::document::Document,
        selection: scope_ide::text::TextRange,
        operation: &str,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.calls
            .push((document.clone(), selection, operation.to_owned()));
        if operation == "fail" {
            return Err(io::Error::other("injected host failure").into());
        }
        writeln!(output, "host-result:{operation}")?;
        Ok(())
    }
}
fn preopened_app(text: &str) -> App {
    use scope_ide::text::{Position, TextEdit, TextRange};
    let mut app = app();
    app.open_path("Cargo.toml").unwrap();
    let document = app.open_document_mut().unwrap();
    let original = document.text();
    let end = Position::new(
        original.bytes().filter(|byte| *byte == b'\n').count(),
        original.rsplit('\n').next().unwrap().len(),
    );
    document
        .apply_edit(TextEdit::replace(
            TextRange::new(Position::new(0, 0), end),
            text,
        ))
        .unwrap();
    app
}

fn command_shell(
    app: App,
    session: InteractiveSession,
    command: &mut RecordingCommand,
    input: &str,
) -> String {
    let mut output = Vec::new();
    cli::run_with_session_and_command(app, session, command, input.as_bytes(), &mut output)
        .unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn host_command_observes_live_unsaved_document_identity_version_and_selection() {
    use scope_ide::text::{Position, TextRange};
    let mut command = RecordingCommand::default();
    let output = command_shell(
        preopened_app("aéz\nnext"),
        InteractiveSession::new(),
        &mut command,
        concat!(
            ":scope host inspect --selection 0:1..0:3\n",
            ":scope host inspect --selection 0:1..0:3\n",
            "replace 2 1 2 5 changed\n",
            ":scope host inspect --selection 0:3..1:7\n",
            ":scope quit\n",
        ),
    );
    assert!(!output.contains("error:"), "{output}");
    assert_eq!(output.matches("host-result:inspect").count(), 3);
    assert_eq!(command.calls.len(), 3);
    let (first, selection, operation) = &command.calls[0];
    assert_eq!(first.text(), "aéz\nnext");
    assert!(first.is_dirty());
    assert_eq!(
        *selection,
        TextRange::new(Position::new(0, 1), Position::new(0, 3))
    );
    assert_eq!(operation, "inspect");
    assert_eq!(first, &command.calls[1].0);
    let (edited, selection, _) = &command.calls[2];
    assert_eq!(edited.id(), first.id());
    assert_eq!(edited.evaluation_source_id(), first.evaluation_source_id());
    assert!(edited.version() > first.version());
    assert_eq!(edited.text(), "aéz\nchanged");
    assert!(edited.is_dirty());
    assert_eq!(
        *selection,
        TextRange::new(Position::new(0, 3), Position::new(1, 7))
    );
}

#[test]
fn host_command_rejects_missing_document_and_invalid_syntax_or_selection() {
    let mut command = RecordingCommand::default();
    let output = command_shell(
        app(),
        InteractiveSession::new(),
        &mut command,
        ":scope host inspect --selection 0:0..0:0\n:scope quit\n",
    );
    assert!(output.contains("error: no open document"), "{output}");
    assert!(command.calls.is_empty());
    let mut input = String::new();
    let invalid = [
        "host inspect --selection 0:0..0:1", // Bare host is not a shell command.
        ":scope host",
        ":scope host inspect",
        ":scope host inspect --selection",
        ":scope host inspect --selection nope",
        ":scope host inspect --selection 0:2..0:3", // Inside é.
        ":scope host inspect --selection 0:3..0:1", // Reversed.
        ":scope host inspect --selection 1:0..1:0", // Missing line.
        ":scope host inspect --selection 0:0..0:5", // Past line end.
        ":scope host inspect --selection 0:0..0:1 extra",
    ];
    for line in invalid {
        input.push_str(line);
        input.push('\n');
    }
    input.push_str(":scope host inspect --selection 0:1..0:3\n:scope quit\n");
    let output = command_shell(
        preopened_app("aéz"),
        InteractiveSession::new(),
        &mut command,
        &input,
    );
    assert_eq!(output.matches("error:").count(), invalid.len(), "{output}");
    assert_eq!(command.calls.len(), 1);
    assert_eq!(command.calls[0].2, "inspect");
    assert!(output.contains("host-result:inspect"), "{output}");
}

#[test]
fn host_command_without_callback_reports_error_and_shell_remains_usable() {
    let mut output = Vec::new();
    cli::run_with_session(
        preopened_app("aéz"),
        InteractiveSession::new(),
        &b":scope host inspect --selection 0:0..0:0\n:scope help\n:scope quit\n"[..],
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("error:"), "{output}");
    assert!(output.contains("Commands:"), "{output}");
}

#[test]
fn host_callback_failure_continues_without_evaluating_namespaced_commands() {
    let (runtime, session) = installed("host");
    let mut command = RecordingCommand::default();
    let output = command_shell(
        preopened_app("live unsaved source"),
        session,
        &mut command,
        concat!(
            "before\n",
            ":scope scratch new host\n",
            ":scope scratch replace 1 1 1 1 buffer-source\n",
            ":scope host fail --selection 0:0..0:6\n",
            ":scope host inspect --selection 0:0..0:6\n",
            "host inspect --selection 0:0..0:6\n",
            ":scope buffer eval\n",
            "after\n",
            ":scope quit\n",
        ),
    );
    assert!(output.contains("error:"), "{output}");
    assert!(output.contains("injected host failure"), "{output}");
    assert!(output.contains("host-result:inspect"), "{output}");
    assert_eq!(command.calls.len(), 2);
    assert_eq!(command.calls[0].0.text(), "live unsaved source");
    assert!(command.calls[0].0.is_dirty());
    assert_eq!(command.calls[0].0, command.calls[1].0);
    assert_eq!(command.calls[0].2, "fail");
    assert_eq!(command.calls[1].2, "inspect");
    assert_eq!(
        events(&runtime),
        [
            "evaluate:before",
            "evaluate:host inspect --selection 0:0..0:6",
            "evaluate:buffer-source",
            "evaluate:after",
            "close",
            "drop",
        ]
    );
}

struct BrokenReader;
impl Read for BrokenReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("input failed"))
    }
}
struct BrokenWriter;
impl Write for BrokenWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("output failed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("flush failed"))
    }
}
#[test]
fn io_errors_propagate_and_close_even_when_close_fails() {
    for read_failure in [true, false] {
        let (runtime, session) = installed("host");
        runtime.borrow_mut().fail_close = true;
        let error = if read_failure {
            cli::run_with_session(app(), session, BufReader::new(BrokenReader), Vec::new())
                .unwrap_err()
        } else {
            cli::run_with_session(app(), session, &b"source\n"[..], BrokenWriter).unwrap_err()
        };
        assert!(error.downcast_ref::<io::Error>().is_some(), "{error}");
        let events = events(&runtime);
        assert_eq!(&events[events.len() - 2..], ["close", "drop"]);
        assert_eq!(Rc::strong_count(&runtime), 1);
    }
}
