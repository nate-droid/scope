use crossterm::{
    cursor::{MoveDown, MoveToColumn, MoveUp},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::{
    io::{self, IsTerminal, Write},
    ops::Range,
};

/// Caller-owned UTF-8 document. Cursors and replacement ranges are byte offsets
/// on scalar boundaries. `replace` must leave the document and cursor valid.
pub trait DocumentEditor {
    fn text(&self) -> &str;
    fn cursor(&self) -> usize;
    fn set_cursor(&mut self, cursor: usize);
    fn replace(&mut self, range: Range<usize>, text: &str) -> io::Result<()>;
}

/// Whether both standard streams support interactive document editing.
pub fn document_editing_available() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

struct Session<'a, W: Write> {
    output: &'a mut W,
    restore_raw: bool,
    view: View,
}
impl<W: Write> Drop for Session<'_, W> {
    fn drop(&mut self) {
        let _ = self.view.finish(self.output);
        let _ = queue!(self.output, DisableBracketedPaste);
        let _ = self.output.flush();
        if self.restore_raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}

/// Edit a caller-owned document using stdin events and the supplied terminal
/// output. Ctrl-S accepts; Ctrl-C or Escape cancels. Cancellation does not undo
/// edits: callers needing rollback should provide a transactional document.
/// Redirected standard streams return `Unsupported` without changing the model.
/// The output must address the same terminal as stdout. Raw mode, bracketed
/// paste are restored on every return, including errors. The inline viewport
/// uses at most 12 rows, including help, and leaves the cursor at column zero
/// on its last row on return. No alternate screen is used.
pub fn edit_document(
    document: &mut impl DocumentEditor,
    output: &mut impl Write,
) -> io::Result<bool> {
    if !document_editing_available() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "document editing requires terminal stdin and stdout",
        ));
    }
    validate(document)?;
    let restore_raw = !terminal::is_raw_mode_enabled()?;
    if restore_raw {
        terminal::enable_raw_mode()?;
    }
    let mut session = Session {
        output,
        restore_raw,
        view: View::default(),
    };
    queue!(session.output, EnableBracketedPaste)?;
    let mut preferred_column = None;
    loop {
        validate(document)?;
        let (width, height) = terminal::size()?;
        session.view.draw(document, session.output, width, height)?;
        if let Some(accepted) =
            apply_document_event(document, event::read()?, &mut preferred_column)?
        {
            return Ok(accepted);
        }
    }
}

fn validate(d: &impl DocumentEditor) -> io::Result<()> {
    if d.text().is_char_boundary(d.cursor()) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "document cursor is not a UTF-8 boundary",
        ))
    }
}
fn line_start(text: &str, cursor: usize) -> usize {
    text[..cursor].rfind('\n').map_or(0, |p| p + 1)
}
fn line_end(text: &str, cursor: usize) -> usize {
    text[cursor..].find('\n').map_or(text.len(), |p| cursor + p)
}
fn replace(d: &mut impl DocumentEditor, range: Range<usize>, text: &str) -> io::Result<()> {
    let cursor = range.start + text.len();
    d.replace(range, text)?;
    d.set_cursor(cursor);
    validate(d)
}
/// Applies one event without owning terminal input, rendering, or raw mode.
/// Returns `Some(true)` for Ctrl-S and `Some(false)` for Ctrl-C/Escape.
/// The caller owns rollback and must reserve its own host shortcuts first.
/// `preferred` retains the scalar column across vertical movement.
pub fn apply_document_event(
    d: &mut impl DocumentEditor,
    event: Event,
    preferred: &mut Option<usize>,
) -> io::Result<Option<bool>> {
    validate(d)?;
    let key = match event {
        Event::Paste(text) => {
            *preferred = None;
            let cursor = d.cursor();
            replace(d, cursor..cursor, &crate::sanitize_paste(&text))?;
            return Ok(None);
        }
        Event::Key(key) if key.kind != KeyEventKind::Release => key,
        _ => return Ok(None),
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(match key.code {
            KeyCode::Char('s') => Some(true),
            KeyCode::Char('c') => Some(false),
            _ => None,
        });
    }
    if key.code == KeyCode::Esc {
        return Ok(Some(false));
    }
    if key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return Ok(None);
    }
    let cursor = d.cursor();
    let text = d.text();
    if matches!(key.code, KeyCode::Up | KeyCode::Down) {
        let start = line_start(text, cursor);
        let column = *preferred.get_or_insert_with(|| text[start..cursor].chars().count());
        let target = if key.code == KeyCode::Up {
            if start == 0 {
                return Ok(None);
            }
            line_start(text, start - 1)
        } else {
            let end = line_end(text, cursor);
            if end == text.len() {
                return Ok(None);
            }
            end + 1
        };
        let end = line_end(text, target);
        let offset = text[target..end]
            .char_indices()
            .nth(column)
            .map_or(end, |(p, _)| target + p);
        d.set_cursor(offset);
        return Ok(None);
    }
    *preferred = None;
    match key.code {
        KeyCode::Left => d.set_cursor(
            text[..cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(p, _)| p),
        ),
        KeyCode::Right => d.set_cursor(
            text[cursor..]
                .chars()
                .next()
                .map_or(cursor, |c| cursor + c.len_utf8()),
        ),
        KeyCode::Home => d.set_cursor(line_start(text, cursor)),
        KeyCode::End => d.set_cursor(line_end(text, cursor)),
        KeyCode::Backspace if cursor > 0 => {
            let start = text[..cursor].char_indices().next_back().unwrap().0;
            replace(d, start..cursor, "")?;
        }
        KeyCode::Delete if cursor < text.len() => {
            let end = cursor + text[cursor..].chars().next().unwrap().len_utf8();
            replace(d, cursor..end, "")?;
        }
        KeyCode::Enter => replace(d, cursor..cursor, "\n")?,
        KeyCode::Tab => replace(d, cursor..cursor, "\t")?,
        KeyCode::Char(c) if !c.is_control() => {
            replace(d, cursor..cursor, c.encode_utf8(&mut [0; 4]))?
        }
        _ => {}
    }
    Ok(None)
}

// Printable ASCII occupies one column and should look like source, not a
// quoted string. Escape everything else to keep output safe and widths exact.
fn render_scalar(c: char) -> impl Iterator<Item = char> {
    let printable = (' '..='~').contains(&c);
    std::iter::once(c)
        .filter(move |_| printable)
        .chain(c.escape_default().filter(move |_| !printable))
}

const MAX_ROWS: u16 = 12;
const HELP: &str = "Ctrl-S: accept | Ctrl-C/Esc: cancel";

#[derive(Default)]
struct View {
    top: usize,
    left: usize,
    rows: u16,
    cursor_row: u16,
    size: Option<(u16, u16)>,
}
impl View {
    // All movement is relative to the reserved region, never screen coordinates.
    fn move_row(&mut self, output: &mut impl Write, row: u16) -> io::Result<()> {
        if row < self.cursor_row {
            queue!(output, MoveUp(self.cursor_row - row))?;
        } else if row > self.cursor_row {
            queue!(output, MoveDown(row - self.cursor_row))?;
        }
        self.cursor_row = row;
        queue!(output, MoveToColumn(0))
    }

    fn reserve(&mut self, output: &mut impl Write, width: u16, height: u16) -> io::Result<()> {
        if self.size == Some((width, height)) {
            return Ok(());
        }
        // A height reduction can clip the old region. Only touch its remaining
        // rows; never move above the visible terminal when finding its origin.
        let visible = height.max(1);
        if self.size.is_none() {
            queue!(output, Print("\r\n"))?;
        }
        self.cursor_row = self.cursor_row.min(visible - 1);
        self.move_row(output, 0)?;
        let old_rows = self.rows.min(visible);
        for row in 0..old_rows {
            self.move_row(output, row)?;
            queue!(output, Clear(ClearType::CurrentLine))?;
        }
        self.move_row(output, 0)?;
        self.rows = visible.min(MAX_ROWS);
        // CRLF reserves real rows even at the bottom of the terminal, allowing
        // normal scrollback scrolling rather than overwriting the parent's text.
        for _ in 1..self.rows {
            queue!(output, Print("\r\n"))?;
            self.cursor_row += 1;
        }
        self.move_row(output, 0)?;
        self.size = Some((width, height));
        Ok(())
    }

    fn finish(&mut self, output: &mut impl Write) -> io::Result<()> {
        if self.rows > 0 {
            self.move_row(output, self.rows - 1)?;
        }
        Ok(())
    }
    fn draw(
        &mut self,
        d: &impl DocumentEditor,
        output: &mut impl Write,
        width: u16,
        height: u16,
    ) -> io::Result<()> {
        self.reserve(output, width, height)?;
        let rows = usize::from(self.rows.saturating_sub(1));
        let capacity = usize::from(width.saturating_sub(1));
        let row = d.text()[..d.cursor()]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        let start = line_start(d.text(), d.cursor());
        let column = d.text()[start..d.cursor()]
            .chars()
            .flat_map(render_scalar)
            .count();
        if row < self.top {
            self.top = row;
        }
        if row >= self.top + rows.max(1) {
            self.top = row + 1 - rows.max(1);
        }
        if column < self.left {
            self.left = column;
        }
        if column >= self.left + capacity.max(1) {
            self.left = column + 1 - capacity.max(1);
        }
        let mut lines = d.text().split('\n').skip(self.top);
        for y in 0..self.rows {
            self.move_row(output, y)?;
            queue!(output, Clear(ClearType::CurrentLine))?;
            let visible: String = if usize::from(y) == rows {
                HELP.chars().take(capacity).collect()
            } else {
                lines
                    .next()
                    .unwrap_or("")
                    .chars()
                    .flat_map(render_scalar)
                    .skip(self.left)
                    .take(capacity)
                    .collect()
            };
            queue!(output, Print(visible))?;
        }
        if rows > 0 {
            self.move_row(output, (row - self.top) as u16)?;
            queue!(
                output,
                MoveToColumn(
                    column
                        .saturating_sub(self.left)
                        .min(capacity.saturating_sub(1)) as u16
                )
            )?;
        }
        output.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::apply_document_event as apply;
    use super::*;
    use crossterm::event::KeyEvent;
    #[derive(Default)]
    struct Model {
        text: String,
        cursor: usize,
        edits: usize,
        fail: bool,
    }
    impl DocumentEditor for Model {
        fn text(&self) -> &str {
            &self.text
        }
        fn cursor(&self) -> usize {
            self.cursor
        }
        fn set_cursor(&mut self, cursor: usize) {
            self.cursor = cursor;
        }
        fn replace(&mut self, range: Range<usize>, text: &str) -> io::Result<()> {
            if self.fail {
                return Err(io::Error::other("replacement failed"));
            }
            self.text.replace_range(range, text);
            self.edits += 1;
            Ok(())
        }
    }
    fn key(d: &mut Model, code: KeyCode, preferred: &mut Option<usize>) {
        apply(
            d,
            Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            preferred,
        )
        .unwrap();
    }
    #[test]
    fn unicode_lines_and_mutations() {
        let mut d = Model {
            text: "é🙂\nx\né🙂z".into(),
            ..Model::default()
        };
        let mut p = None;
        key(&mut d, KeyCode::Right, &mut p);
        assert_eq!(d.cursor, 2);
        key(&mut d, KeyCode::Right, &mut p);
        assert_eq!(d.cursor, 6);
        key(&mut d, KeyCode::Down, &mut p);
        assert_eq!(d.cursor, 8);
        key(&mut d, KeyCode::Down, &mut p);
        assert_eq!(d.cursor, 15);
        key(&mut d, KeyCode::Home, &mut p);
        assert_eq!(d.cursor, 9);
        key(&mut d, KeyCode::Backspace, &mut p);
        assert_eq!(d.text, "é🙂\nxé🙂z");
        key(&mut d, KeyCode::Enter, &mut p);
        key(&mut d, KeyCode::Tab, &mut p);
        key(&mut d, KeyCode::End, &mut p);
        key(&mut d, KeyCode::Left, &mut p);
        key(&mut d, KeyCode::Delete, &mut p);
        assert_eq!(d.text, "é🙂\nx\n\té🙂");
        assert_eq!(d.edits, 4);
    }
    #[test]
    fn paste_outcomes_and_errors() {
        let mut d = Model::default();
        let mut p = None;
        apply(&mut d, Event::Paste("a\r\n\t\x1b[31mé\x03".into()), &mut p).unwrap();
        assert_eq!(d.text, "a\n\té");
        assert_eq!(d.cursor, d.text.len());
        for (code, modifiers, expected) in [
            (KeyCode::Char('s'), KeyModifiers::CONTROL, true),
            (KeyCode::Char('c'), KeyModifiers::CONTROL, false),
            (KeyCode::Esc, KeyModifiers::NONE, false),
        ] {
            assert_eq!(
                apply(&mut d, Event::Key(KeyEvent::new(code, modifiers)), &mut p).unwrap(),
                Some(expected)
            );
        }
        d.fail = true;
        assert!(apply(&mut d, Event::Paste("x".into()), &mut p).is_err());
        d.cursor = 4;
        assert!(validate(&d).is_err());
    }
    #[test]
    fn printable_ascii_verbatim_and_controls_escaped() {
        let ascii: String = (' '..='~').collect();
        assert_eq!(
            ascii.chars().flat_map(render_scalar).collect::<String>(),
            ascii
        );
        let source = "say(\"it's C:\\src\");";
        let unsafe_text = "\x1b[31m\t\x07\r\u{7f}\u{9b}🙂";
        let escaped: String = unsafe_text.chars().flat_map(char::escape_default).collect();
        let mut d = Model {
            text: format!("{source}{unsafe_text}"),
            cursor: source.len(),
            ..Model::default()
        };
        let mut view = View::default();
        let mut out = Vec::new();
        view.draw(&d, &mut out, 200, 3).unwrap();
        let rendered = String::from_utf8(out).unwrap();
        assert!(rendered.contains(&format!("{source}{escaped}")));
        assert!(!rendered.contains(unsafe_text));
        assert!(rendered.is_ascii());
        assert!(rendered.ends_with(&format!("\x1b[{}G", source.len() + 1)));
        d.cursor = d.text.len();
        let mut out = Vec::new();
        view.draw(&d, &mut out, 200, 3).unwrap();
        assert!(
            String::from_utf8(out)
                .unwrap()
                .ends_with(&format!("\x1b[{}G", source.len() + escaped.len() + 1))
        );
    }

    #[test]
    fn inline_reservation_help_and_return_position() {
        let d = Model {
            text: "hello\nworld".into(),
            cursor: 8,
            ..Model::default()
        };
        let mut view = View::default();
        let mut out = Vec::new();
        view.draw(&d, &mut out, 80, 40).unwrap();
        let first = String::from_utf8(out.clone()).unwrap();
        assert_eq!(first.matches("\r\n").count(), 12);
        assert_eq!(first.matches("\x1b[2K").count(), 12);
        assert!(first.contains(HELP));
        assert!(first.contains("hello"));
        assert!(first.contains("world"));
        assert!(!first.contains("\x1b[2J"));
        assert!(!first.contains("1049"));
        assert!(!first.contains('H')); // No absolute row positioning.
        assert_eq!(view.rows, MAX_ROWS);
        assert_eq!(view.cursor_row, 1);
        out.clear();
        view.draw(&d, &mut out, 80, 40).unwrap();
        assert!(!out.windows(2).any(|s| s == b"\r\n"));
        assert_eq!(
            String::from_utf8(out).unwrap().matches("\x1b[2K").count(),
            12
        );
        let mut finish = Vec::new();
        view.finish(&mut finish).unwrap();
        assert_eq!(view.cursor_row, 11);
        assert_eq!(String::from_utf8(finish).unwrap(), "\x1b[10B\x1b[1G");
    }

    #[test]
    fn inline_scroll_resize_and_stale_rows() {
        let mut d = Model {
            text: (0..30).map(|i| format!("line{i}\n")).collect(),
            ..Model::default()
        };
        d.cursor = d.text.len();
        let mut view = View::default();
        for (width, height, expected_rows) in [(40, 25, 12), (20, 5, 5), (80, 30, 12)] {
            let mut out = Vec::new();
            view.draw(&d, &mut out, width, height).unwrap();
            assert_eq!(view.rows, expected_rows);
            assert!(view.cursor_row < expected_rows - 1);
            assert_eq!(view.top + usize::from(view.cursor_row), 30);
            let rendered = String::from_utf8(out).unwrap();
            assert!(!rendered.contains("\x1b[2J"));
            assert!(rendered.contains("Ctrl-S:"));
        }
        d.text.clear();
        d.cursor = 0;
        let mut out = Vec::new();
        view.draw(&d, &mut out, 80, 30).unwrap();
        let rendered = String::from_utf8(out).unwrap();
        assert_eq!(rendered.matches("\x1b[2K").count(), 12);
        assert!(!rendered.contains("line"));
        assert_eq!(view.top, 0);
        // On a one-row terminal help takes priority over document content.
        out = Vec::new();
        view.draw(&d, &mut out, 80, 1).unwrap();
        assert!(String::from_utf8(out).unwrap().contains(HELP));
        assert_eq!(view.cursor_row, 0);
    }

    #[test]
    fn viewport_bounded_safe_and_resize() {
        let d = Model {
            text: "\x1b[31m🙂\t\nlast".into(),
            cursor: 15,
            ..Model::default()
        };
        assert_eq!(d.cursor, d.text.len());
        let mut view = View::default();
        for (w, h) in [(8, 1), (2, 2), (0, 0), (80, 24)] {
            let mut out = Vec::new();
            view.draw(&d, &mut out, w, h).unwrap();
            assert!(out.is_ascii());
            assert!(!out.windows(5).any(|s| s == b"\x1b[31m"));
            assert!(out.len() < 1000);
        }
    }
}
