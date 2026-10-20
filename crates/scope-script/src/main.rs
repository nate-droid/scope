use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("scope-script: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next() else {
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
            return Ok(scope_script::repl::run_terminal(
                &mut scope_script::host::NoHost,
            )?);
        }
        return Ok(scope_script::repl::run(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            &mut scope_script::host::NoHost,
        )?);
    };
    if args.next().is_some() {
        return Err("expected a single script path; use --help for usage".into());
    }
    if path == "--help" {
        println!(
            "Usage: scope-script [FILE]\n       scope-script --help\n\nWith no arguments, start the REPL.\nExecute a UTF-8 script file (maximum 64 KiB) with no host providers or authority.\nOnly explicit print output is written in file mode."
        );
        return Ok(());
    }

    let path = std::path::Path::new(&path);
    // Match the parser's source limit, with one extra byte to detect overflow.
    const MAX_SOURCE: u64 = 65_536;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_SOURCE + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > MAX_SOURCE as usize {
        return Err(format!(
            "{}: source size limit exceeded (maximum 64 KiB)",
            path.display()
        )
        .into());
    }
    let source = std::str::from_utf8(&bytes)
        .map_err(|error| format!("{}: input is not UTF-8: {error}", path.display()))?;
    let mut interpreter = scope_script::Interpreter::new();
    let result = interpreter.eval(source, &mut scope_script::host::NoHost);
    let mut output = std::io::stdout().lock();
    for value in interpreter.take_output() {
        writeln!(output, "{value:?}")?;
    }
    output.flush()?;
    result.map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(())
}
