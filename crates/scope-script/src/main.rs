use scope_script::presentation::{format_value, source_location, value_json};
use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

#[derive(Clone, Copy, Default, PartialEq)]
enum Output {
    #[default]
    Prints,
    Summary,
    Json,
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("scope-script: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty() {
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
            scope_script::repl::run_terminal(&mut scope_script::host::NoHost)?;
        } else {
            scope_script::repl::run(
                std::io::stdin().lock(),
                std::io::stdout().lock(),
                &mut scope_script::host::NoHost,
            )?;
        }
        return Ok(true);
    }
    if args.len() == 1 && args[0] == "--help" {
        println!(
            "Usage: scope-script [--output prints|summary|json] FILE\n       scope-script --help\n\nWith no arguments, start the REPL.\nExecute a UTF-8 script file (maximum 64 KiB) with no host providers or authority.\n\nOutput modes:\n  prints   Explicit print output only (default).\n  summary  Print output, followed by labeled final value and interpreter status.\n  json     One standalone scope-script.execution version 1 record containing\n           source path/text, full printed values, tagged outcome, and no-host metadata.\n           Evaluation errors include byte offset, 1-based line/column, message,\n           and the complete error source chain. No semantic inference is performed.\n\nEvaluation errors exit with failure; JSON evaluation errors are reported only on stdout."
        );
        return Ok(true);
    }
    let mut mode = Output::default();
    let mut path = None;
    let mut output_seen = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--output" {
            if output_seen {
                return Err("--output may only be specified once".into());
            }
            output_seen = true;
            mode = match args.next().as_deref().and_then(|value| value.to_str()) {
                Some("prints") => Output::Prints,
                Some("summary") => Output::Summary,
                Some("json") => Output::Json,
                _ => {
                    return Err(
                        "expected --output prints|summary|json; use --help for usage".into(),
                    );
                }
            };
        } else if arg.to_string_lossy().starts_with('-') {
            return Err(format!(
                "unknown option {}; use --help for usage",
                arg.to_string_lossy()
            )
            .into());
        } else if path.replace(arg).is_some() {
            return Err("expected a single script path; use --help for usage".into());
        }
    }
    let path = path.ok_or("expected a script path; use --help for usage")?;
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
    let printed = interpreter.take_output();
    let mut output = std::io::stdout().lock();
    if mode == Output::Json {
        let record = execution_record(path, source, &printed, &result);
        serde_json::to_writer(&mut output, &record)?;
        writeln!(output)?;
    } else {
        for value in &printed {
            writeln!(output, "{}", format_value(value))?;
        }
        if mode == Output::Summary {
            match &result {
                Ok(value) => writeln!(output, "Final value: {}", format_value(value))?,
                Err(_) => writeln!(output, "Final value: unavailable (evaluation failed)")?,
            }
            writeln!(
                output,
                "Interpreter status: {}",
                if result.is_ok() { "success" } else { "error" }
            )?;
        }
    }
    output.flush()?;
    if mode != Output::Json {
        result.as_ref().map_err(|error| {
            let (line, column) = source_location(source, error.offset);
            format!("{}:{line}:{column}: {error}", path.display())
        })?;
    }
    Ok(result.is_ok())
}

fn execution_record(
    path: &std::path::Path,
    source: &str,
    printed: &[scope_script::Value],
    result: &Result<scope_script::Value, scope_script::Error>,
) -> serde_json::Value {
    let outcome = match result {
        Ok(value) => serde_json::json!({"status": "success", "value": value_json(value)}),
        Err(error) => {
            let (line, column) = source_location(source, error.offset);
            let mut chain = Vec::new();
            let mut cause = std::error::Error::source(error);
            while let Some(error) = cause {
                chain.push(error.to_string());
                cause = error.source();
            }
            serde_json::json!({
                "status": "error",
                "error": {
                    "offset": error.offset,
                    "line": line,
                    "column": column,
                    "message": error.to_string(),
                    "source_chain": chain,
                }
            })
        }
    };
    serde_json::json!({
        "schema": "scope-script.execution",
        "version": 1,
        "source": {"path": path, "text": source},
        "printed": printed.iter().map(value_json).collect::<Vec<_>>(),
        "outcome": outcome,
        "host": {"mode": "no-host", "invocations": []},
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_preserves_source_and_full_values() {
        let text = "\"escaped\\ntext\"";
        let value = scope_script::Value::String("x".repeat(10_000));
        let record = execution_record(
            std::path::Path::new("example.scope"),
            text,
            std::slice::from_ref(&value),
            &Ok(value.clone()),
        );
        let encoded = serde_json::to_string(&record).unwrap();
        let decoded: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded["schema"], "scope-script.execution");
        assert_eq!(decoded["version"], 1);
        assert_eq!(decoded["source"]["text"], text);
        assert_eq!(decoded["printed"][0], value_json(&value));
        assert_eq!(decoded["outcome"]["value"], value_json(&value));
        assert_eq!(decoded["host"]["mode"], "no-host");
        assert_eq!(decoded["host"]["invocations"], serde_json::json!([]));
    }

    #[test]
    fn json_error_has_location_and_complete_source_chain() {
        let error = scope_script::Error {
            offset: 4,
            kind: scope_script::ErrorKind::Runtime("failed".into()),
        };
        let record = execution_record(
            std::path::Path::new("example.scope"),
            "é\n x",
            &[scope_script::Value::Integer(7)],
            &Err(error),
        );
        let outcome = &record["outcome"];
        assert_eq!(outcome["status"], "error");
        assert_eq!(outcome["error"]["offset"], 4);
        assert_eq!(outcome["error"]["line"], 2);
        assert_eq!(outcome["error"]["column"], 2);
        assert_eq!(
            outcome["error"]["message"],
            "runtime error: failed at byte 4"
        );
        assert_eq!(
            outcome["error"]["source_chain"],
            serde_json::json!(["runtime error: failed"])
        );
        assert_eq!(record["printed"], serde_json::json!([7]));
    }
}
