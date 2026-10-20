//! Single-line Unicode-scalar editing with byte cursors and ASCII-only rendering.
//! Interactive submissions are added to history; buffered input is returned verbatim
//! except for a terminating LF and its optional preceding CR.

use crossterm::{
    cursor::MoveToColumn,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::io::{self, BufRead, IsTerminal, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Submitted(String),
    Cancelled,
    Eof,
}

/// Edits Unicode scalars, not grapheme clusters. Cursor offsets are UTF-8 bytes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LineBuffer {
    text: String,
    cursor: usize,
}

impl LineBuffer {
    /// Restores exact text without sanitization. Panics unless cursor is a valid
    /// UTF-8 boundary (including the end). Rendering still escapes unsafe text.
    pub fn from_parts(text: String, cursor: usize) -> Self {
        assert!(
            text.is_char_boundary(cursor),
            "cursor must be a UTF-8 boundary"
        );
        Self { text, cursor }
    }
    pub fn into_parts(self) -> (String, usize) {
        (self.text, self.cursor)
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    /// Strips terminal sequences and controls, replaces newlines with spaces,
    /// and removes tabs. Use sanitize_paste for multiline text with tabs intact.
    pub fn insert(&mut self, text: &str) {
        let text = sanitize(text);
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }
    pub fn left(&mut self) {
        if let Some((i, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = i;
        }
    }
    pub fn right(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.drain(self.cursor..end);
    }
    pub fn delete(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.text.drain(self.cursor..self.cursor + c.len_utf8());
        }
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
    /// Release events and unrecognized modifier combinations are ignored.
    /// Ctrl-D deletes at the cursor, or signals EOF only when the line is empty.
    /// Ctrl-C and Escape clear the buffer before returning Cancelled.
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        if key.kind == KeyEventKind::Release {
            return Outcome::Continue;
        }
        if key.modifiers == KeyModifiers::CONTROL {
            match key.code {
                KeyCode::Char('a' | 'A') => self.home(),
                KeyCode::Char('e' | 'E') => self.end(),
                KeyCode::Char('c' | 'C') => {
                    self.clear();
                    return Outcome::Cancelled;
                }
                KeyCode::Char('d' | 'D') if self.text.is_empty() => return Outcome::Eof,
                KeyCode::Char('d' | 'D') => self.delete(),
                _ => {}
            }
            return Outcome::Continue;
        }
        if !(key.modifiers - KeyModifiers::SHIFT).is_empty() {
            return Outcome::Continue;
        }
        match key.code {
            KeyCode::Enter => return Outcome::Submitted(self.text.clone()),
            KeyCode::Esc => {
                self.clear();
                return Outcome::Cancelled;
            }
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Home => self.home(),
            KeyCode::End => self.end(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Char(c) => self.insert(c.encode_utf8(&mut [0; 4])),
            _ => {}
        }
        Outcome::Continue
    }
}

fn sanitize(text: &str) -> String {
    sanitize_paste(text)
        .chars()
        .filter(|&c| c != '\t')
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect()
}

/// Sanitizes pasted text for multiline editors, preserving newlines and tabs.
/// CRLF and lone CR normalize to LF. All other control characters and terminal
/// sequences (CSI, OSC, DCS, SOS, PM, APC, and single-character ESC commands)
/// are removed. Unterminated sequences consume the remainder of the input.
pub fn sanitize_paste(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let sequence = if c == '\u{1b}' {
            chars.next()
        } else {
            match c {
                '\u{9b}' => Some('['),
                '\u{9d}' => Some(']'),
                '\u{90}' => Some('P'),
                '\u{98}' => Some('X'),
                '\u{9e}' => Some('^'),
                '\u{9f}' => Some('_'),
                _ => None,
            }
        };
        if let Some(kind) = sequence {
            match kind {
                '[' => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                ']' | 'P' | 'X' | '^' | '_' => {
                    while let Some(c) = chars.next() {
                        if c == '\u{9c}' || (kind == ']' && c == '\u{7}') {
                            break;
                        }
                        if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                '\u{20}'..='\u{2f}' => {
                    for c in chars.by_ref() {
                        if ('\u{30}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Unbounded in-memory history. Navigation belongs to Reader, allowing reuse.
#[derive(Debug, Default, Clone)]
pub struct History {
    entries: Vec<String>,
}
impl History {
    pub fn new() -> Self {
        Self::default()
    }
    /// Empty entries and consecutive duplicates are ignored.
    pub fn push(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !text.is_empty() && self.entries.last() != Some(&text) {
            self.entries.push(text);
        }
    }
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

/// Deterministic editor with optional history navigation. Down past the newest
/// entry restores the original draft, including its cursor. Recalled edits are
/// temporary and never mutate history. Submission records a history entry.
/// Submission, cancellation, and EOF reset the buffer and navigation for reuse.
#[derive(Debug, Default)]
pub struct Reader {
    pub buffer: LineBuffer,
    position: Option<usize>,
    draft: Option<LineBuffer>,
}
impl Reader {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> Outcome {
        if key.kind != KeyEventKind::Release && key.modifiers.is_empty() {
            match key.code {
                KeyCode::Up if !history.entries.is_empty() => {
                    let i = match self.position {
                        None => {
                            self.draft = Some(self.buffer.clone());
                            history.entries.len() - 1
                        }
                        Some(i) => i.saturating_sub(1),
                    };
                    self.position = Some(i);
                    let text = history.entries[i].clone();
                    self.buffer = LineBuffer::from_parts(text.clone(), text.len());
                    return Outcome::Continue;
                }
                KeyCode::Down => {
                    if let Some(i) = self.position {
                        if i + 1 < history.entries.len() {
                            self.position = Some(i + 1);
                            let text = history.entries[i + 1].clone();
                            self.buffer = LineBuffer::from_parts(text.clone(), text.len());
                        } else {
                            self.position = None;
                            self.buffer = self.draft.take().unwrap_or_default();
                        }
                    }
                    return Outcome::Continue;
                }
                _ => {}
            }
        }
        let outcome = self.buffer.handle_key(key);
        if let Outcome::Submitted(text) = &outcome {
            history.push(text.clone());
        }
        if outcome != Outcome::Continue {
            self.buffer.clear();
            self.position = None;
            self.draft = None;
        }
        outcome
    }
}

/// Buffered fallback: no prompt, editing, sanitization or history side effects.
/// EOF with a partial final line submits that line; only zero bytes signals EOF.
pub fn read_from(input: &mut impl BufRead) -> io::Result<Outcome> {
    let mut text = String::new();
    if input.read_line(&mut text)? == 0 {
        return Ok(Outcome::Eof);
    }
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    Ok(Outcome::Submitted(text))
}

struct TerminalGuard {
    restore_raw: bool,
}
impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        let restore_raw = !terminal::is_raw_mode_enabled()?;
        if restore_raw {
            terminal::enable_raw_mode()?;
        }
        let guard = Self { restore_raw };
        let mut out = io::stdout();
        queue!(out, EnableBracketedPaste)?;
        out.flush()?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = queue!(out, DisableBracketedPaste);
        let _ = out.flush();
        if self.restore_raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}

fn escaped(text: &str) -> String {
    text.chars().flat_map(char::escape_default).collect()
}

// Reserve the last column to avoid terminal autowrap. Escapes may be clipped:
// every rendered character is ASCII, so byte offsets equal terminal columns.
fn viewport(prompt: &str, buffer: &LineBuffer, width: u16) -> (String, u16) {
    let capacity = usize::from(width.saturating_sub(1));
    if capacity == 0 {
        return (String::new(), 0);
    }
    let prompt = escaped(prompt);
    let prefix = &prompt[..prompt.len().min(capacity / 2)];
    let text = escaped(buffer.text());
    let cursor = escaped(&buffer.text()[..buffer.cursor()]).len();
    let available = capacity - prefix.len();
    let start = cursor.saturating_sub(available - 1);
    let end = (start + available).min(text.len());
    (
        format!("{prefix}{}", &text[start..end]),
        (prefix.len() + cursor - start) as u16,
    )
}

/// Uses interactive editing only when BOTH stdin and stdout are terminals.
/// Otherwise delegates directly to read_from without printing a prompt.
/// Errors propagate after RAII cleanup; cancellation and EOF are distinct.
pub fn read_line(prompt: &str, history: &mut History) -> io::Result<Outcome> {
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return read_from(&mut io::stdin().lock());
    }
    let _guard = TerminalGuard::enter()?;
    let mut reader = Reader::new();
    let mut out = io::stdout();
    loop {
        let (line, column) = viewport(prompt, &reader.buffer, terminal::size()?.0);
        queue!(
            out,
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            Print(line),
            MoveToColumn(column)
        )?;
        out.flush()?;
        let outcome = match event::read()? {
            Event::Key(key) => reader.handle_key(key, history),
            Event::Paste(text) => {
                reader.buffer.insert(&text);
                Outcome::Continue
            }
            _ => Outcome::Continue,
        };
        if outcome != Outcome::Continue {
            queue!(out, Print("\r\n"))?;
            out.flush()?;
            return Ok(outcome);
        }
    }
}

#[cfg(test)]
mod tests;
