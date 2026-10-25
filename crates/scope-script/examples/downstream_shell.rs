//! Buffered downstream shell demo: `cargo run -p scope-script --example downstream_shell`.
#[allow(dead_code)]
#[path = "support/multi_semantic.rs"]
pub mod fixture;

use scope_ide::{
    app::App,
    cli::{self, DownstreamError, DownstreamOutcome, ReadOnlyCommand},
    document::Document,
    language::InteractiveSession,
    scratch::{ScratchBuffer, ScratchBuffers},
    text::TextRange,
    workspace::Workspace,
};
use scope_script::runner::{ScriptRunner, Snapshot};
use std::io::{Cursor, Write};

pub const ECHO: &str =
    "scope.invoke(\"semantic\", \"language.example.multi.echo\", {label: \"trusted alias\"})";
pub const DENIED: &str = "scope.invoke(\"semantic\", \"language.example.multi.denied\", {})";

#[derive(Debug)]
pub struct TargetState {
    pub snapshot: Snapshot,
    pub id: u64,
    pub dirty: bool,
    pub destination: Option<String>,
}

pub struct DownstreamShell<'a> {
    pub runner: ScriptRunner<'a>,
    pub selections: usize,
    pub arguments: Vec<Vec<String>>,
    pub targets: Vec<Option<TargetState>>,
}

impl<'a> DownstreamShell<'a> {
    pub fn new(service: &'a fixture::MultiLanguage) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            runner: ScriptRunner::with_operations(
                service,
                [("echo".into(), Some(true)), ("denied".into(), Some(false))],
            )?,
            selections: 0,
            arguments: Vec::new(),
            targets: Vec::new(),
        })
    }
}

impl ReadOnlyCommand for DownstreamShell<'_> {
    fn downstream_namespaces(&self) -> &[&str] {
        &["demo"]
    }

    fn invoke_downstream(
        &mut self,
        namespace: &str,
        command: &str,
        args: &[&str],
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<DownstreamOutcome, DownstreamError> {
        if namespace != "demo" {
            return Ok(DownstreamOutcome::Unhandled);
        }
        let script = match command {
            "echo" => ECHO,
            "denied" => DENIED,
            _ => return Ok(DownstreamOutcome::Unhandled),
        };
        self.selections += 1;
        self.arguments
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        self.targets.push(target.map(|source| TargetState {
            snapshot: Snapshot {
                identity: source.evaluation_source_id().clone(),
                revision: scope_language::RevisionId::from_version(source.version()),
                text: source.text().to_owned(),
            },
            id: source.id(),
            dirty: source.is_dirty(),
            destination: source.destination_path().map(|path| format!("{path}")),
        }));
        let target = target.ok_or_else(|| {
            DownstreamError::new("target_required", "bind a semantic target first")
        })?;
        // Arguments are data for the host, never interpolated into executable source.
        let mut scripts = ScratchBuffers::default();
        scripts.create("scope-script", script);
        self.runner
            .invoke_script_with_target(
                scripts.selected().expect("local script"),
                Some(target),
                output,
            )
            .map_err(|error| DownstreamError::new("script_error", error.to_string()))?;
        Ok(DownstreamOutcome::Handled)
    }

    fn invoke_script_with_target(
        &mut self,
        source: &ScratchBuffer,
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.runner
            .invoke_script_with_target(source, target, output)
    }

    fn invoke(
        &mut self,
        document: &Document,
        selection: TextRange,
        operation: &str,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.runner.invoke(document, selection, operation, output)
    }
}

pub fn shell(
    host: &mut DownstreamShell<'_>,
    input: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let app = App::new(Workspace::read_only(directory.path().to_string_lossy())?)?;
    let mut output = Vec::new();
    cli::run_with_session_and_command(
        app,
        InteractiveSession::new(),
        host,
        Cursor::new(input),
        &mut output,
    )?;
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
    Ok(String::from_utf8(output)?)
}

#[allow(dead_code)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = fixture::MultiLanguage::default();
    let mut host = DownstreamShell::new(&service)?;
    let output = shell(
        &mut host,
        concat!(
            ":scope downstream\n",
            ":scope scratch new example.multi\n:scope line 1 \"unsaved α\"\n",
            ":scope semantic-target 0\n:scope downstream demo echo α $literal\n",
            ":scope downstream demo denied\n:scope session\n:scope quit\n"
        ),
    )?;
    print!("{output}");
    assert_eq!(host.selections, 2);
    assert!(host.runner.records[0].outcome.is_ok());
    assert!(host.runner.records[1].outcome.is_err());
    assert_eq!(*service.calls.lock().unwrap(), ["echo"]);
    println!(
        "Arguments: {:?}; live targets: {:?}",
        host.arguments, host.targets
    );
    Ok(())
}
