use scope_terminal::{History, Outcome, read_line};

fn main() -> std::io::Result<()> {
    let mut history = History::new();
    loop {
        match read_line("> ", &mut history)? {
            Outcome::Submitted(line) => println!("{line}"),
            Outcome::Cancelled => continue,
            Outcome::Eof => break,
            Outcome::Continue => unreachable!("read_line only returns terminal outcomes"),
        }
    }
    Ok(())
}
