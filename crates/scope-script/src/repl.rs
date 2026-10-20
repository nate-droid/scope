//! Standalone terminal input and bounded, prompt-free host-supplied streams.
use crate::{Host, Interpreter, Value};
use std::io::{self, BufRead, Write};

pub fn run(input: impl BufRead, mut output: impl Write, host: &mut impl Host) -> io::Result<()> {
    let mut interpreter = Interpreter::new();
    let mut input = input;
    loop {
        // Bound a line before decoding or allocating an arbitrary input stream.
        let mut bytes = Vec::new();
        let n = input.by_ref().take(65_538).read_until(b'\n', &mut bytes)?;
        if n == 0 {
            break;
        }
        if n > 65_536 {
            writeln!(output, "input line too large; session ended")?;
            break;
        }
        let Ok(line) = std::str::from_utf8(&bytes) else {
            writeln!(output, "input is not UTF-8")?;
            continue;
        };
        if line.trim() == ":quit" {
            break;
        }
        evaluate(&mut interpreter, line, &mut output, host)?;
    }
    Ok(())
}
use std::io::Read;

/// The shared reader restores terminal state before evaluation or output occurs.
/// Call only when stdin and stdout are terminals; `run` retains bounded stream IO.
pub fn run_terminal(host: &mut impl Host) -> io::Result<()> {
    let mut interpreter = Interpreter::new();
    let mut history = scope_terminal::History::new();
    loop {
        let mut completion = crate::completion::ScriptCompletionProvider::new(&interpreter, &*host);
        match scope_terminal::read_line_with_completion("scope> ", &mut history, &mut completion)? {
            scope_terminal::Outcome::Submitted(line) => {
                if line.trim() == ":quit" {
                    return Ok(());
                }
                let mut output = io::stdout().lock();
                evaluate(&mut interpreter, &line, &mut output, host)?;
                output.flush()?;
            }
            scope_terminal::Outcome::Cancelled => continue,
            scope_terminal::Outcome::Eof => return Ok(()),
            scope_terminal::Outcome::Continue => {
                unreachable!("read_line returns a completed outcome")
            }
        }
    }
}

fn evaluate(
    interpreter: &mut Interpreter,
    line: &str,
    output: &mut impl Write,
    host: &mut impl Host,
) -> io::Result<()> {
    let result = interpreter.eval(line, host);
    for value in interpreter.take_output() {
        writeln!(output, "{value:?}")?;
    }
    match result {
        Ok(Value::Null) => {}
        Ok(value) => writeln!(output, "=> {value:?}")?,
        Err(error) => writeln!(output, "{error}")?,
    }
    Ok(())
}
