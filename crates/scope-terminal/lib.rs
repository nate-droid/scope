//! Unicode-scalar editing with byte cursors and single-line ASCII-only rendering.
//! Paste defaults to single-line text; normalized layout preservation is opt-in.
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
use std::ops::Range;

/// A caller-discovered completion. All offsets are half-open UTF-8 byte offsets
/// into the supplied input. Ranges must be ordered and on scalar boundaries.
/// Insertion text must be single-line, without controls or terminal sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCandidate {
    pub replacement: Range<usize>,
    pub insertion: String,
    /// Display-only text, optionally including a description supplied by the caller.
    /// Falls back to insertion when absent; never used for matching or replacement.
    /// Renderers must escape and bound this untrusted text before printing it.
    pub label: Option<String>,
}

/// Synchronous discovery only; language parsing, filtering, ordering, quoting,
/// and any catalog access belong to the caller. Called afresh on every Tab.
pub trait CompletionProvider {
    fn complete(&mut self, input: &str, cursor: usize) -> Vec<CompletionCandidate>;
}

impl<F> CompletionProvider for F
where
    F: FnMut(&str, usize) -> Vec<CompletionCandidate>,
{
    fn complete(&mut self, input: &str, cursor: usize) -> Vec<CompletionCandidate> {
        self(input, cursor)
    }
}

/// Result of one completion request. Display labels are raw data; renderers must
/// escape them. No candidates or selection are cached between requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionAction {
    None,
    Applied,
    Display(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Submitted(String),
    Cancelled,
    Eof,
}

/// Insertion policy for decoded paste payloads, independent of keyboard bindings.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PastePolicy {
    /// Legacy behavior: normalize/filter text, flatten newlines and remove tabs.
    #[default]
    SingleLine,
    /// Insert exactly the output of [`sanitize_paste`], preserving LF and tabs.
    /// This normalizes CR/CRLF and filters controls, not raw-byte preservation.
    /// It does not enable logical-line editing or change Enter/Tab bindings.
    NormalizedLayout,
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
    /// Inserts a decoded paste payload according to an explicit policy.
    /// Newlines and tabs are data, never key events. The cursor advances by the
    /// inserted UTF-8 byte length; existing text is left unchanged.
    pub fn insert_paste(&mut self, text: &str, policy: PastePolicy) {
        match policy {
            PastePolicy::SingleLine => self.insert(text),
            PastePolicy::NormalizedLayout => {
                let text = sanitize_paste(text);
                self.text.insert_str(self.cursor, &text);
                self.cursor += text.len();
            }
        }
    }
    /// Atomically replaces a valid UTF-8 byte range and places the cursor after
    /// the insertion. Invalid ranges or unsafe single-line text leave it intact.
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> bool {
        if range.start > range.end
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
            || sanitize(text) != text
        {
            return false;
        }
        self.text.replace_range(range.clone(), text);
        self.cursor = range.start + text.len();
        true
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

/// A recalled entry or the original, caller-owned draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistorySelection<D> {
    Entry(String),
    Draft(D),
}

/// Navigation over oldest-to-newest entries, independent of storage policy.
/// Entries must remain unchanged during navigation; reset before changing them.
#[derive(Debug, Clone)]
pub struct HistoryNavigation<D> {
    position: Option<usize>,
    draft: Option<D>,
}

impl<D> Default for HistoryNavigation<D> {
    fn default() -> Self {
        Self {
            position: None,
            draft: None,
        }
    }
}

impl<D> HistoryNavigation<D> {
    /// Saves the draft on first recall and stays put at the oldest entry.
    pub fn previous(&mut self, entries: &[String], current: D) -> Option<String> {
        if entries.is_empty() {
            return None;
        }
        let index = match self.position {
            None => {
                self.draft = Some(current);
                entries.len() - 1
            }
            Some(index) => index.saturating_sub(1),
        };
        self.position = Some(index);
        Some(entries[index].clone())
    }

    /// Moving past the newest entry returns the saved draft exactly once.
    pub fn next(&mut self, entries: &[String]) -> Option<HistorySelection<D>> {
        let index = self.position? + 1;
        if index < entries.len() {
            self.position = Some(index);
            Some(HistorySelection::Entry(entries[index].clone()))
        } else {
            self.position = None;
            self.draft.take().map(HistorySelection::Draft)
        }
    }

    pub fn reset(&mut self) {
        self.position = None;
        self.draft = None;
    }
}

/// Deterministic editor with optional history navigation. Down past the newest
/// entry restores the original draft, including its cursor. Recalled edits are
/// temporary and never mutate history. Submission records a history entry.
/// Submission, cancellation, and EOF reset the buffer and navigation for reuse.
#[derive(Debug, Default)]
pub struct Reader {
    pub buffer: LineBuffer,
    navigation: HistoryNavigation<LineBuffer>,
}
impl Reader {
    pub fn new() -> Self {
        Self::default()
    }
    /// Applies a unique candidate, or extends a common prefix when all valid
    /// candidates replace the same range ending at the cursor and extend the
    /// existing text. Otherwise returns labels in provider order for display.
    /// Invalid candidates are ignored. No implicit spaces are appended.
    pub fn complete(
        &mut self,
        provider: &mut (impl CompletionProvider + ?Sized),
    ) -> CompletionAction {
        let candidates: Vec<_> = provider
            .complete(self.buffer.text(), self.buffer.cursor())
            .into_iter()
            .filter(|c| {
                c.replacement.start <= c.replacement.end
                    && self.buffer.text.is_char_boundary(c.replacement.start)
                    && self.buffer.text.is_char_boundary(c.replacement.end)
                    && sanitize(&c.insertion) == c.insertion
            })
            .collect();
        let Some(first) = candidates.first() else {
            return CompletionAction::None;
        };
        if candidates.len() == 1 {
            self.buffer
                .replace(first.replacement.clone(), &first.insertion);
            return CompletionAction::Applied;
        }
        if candidates
            .iter()
            .all(|c| c.replacement == first.replacement)
            && first.replacement.end == self.buffer.cursor()
        {
            let current = &self.buffer.text()[first.replacement.clone()];
            if candidates.iter().all(|c| c.insertion.starts_with(current)) {
                let mut length = first.insertion.len();
                for candidate in &candidates[1..] {
                    length = first
                        .insertion
                        .chars()
                        .zip(candidate.insertion.chars())
                        .take_while(|(a, b)| a == b)
                        .map(|(c, _)| c.len_utf8())
                        .sum::<usize>()
                        .min(length);
                }
                if length > current.len() {
                    self.buffer
                        .replace(first.replacement.clone(), &first.insertion[..length]);
                    return CompletionAction::Applied;
                }
            }
        }
        CompletionAction::Display(
            candidates
                .into_iter()
                .map(|c| c.label.unwrap_or(c.insertion))
                .collect(),
        )
    }

    /// Inserts paste without submitting, completing, recording history, or
    /// changing navigation state. Submission/cancellation still use handle_key.
    pub fn handle_paste(&mut self, text: &str, policy: PastePolicy) -> Outcome {
        self.buffer.insert_paste(text, policy);
        Outcome::Continue
    }

    pub fn handle_key(&mut self, key: KeyEvent, history: &mut History) -> Outcome {
        if key.kind != KeyEventKind::Release && key.modifiers.is_empty() {
            match key.code {
                KeyCode::Up => {
                    if let Some(text) = self
                        .navigation
                        .previous(&history.entries, self.buffer.clone())
                    {
                        let cursor = text.len();
                        self.buffer = LineBuffer::from_parts(text, cursor);
                    }
                    return Outcome::Continue;
                }
                KeyCode::Down => {
                    match self.navigation.next(&history.entries) {
                        Some(HistorySelection::Entry(text)) => {
                            let cursor = text.len();
                            self.buffer = LineBuffer::from_parts(text, cursor);
                        }
                        Some(HistorySelection::Draft(draft)) => self.buffer = draft,
                        None => {}
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
            self.navigation.reset();
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

// Bound output during escaping, including for very large downstream descriptions.
fn completion_line(label: &str, width: u16) -> String {
    label
        .chars()
        .flat_map(char::escape_default)
        .take(usize::from(width.saturating_sub(1)))
        .collect()
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
    read_line_with_completion(prompt, history, &mut |_: &str, _: usize| Vec::new())
}

/// Like read_line, with synchronous, caller-owned Tab discovery. Redirected
/// input never calls the provider. Ambiguous labels print on separate lines,
/// escaped and clipped to terminal width; at most 100 are displayed per Tab.
pub fn read_line_with_completion(
    prompt: &str,
    history: &mut History,
    provider: &mut (impl CompletionProvider + ?Sized),
) -> io::Result<Outcome> {
    read_line_with_completion_and_paste_policy(prompt, history, provider, PastePolicy::default())
}

/// Like [`read_line`], with an explicit paste policy and Tab remaining a no-op.
/// Redirected input is unaffected by the policy and remains line-framed.
pub fn read_line_with_paste_policy(
    prompt: &str,
    history: &mut History,
    policy: PastePolicy,
) -> io::Result<Outcome> {
    read_line_with_completion_and_paste_policy(
        prompt,
        history,
        &mut |_: &str, _: usize| Vec::new(),
        policy,
    )
}

/// Like [`read_line_with_completion`], with an explicit policy for decoded paste
/// events only. Enter, keyboard Tab, history, cancellation, EOF and completion
/// validation are unchanged. Layout is displayed escaped in the same single-line
/// viewport, not a multiline editor. Redirected input ignores this policy.
pub fn read_line_with_completion_and_paste_policy(
    prompt: &str,
    history: &mut History,
    provider: &mut (impl CompletionProvider + ?Sized),
    policy: PastePolicy,
) -> io::Result<Outcome> {
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
            Event::Key(key)
                if key.code == KeyCode::Tab
                    && key.modifiers.is_empty()
                    && key.kind != KeyEventKind::Release =>
            {
                if let CompletionAction::Display(labels) = reader.complete(provider) {
                    let width = terminal::size()?.0;
                    queue!(out, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
                    for label in labels.iter().take(100) {
                        queue!(out, Print(completion_line(label, width)), Print("\r\n"))?;
                    }
                }
                Outcome::Continue
            }
            Event::Key(key) => reader.handle_key(key, history),
            Event::Paste(text) => reader.handle_paste(&text, policy),
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
