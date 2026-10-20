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
