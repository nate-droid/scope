//! Run: cargo run -p scope-terminal --example completion
use scope_terminal::{
    CompletionCandidate, History, Outcome, PastePolicy, read_line_with_completion_and_paste_policy,
};

fn main() -> std::io::Result<()> {
    let policy = if std::env::args().any(|arg| arg == "--normalized-layout") {
        PastePolicy::NormalizedLayout
    } else {
        PastePolicy::default()
    };
    let mut history = History::new();
    let mut provider = |input: &str, cursor: usize| {
        ["help", "hello", "halt", "quit"]
            .into_iter()
            .filter(|command| command.starts_with(&input[..cursor]))
            .map(|command| CompletionCandidate {
                replacement: 0..cursor,
                insertion: command.into(),
                label: match command {
                    "help" => Some("help -- show commands".into()),
                    "hello" => Some("hello -- greeting\n\x1b[2J中".into()),
                    _ => None,
                },
            })
            .collect()
    };
    loop {
        match read_line_with_completion_and_paste_policy(
            "commands> ",
            &mut history,
            &mut provider,
            policy,
        )? {
            Outcome::Submitted(line) if line == "quit" => break,
            Outcome::Submitted(line) => println!("submitted: {line:?}"),
            Outcome::Eof => break,
            _ => {}
        }
    }
    Ok(())
}
