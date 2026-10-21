//! Run `cargo run -p scope-script --example semantic -- --allow-read [SCRIPT.scope]`.
//! The optional file is Script source, not the semantic target. The target remains
//! the fixed declaration snapshot; no filesystem or Path operations are installed.
#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod language;
use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
    text::{Position, TextRange},
    tool::{AgentSession, semantic::SemanticToolProvider},
};
use scope_language::SemanticOperationEffect;
use scope_script::{Error, Interpreter, Value, semantic::SemanticHost};
use std::io::Read;

const MAX_SCRIPT_BYTES: usize = 64 * 1024;

fn read_script(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| {
            file.take((MAX_SCRIPT_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > MAX_SCRIPT_BYTES {
        return Err(format!("{}: Script exceeds 64 KiB", path.display()).into());
    }
    String::from_utf8(bytes)
        .map_err(|error| format!("{}: input is not UTF-8: {error}", path.display()).into())
}

fn diagnostic(file: &str, source: &str, error: &Error) -> String {
    let mut offset = error.offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let prefix = &source[..offset];
    let line = 1 + prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = 1 + prefix.rsplit('\n').next().unwrap_or("").chars().count();
    format!("{file}:{line}:{column}: {error}")
}

struct RunResult {
    output: Vec<Value>,
    outcome: Result<Value, Error>,
}

fn main() -> std::process::ExitCode {
    match execute() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("semantic: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (file, authorization) = match args.as_slice() {
        [] => (None, None),
        [flag] if flag == "--allow-read" => (None, Some(true)),
        [file] if !file.starts_with('-') => (Some(file), None),
        [flag, file] if flag == "--allow-read" && !file.starts_with('-') => {
            (Some(file), Some(true))
        }
        _ => return Err("usage: semantic [--allow-read] [SCRIPT.scope]".into()),
    };
    let script = match file {
        Some(file) => read_script(std::path::Path::new(file))?,
        None => include_str!("semantic.scope").to_owned(),
    };
    let result = run(&script, authorization)?;
    for value in result.output {
        println!("{value:?}");
    }
    result.outcome.map_err(|error| {
        diagnostic(
            file.map(String::as_str).unwrap_or("semantic.scope"),
            &script,
            &error,
        )
    })?;
    Ok(())
}

fn run(script: &str, authorization: Option<bool>) -> Result<RunResult, Box<dyn std::error::Error>> {
    let service = language::DeclarationLanguage::default();
    let mut buffers = ScratchBuffers::default();
    buffers.create("example.declarations", language::SOURCE);
    let target = buffers.selected().unwrap();
    let before = (target.text().to_owned(), target.version());
    let mut controller = SourceEditController::default();
    let mut interpreter = Interpreter::new();
    let result = {
        let provider = SemanticToolProvider::new(
            &service,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(TextRange::new(Position::new(0, 0), Position::new(0, 15))),
            |request| {
                request.operation.id == "describe_selection"
                    && request.operation.effect == SemanticOperationEffect::Read
                    && request.source.text == before.0
                    && request.source.version == before.1
                    && request.arguments == &serde_json::json!({})
            },
        )?;
        let session = AgentSession::builder().provider(provider).build()?;
        let mut host = SemanticHost::new(
            session,
            "language.example.declarations.describe_selection",
            target.text(),
            target.version(),
            authorization,
        );
        interpreter.eval(script, &mut host)
    };
    assert_eq!((target.text().to_owned(), target.version()), before);
    assert_eq!(controller.list().count(), 0);
    Ok(RunResult {
        output: interpreter.take_output(),
        outcome: result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_script_uses_real_semantic_bridge() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("inspect.scope");
        let script = include_str!("semantic.scope");
        std::fs::write(&path, script).unwrap();
        let result = run(&read_script(&path).unwrap(), Some(true)).unwrap();
        assert!(result.outcome.is_ok());
        let output = result.output;
        assert_eq!(output[1], Value::String("answer".into()));
        assert_eq!(output[2], Value::String("integer_literal_binding".into()));
        assert_eq!(output[4], Value::String("unknown".into()));
        assert_eq!(std::fs::read_to_string(path).unwrap(), script);
    }

    #[test]
    fn missing_and_denied_authorization_fail() {
        for authorization in [None, Some(false)] {
            assert!(
                run(include_str!("semantic.scope"), authorization)
                    .unwrap()
                    .outcome
                    .is_err()
            );
        }
    }

    #[test]
    fn prints_survive_errors_and_interpreters_are_fresh() {
        let source = "let saved = 1; print(\"before\");\n  missing";
        let result = run(source, Some(true)).unwrap();
        assert_eq!(result.output, vec![Value::String("before".into())]);
        let error = result.outcome.unwrap_err();
        assert!(diagnostic("broken.scope", source, &error).starts_with("broken.scope:2:3:"));
        let next = run("saved", Some(true)).unwrap();
        assert!(next.output.is_empty());
        assert!(next.outcome.is_err());
    }

    #[test]
    fn file_limit_and_utf8_are_checked() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("script.scope");
        std::fs::write(&path, vec![b' '; MAX_SCRIPT_BYTES]).unwrap();
        assert_eq!(read_script(&path).unwrap().len(), MAX_SCRIPT_BYTES);
        std::fs::write(&path, vec![b' '; MAX_SCRIPT_BYTES + 1]).unwrap();
        assert!(
            read_script(&path)
                .unwrap_err()
                .to_string()
                .contains("64 KiB")
        );
        std::fs::write(&path, [0xff]).unwrap();
        assert!(read_script(&path).is_err());
    }

    #[test]
    fn unicode_diagnostic_columns_count_scalars() {
        let source = "print(\"🦀\"); missing";
        let error = run(source, Some(true)).unwrap().outcome.unwrap_err();
        assert!(diagnostic("unicode.scope", source, &error).starts_with("unicode.scope:1:13:"));
    }

    #[test]
    fn semantic_rejection_is_not_interpreter_failure() {
        let output = run(
            r#"let operation = scope.operations()[0];
            let result = scope.invoke(operation.provider, operation.action, {unexpected: true});
            print(result.status);"#,
            Some(true),
        )
        .unwrap();
        assert!(output.outcome.is_ok());
        assert_eq!(
            output.output,
            vec![Value::String("invalid_arguments".into())]
        );
    }
}
