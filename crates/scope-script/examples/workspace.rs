//! Read-only downstream workspace host; no implicit root or authority.
mod workspace_support;
use scope_script::Interpreter;
use std::io::Read;
use workspace_support::{SCRIPT, WorkspaceHost};

const USAGE: &str = "usage: workspace --allow-read ROOT [SCRIPT]\nWithout SCRIPT, runs the embedded handoff report.\nSCRIPT is a local UTF-8 file (maximum 64 KiB). Read access is denied by default.";

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--help"] || args.as_slice() == ["-h"] {
        println!("{USAGE}");
        return Ok(());
    }
    if !(args.len() == 2 || args.len() == 3) || args[0] != "--allow-read" {
        return Err(format!("read access requires explicit --allow-read ROOT\n{USAGE}").into());
    }
    let source = if let Some(path) = args.get(2) {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| format!("cannot open script {path}: {e}"))?
            .take(65_537)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 65_536 {
            return Err("script exceeds 64 KiB source limit".into());
        }
        String::from_utf8(bytes).map_err(|e| format!("script must be UTF-8: {e}"))?
    } else {
        SCRIPT.to_owned()
    };
    let mut host = WorkspaceHost::open(&args[1], true)?;
    let mut interpreter = Interpreter::new();
    let result = interpreter.eval(&source, &mut host);
    let output = interpreter.take_output();
    for value in &output {
        println!("{value:#?}");
    }
    match result {
        Ok(value) if output.is_empty() => println!("{value:#?}"),
        Ok(_) => (),
        Err(error) => {
            if let Some(cause) = &host.last_error {
                eprintln!("host cause: {cause}");
            }
            return Err(format!(
                "{}: {error}",
                args.get(2)
                    .map_or("embedded workspace.scope", String::as_str)
            )
            .into());
        }
    }
    let _ = workspace_support::direct;
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("workspace: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
