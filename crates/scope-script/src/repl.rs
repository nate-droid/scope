//! Line-oriented REPL over host-supplied streams; no file or process access.
use crate::{Host, Interpreter, Value};
use std::io::{self, BufRead, Write};

pub fn run(input: impl BufRead, mut output: impl Write, host: &mut impl Host) -> io::Result<()> {
    let mut interpreter = Interpreter::new();
    let mut input = input;
    loop {
        write!(output, "scope> ")?;
        output.flush()?;
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
        let result = interpreter.eval(line, host);
        for value in interpreter.take_output() {
            writeln!(output, "{value:?}")?;
        }
        match result {
            Ok(Value::Null) => {}
            Ok(value) => writeln!(output, "=> {value:?}")?,
            Err(error) => writeln!(output, "{error}")?,
        }
    }
    Ok(())
}
use std::io::Read;
