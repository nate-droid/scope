//! Transactional cooked-mode editing; coordinates are one-based Unicode scalars.

use std::io::{BufRead, Write};

use crate::text::{Span, TextBuffer, TextEdit};

pub(super) fn run(
    buffers: &mut crate::scratch::ScratchBuffers,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
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
            return Ok(());
        }
        if input_line.ends_with('\n') {
            input_line.pop();
            if input_line.ends_with('\r') {
                input_line.pop();
            }
        }
        if !input_line.starts_with(':') {
            let insertion = format!("{input_line}\n");
            draft.insert_str(cursor, &insertion);
            cursor += insertion.len();
            continue;
        }
        let (command, arguments) = input_line
            .split_once(char::is_whitespace)
            .unwrap_or((&input_line, ""));
        let result: Result<(), String> = match command {
            ":done" if arguments.trim().is_empty() => {
                if draft != original {
                    let range =
                        TextBuffer::new(&original).range_for_span(Span::new(0, original.len()))?;
                    buffers
                        .selected_mut()?
                        .apply_edit(TextEdit::replace(range, draft))?;
                }
                return Ok(());
            }
            ":cancel" if arguments.trim().is_empty() => return Ok(()),
            ":help" if arguments.trim().is_empty() => {
                writeln!(
                    output,
                    ":at LINE COLUMN (one-based Unicode scalars)\n:insert TEXT (escapes: \\n \\t \\\\; literal colons allowed)\n:delete COUNT (forward, including newlines)\n:show [START] (up to 40 lines, 160 characters each)\n:done (commit)\n:cancel (discard; EOF also discards)\n:help\nOther lines insert literally, followed by a newline."
                )?;
                Ok(())
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
            ":insert" => decode(arguments).map(|text| {
                draft.insert_str(cursor, &text);
                cursor += text.len();
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
                        draft.replace_range(cursor..cursor + bytes, "");
                        Ok(())
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

fn coordinates(text: &str, cursor: usize) -> (usize, usize) {
    let prefix = &text[..cursor];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
    )
}

fn offset(text: &str, line: usize, column: usize) -> Option<usize> {
    let line_index = line.checked_sub(1)?;
    let column_index = column.checked_sub(1)?;
    let mut start = 0;
    for (index, contents) in text.split('\n').enumerate() {
        if index == line_index {
            return contents
                .char_indices()
                .map(|(byte, _)| byte)
                .chain(std::iter::once(contents.len()))
                .nth(column_index)
                .map(|byte| start + byte);
        }
        start += contents.len() + 1;
    }
    None
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
