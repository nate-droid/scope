//! Transactional source editing; command coordinates are one-based Unicode scalars.

use std::io::{BufRead, Write};

use crate::text::{Span, TextBuffer, TextEdit};

#[cfg(test)]
pub(super) fn run(
    buffers: &mut crate::scratch::ScratchBuffers,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    run_with_completion(buffers, input, output, &|_, _, _| Vec::new()).map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorOutcome {
    Committed,
    Canceled,
}

pub(super) fn run_with_completion(
    buffers: &mut crate::scratch::ScratchBuffers,
    input: &mut impl BufRead,
    output: &mut impl Write,
    complete: &impl Fn(
        &crate::scratch::ScratchBuffer,
        &str,
        usize,
    ) -> Vec<crate::language::CompletionItem>,
) -> Result<EditorOutcome, Box<dyn std::error::Error>> {
    let selected = buffers.selected()?;
    let original = selected.text().to_owned();
    let identity = format!(
        "buffer {} ({}){}",
        selected.id(),
        selected.language_id(),
        selected
            .destination_path()
            .map(|path| format!(" {path}"))
            .unwrap_or_default()
    );
    let original_dirty = selected.is_dirty();
    let target = (selected.id(), selected.version());
    let mut draft = original.clone();
    let mut cursor = 0;
    let mut input_line = String::new();
    loop {
        let (line, column) = coordinates(&draft, cursor);
        write!(
            output,
            "{identity}{} {line}:{column}> ",
            if original_dirty || draft != original {
                " *"
            } else {
                ""
            }
        )?;
        output.flush()?;
        input_line.clear();
        if input.read_line(&mut input_line)? == 0 {
            writeln!(output, "Draft canceled at EOF; original buffer unchanged.")?;
            return Ok(EditorOutcome::Canceled);
        }
        if input_line.ends_with('\n') {
            input_line.pop();
            if input_line.ends_with('\r') {
                input_line.pop();
            }
        }
        // Cooked-mode navigation arrives as text, not editor commands. Reject the
        // whole line so neither surrounding text nor terminal controls are applied.
        if input_line
            .chars()
            .any(|ch| ch == '\u{1b}' || ('\u{80}'..='\u{9f}').contains(&ch))
        {
            writeln!(
                output,
                "error: input line rejected: terminal escape/control sequences are not supported; use :at to move the cursor."
            )?;
            continue;
        }
        if !input_line.starts_with(':') {
            let insertion = format!("{input_line}\n");
            replace_draft(&mut draft, cursor..cursor, &insertion)?;
            cursor += insertion.len();
            continue;
        }
        let (command, arguments) = input_line
            .split_once(char::is_whitespace)
            .unwrap_or((&input_line, ""));
        let result: Result<(), String> = match command {
            ":done" if arguments.trim().is_empty() => {
                if draft != original {
                    commit(buffers, target, &original, draft)?;
                }
                return Ok(EditorOutcome::Committed);
            }
            ":cancel" if arguments.trim().is_empty() => return Ok(EditorOutcome::Canceled),
            ":help" if arguments.trim().is_empty() => {
                writeln!(
                    output,
                    ":at LINE COLUMN (one-based Unicode scalars)\n:insert TEXT (escapes: \\n \\t \\\\; literal colons allowed)\n:delete COUNT (forward, including newlines)\n:show [START] (up to 40 lines, 160 characters each)\n:complete [INDEX] (list source completions or apply a current completion)\n:done (commit)\n:cancel (discard; EOF also discards)\n:help\nOther lines insert literally, followed by a newline."
                )?;
                Ok(())
            }
            ":complete" => {
                let items = crate::shell_input::valid_completions(
                    &draft,
                    complete(buffers.selected()?, &draft, cursor),
                );
                if arguments.trim().is_empty() {
                    for (index, item) in items.iter().take(40).enumerate() {
                        let label: String = item
                            .label
                            .chars()
                            .take(160)
                            .flat_map(char::escape_debug)
                            .collect();
                        writeln!(output, "{}: {label}", index + 1)?;
                    }
                    writeln!(
                        output,
                        "{} completion(s); :complete INDEX to apply (recomputed).",
                        items.len()
                    )?;
                    Ok(())
                } else {
                    arguments
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| index.checked_sub(1))
                        .and_then(|index| items.into_iter().nth(index))
                        .ok_or_else(|| {
                            "usage: :complete [INDEX]; INDEX must name a current completion"
                                .to_owned()
                        })
                        .and_then(|item| {
                            crate::shell_input::apply_completion(&mut draft, &mut cursor, item)
                                .map_err(str::to_owned)
                        })
                }
            }
            ":at" => {
                let values: Vec<_> = arguments.split_whitespace().collect();
                if values.len() != 2 {
                    Err("usage: :at LINE COLUMN".into())
                } else {
                    values[0]
                        .parse::<usize>()
                        .ok()
                        .zip(values[1].parse::<usize>().ok())
                        .and_then(|(line, column)| offset(&draft, line, column))
                        .map(|position| cursor = position)
                        .ok_or_else(|| "cursor is out of bounds (coordinates start at 1)".into())
                }
            }
            ":insert" => decode(arguments).and_then(|text| {
                replace_draft(&mut draft, cursor..cursor, &text).map_err(|e| e.to_string())?;
                cursor += text.len();
                Ok(())
            }),
            ":delete" => match arguments.trim().parse::<usize>() {
                Ok(count) => {
                    let mut chars = draft[cursor..].chars();
                    let mut bytes = 0;
                    let mut valid = true;
                    for _ in 0..count {
                        match chars.next() {
                            Some(ch) => bytes += ch.len_utf8(),
                            None => {
                                valid = false;
                                break;
                            }
                        }
                    }
                    if valid {
                        replace_draft(&mut draft, cursor..cursor + bytes, "")
                            .map_err(|e| e.to_string())
                    } else {
                        Err("delete extends past the end of the draft".into())
                    }
                }
                Err(_) => Err("usage: :delete COUNT".into()),
            },
            ":show" => {
                let start = if arguments.trim().is_empty() {
                    Some(1)
                } else {
                    arguments.trim().parse::<usize>().ok()
                };
                match start.filter(|start| *start > 0 && *start <= draft.split('\n').count()) {
                    Some(start) => {
                        for (index, line) in draft.split('\n').enumerate().skip(start - 1).take(40)
                        {
                            let preview: String = line
                                .chars()
                                .take(160)
                                .flat_map(char::escape_debug)
                                .collect();
                            writeln!(
                                output,
                                "{}: {preview}{}",
                                index + 1,
                                if line.chars().count() > 160 {
                                    " …"
                                } else {
                                    ""
                                }
                            )?;
                        }
                        Ok(())
                    }
                    None => {
                        Err("usage: :show [START]; START must be an existing one-based line".into())
                    }
                }
            }
            _ => Err(format!(
                "unknown or invalid editor command: {command}; use :help"
            )),
        };
        if let Err(error) = result {
            writeln!(output, "error: {error}")?;
        }
    }
}

fn replace_draft(
    draft: &mut String,
    range: std::ops::Range<usize>,
    text: &str,
) -> Result<(), crate::text::TextBufferError> {
    let mut buffer = TextBuffer::new(draft.as_str());
    let range = buffer.range_for_span(Span::from(range))?;
    buffer.apply_edit(TextEdit::replace(range, text))?;
    *draft = buffer.into_string();
    Ok(())
}

fn commit(
    buffers: &mut crate::scratch::ScratchBuffers,
    target: (u64, u64),
    original: &str,
    draft: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let selected = buffers.selected_mut()?;
    if (selected.id(), selected.version()) != target || selected.text() != original {
        return Err("draft target changed; edit canceled without overwriting source".into());
    }
    if draft != original {
        let range = TextBuffer::new(original).range_for_span(Span::new(0, original.len()))?;
        selected.apply_edit(TextEdit::replace(range, draft))?;
    }
    Ok(())
}

struct KeyboardDraft {
    text: String,
    cursor: usize,
}

impl scope_terminal::DocumentEditor for KeyboardDraft {
    fn text(&self) -> &str {
        &self.text
    }
    fn cursor(&self) -> usize {
        self.cursor
    }
    fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
    }
    fn replace(&mut self, range: std::ops::Range<usize>, text: &str) -> std::io::Result<()> {
        replace_draft(&mut self.text, range, text).map_err(std::io::Error::other)
    }
}

pub(super) fn run_keyboard(
    buffers: &mut crate::scratch::ScratchBuffers,
    output: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let selected = buffers.selected()?;
    let target = (selected.id(), selected.version());
    let original = selected.text().to_owned();
    let mut draft = KeyboardDraft {
        text: original.clone(),
        cursor: 0,
    };
    if scope_terminal::edit_document(&mut draft, output)? {
        commit(buffers, target, &original, draft.text)?;
    }
    Ok(())
}

fn coordinates(text: &str, cursor: usize) -> (usize, usize) {
    TextBuffer::new(text)
        .scalar_coordinates(cursor)
        .expect("valid draft cursor")
}

fn offset(text: &str, line: usize, column: usize) -> Option<usize> {
    TextBuffer::new(text).scalar_offset(line, column)
}

fn decode(text: &str) -> Result<String, String> {
    let mut result = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        result.push(if ch == '\\' {
            match chars.next() {
                Some('n') => '\n',
                Some('t') => '\t',
                Some('\\') => '\\',
                _ => return Err("invalid escape; supported escapes are \\n, \\t, and \\\\".into()),
            }
        } else {
            ch
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchBuffers;
    use std::io::Cursor;

    fn edit(source: &str, commands: &str) -> (ScratchBuffers, String) {
        let mut buffers = ScratchBuffers::default();
        buffers.create("text", source);
        let mut output = Vec::new();
        run(&mut buffers, &mut Cursor::new(commands), &mut output).unwrap();
        (buffers, String::from_utf8(output).unwrap())
    }

    #[test]
    fn commit_rejects_stale_revision_and_changed_selection() {
        let mut buffers = ScratchBuffers::default();
        buffers.create("text", "original");
        let target = (buffers.selected().unwrap().id(), 0);
        buffers
            .selected_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(crate::text::Position::new(0, 0), "new "))
            .unwrap();
        assert!(commit(&mut buffers, target, "original", "draft".into()).is_err());
        assert_eq!(buffers.selected().unwrap().text(), "new original");
        buffers.create("text", "other");
        assert!(commit(&mut buffers, target, "original", "draft".into()).is_err());
        assert_eq!(buffers.selected().unwrap().text(), "other");
    }

    #[test]
    fn cooked_reentry_inserts_instead_of_replacing_existing_source() {
        // Reproduces the reported accumulation: plain lines are insertions,
        // so reentering a program retains the old source after the cursor.
        let mut buffers = ScratchBuffers::default();
        buffers.create("text", "old program\nold error\n");
        run(
            &mut buffers,
            &mut Cursor::new("new program\n:done\n"),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(
            buffers.selected().unwrap().text(),
            "new program\nold program\nold error\n"
        );
        assert_eq!(buffers.selected().unwrap().version(), 1);
    }

    #[test]
    fn keyboard_and_command_drafts_share_source_edits_and_commit_semantics() {
        use scope_terminal::DocumentEditor;
        let source = "let x = 6;\nlet y = 7;\nprint({x: x, y: y});\nx * y";
        let mut explicit = ScratchBuffers::default();
        explicit.create("scope-script", source);
        run(
            &mut explicit,
            &mut Cursor::new(":at 2 9\n:delete 1\n:insert 8\n:done\n"),
            &mut Vec::new(),
        )
        .unwrap();
        let mut human = ScratchBuffers::default();
        human.create("scope-script", source);
        let target = (human.selected().unwrap().id(), 0);
        let cursor = TextBuffer::new(source).scalar_offset(2, 9).unwrap();
        let mut draft = KeyboardDraft {
            text: source.into(),
            cursor,
        };
        draft.replace(cursor..cursor + 1, "").unwrap();
        draft.replace(cursor..cursor, "8").unwrap();
        commit(&mut human, target, source, draft.text).unwrap();
        assert_eq!(
            human.selected().unwrap().text(),
            explicit.selected().unwrap().text()
        );
        assert_eq!(human.selected().unwrap().version(), 1);
        let accepted = human.selected().unwrap().text().to_owned();
        commit(&mut human, (target.0, 1), &accepted, accepted.clone()).unwrap();
        assert_eq!(human.selected().unwrap().version(), 1);
    }

    #[test]
    fn completion_inspects_draft_and_commits_only_on_done() {
        for finish in [":done", ":cancel"] {
            let mut buffers = ScratchBuffers::default();
            buffers.create("text", "é");
            let mut output = Vec::new();
            run_with_completion(
                &mut buffers,
                &mut Cursor::new(format!(
                    ":at 1 2\n:insert x\n:complete\n:complete 1\n{finish}\n"
                )),
                &mut output,
                &|source, draft, cursor| {
                    assert_eq!(source.text(), "é");
                    assert_eq!(source.version(), 0);
                    assert_eq!(draft, "éx");
                    assert_eq!(cursor, 3);
                    vec![
                        crate::language::CompletionItem {
                            label: "invalid".into(),
                            replacement: 1..2,
                            insert_text: "bad".into(),
                            detail: None,
                        },
                        crate::language::CompletionItem {
                            label: "expanded".into(),
                            replacement: 2..3,
                            insert_text: "猫\n".into(),
                            detail: None,
                        },
                    ]
                },
            )
            .unwrap();
            let selected = buffers.selected().unwrap();
            assert_eq!(
                selected.text(),
                if finish == ":done" { "é猫\n" } else { "é" }
            );
            assert_eq!(selected.version(), if finish == ":done" { 1 } else { 0 });
            assert!(String::from_utf8(output).unwrap().contains("1: expanded"));
        }
    }

    #[test]
    fn multiline_unicode_and_forward_deletion() {
        let (buffers, output) = edit(
            "é猫\n🙂x\n",
            ":at 1 2\n:delete 2\n:insert Ω\\n\\t\\\\:literal\n:done\n",
        );
        assert_eq!(buffers.selected().unwrap().text(), "éΩ\n\t\\:literal🙂x\n");
        assert_eq!(buffers.selected().unwrap().version(), 1);
        assert!(output.contains("1:2>"));
        assert!(output.contains("2:11>"));
    }

    #[test]
    fn literal_lines_preserve_spaces_and_add_newline() {
        let (buffers, _) = edit("", "  猫  \n\n:insert :colon\n:done\n");
        assert_eq!(buffers.selected().unwrap().text(), "  猫  \n\n:colon");
    }

    #[test]
    fn cancel_and_eof_are_transactional() {
        for commands in [":insert changed\n:cancel\n", ":insert changed\n", ""] {
            let (buffers, _) = edit("original", commands);
            let buffer = buffers.selected().unwrap();
            assert_eq!(buffer.text(), "original");
            assert_eq!(buffer.version(), 0);
            assert!(buffer.is_dirty());
        }
    }

    #[test]
    fn invalid_commands_leave_cursor_and_draft_unchanged() {
        let (buffers, output) = edit(
            "猫\n",
            ":at 0 1\n:at 1 3\n:at 3 1\n:at 1 2\n:delete 3\n:insert bad\\q\n:unknown\n:insert !\n:done\n",
        );
        assert_eq!(buffers.selected().unwrap().text(), "猫!\n");
        assert_eq!(output.matches("error:").count(), 6);
    }

    #[test]
    fn terminal_controls_reject_whole_lines_before_insertion_or_dispatch() {
        for control in [
            "\u{1b}[A", "\u{1b}OD", "\u{1b}", "\u{85}", "\u{9b}D", "\u{9d}",
        ] {
            for line in [
                format!("prefix{control}suffix"),
                format!(":insert prefix{control}suffix"),
                format!(":unknown{control}"),
                format!(":at 1 1{control}"),
                format!(":done {control}"),
                format!(":cancel {control}"),
            ] {
                let (buffers, output) =
                    edit("猫x", &format!(":at 1 2\n{line}\r\n:insert !\n:done\n"));
                let buffer = buffers.selected().unwrap();
                assert_eq!(buffer.text(), "猫!x", "{line:?}");
                assert_eq!(buffer.version(), 1);
                assert_eq!(output.matches("error: input line rejected:").count(), 1);
                assert!(!output.contains(control), "unsafe diagnostic: {output:?}");
            }
        }
    }

    #[test]
    fn rejected_lines_preserve_transaction_semantics() {
        for ending in [":done\n", ":cancel\n", ""] {
            let (buffers, output) = edit("original", &format!("bad\u{1b}[Dtext\n{ending}"));
            let buffer = buffers.selected().unwrap();
            assert_eq!(buffer.text(), "original");
            assert_eq!(buffer.version(), 0);
            assert!(buffer.is_dirty());
            assert!(output.contains("error: input line rejected:"));

            let (buffers, _) = edit(
                "original",
                &format!(":insert good\nbad\u{9b}Dtext\n{ending}"),
            );
            let buffer = buffers.selected().unwrap();
            assert_eq!(
                buffer.text(),
                if ending == ":done\n" {
                    "goodoriginal"
                } else {
                    "original"
                }
            );
            assert_eq!(buffer.version(), if ending == ":done\n" { 1 } else { 0 });
        }
    }

    #[test]
    fn show_is_bounded_by_lines_and_unicode_scalars() {
        let source = vec!["猫".repeat(200); 50].join("\n");
        let (_, output) = edit(&source, ":show\n:cancel\n");
        assert_eq!(output.matches(&"猫".repeat(160)).count(), 40);
        assert!(!output.contains(&"猫".repeat(161)));
        assert!(!output.contains("41: "));
        let (_, output) = edit(&source, ":show 49\n:cancel\n");
        assert!(output.contains("49: "));
        assert!(output.contains("50: "));
        assert!(!output.contains("48: "));
    }

    #[test]
    fn trailing_empty_line_and_unchanged_done() {
        let (buffers, output) = edit("é\n", ":at 2 1\n:done\n");
        assert_eq!(buffers.selected().unwrap().version(), 0);
        assert!(output.contains("2:1>"));
    }
}
