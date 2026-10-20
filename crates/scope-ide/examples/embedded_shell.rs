//! Run with `cargo run -p scope-ide --example embedded_shell`.
//! Redirected, deterministic input; no registry, network, or runtime dependencies.
use scope_ide::{app::App, cli, language::InteractiveSession, workspace::Workspace};
use scope_language::{
    EvaluationResult, EvaluationStatus, InteractiveLanguageSession, SessionError,
};
use std::{cell::RefCell, error::Error, rc::Rc};

#[derive(Default)]
struct Runtime {
    submissions: Vec<String>,
    closed: bool,
}
struct Adapter(Rc<RefCell<Runtime>>);
impl InteractiveLanguageSession for Adapter {
    fn language_id(&self) -> &str {
        "host.echo"
    }
    fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
        self.0.borrow_mut().submissions.push(source.to_owned());
        Ok(EvaluationResult {
            output: vec![format!("echo: {source}")],
            diagnostics: vec![],
            status: EvaluationStatus::Success,
        })
    }
    fn reset(&mut self) -> Result<(), SessionError> {
        self.0.borrow_mut().submissions.clear();
        Ok(())
    }
    fn close(&mut self) -> Result<(), SessionError> {
        self.0.borrow_mut().closed = true;
        Ok(())
    }
}
impl Drop for Adapter {
    fn drop(&mut self) {
        // Real adapters must also release resources if explicit close fails.
        self.0.borrow_mut().closed = true;
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let runtime = Rc::new(RefCell::new(Runtime::default()));
    let mut session = InteractiveSession::new();
    session.install("host.echo", Box::new(Adapter(runtime.clone())))?;
    let workspace = Workspace::read_only(env!("CARGO_MANIFEST_DIR"))?;
    let app = App::builder(workspace).build()?;
    let input = concat!(
        ":scope session\n",
        "hello from the host\n",
        ":scope scratch new host.echo\n",
        ":scope scratch replace 1 1 1 1 hello from a buffer\n",
        ":scope buffer eval\n",
        ":scope quit\n",
    );
    let mut output = Vec::new();
    let result = cli::run_with_session(app, session, input.as_bytes(), &mut output);
    // The host retains a typed handle after the shell consumes its controller,
    // including on I/O errors. No downcast, static provider, or leaked allocation.
    assert!(runtime.borrow().closed);
    result?;
    assert_eq!(
        runtime.borrow().submissions,
        ["hello from the host", "hello from a buffer"]
    );
    print!("{}", String::from_utf8(output)?);
    println!("host: runtime closed = {}", runtime.borrow().closed);
    Ok(())
}
