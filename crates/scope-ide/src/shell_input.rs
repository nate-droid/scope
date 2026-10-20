//! Terminal-only, bounded multiline shell editing. The caller prints `scope> ` first.
//!
//! Submission does not record history: the caller decides which commands to record.
//! Non-ASCII characters are edited as Unicode scalars and displayed as escapes,
//! avoiding ambiguous cell widths without adding a Unicode-width dependency.

use std::io::{self, IsTerminal, Write};

use crossterm::{
    cursor::{self, MoveTo},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    queue,
    style::Print,
    terminal::{self, Clear, ClearType, ScrollUp},
};

use crate::history::CommandHistory;

const PROMPT: &str = "scope> ";
const CONTINUATION: &str = "     > ";
const MAX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_VIEWPORT_ROWS: u16 = 8;

struct DisplayGuard;

impl DisplayGuard {
    fn enter(output: &mut impl Write) -> io::Result<Self> {
        let guard = Self;
        queue!(output, EnableBracketedPaste)?;
        output.flush()?;
        Ok(guard)
    }
}

impl Drop for DisplayGuard {
    fn drop(&mut self) {
        let mut output = io::stdout();
        let _ = queue!(output, DisableBracketedPaste);
        let _ = output.flush();
    }
}

struct RawGuard {
    restore: bool,
}

impl RawGuard {
    fn enter() -> io::Result<Self> {
        let restore = !terminal::is_raw_mode_enabled()?;
        if restore {
            terminal::enable_raw_mode()?;
        }
        Ok(Self { restore })
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        if self.restore {
            // Drop also runs on event, rendering, and flush errors (and unwinding).
            let _ = terminal::disable_raw_mode();
        }
    }
}

#[derive(Default)]
struct Input {
    line: String,
    // Always a UTF-8 byte boundary.
    cursor: usize,
    status: Option<&'static str>,
    choices: Vec<crate::language::CompletionItem>,
    selected: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Continue,
    Submit(String),
    Eof,
    Cancel,
}

impl Input {
    fn key_with_completion(
        &mut self,
        key: KeyEvent,
        history: &mut CommandHistory,
        inspect: &impl Fn(&str) -> crate::language::Completeness,
        complete: &impl Fn(&str, usize) -> Vec<crate::language::CompletionItem>,
    ) -> Action {
        if key.code == KeyCode::Tab
            && key.modifiers.is_empty()
            && key.kind != KeyEventKind::Release
            && self.choices.is_empty()
        {
            self.complete(complete(&self.line, self.cursor));
            Action::Continue
        } else {
            self.key_with(key, history, inspect)
        }
    }

    fn complete(&mut self, items: Vec<crate::language::CompletionItem>) {
        self.choices = items
            .into_iter()
            .filter(|item| {
                item.replacement.start <= item.replacement.end
                    && self.line.is_char_boundary(item.replacement.start)
                    && self.line.is_char_boundary(item.replacement.end)
            })
            .collect();
        self.selected = 0;
        if self.choices.len() == 1 {
            self.accept_completion();
        }
    }

    fn accept_completion(&mut self) {
        let item = self.choices.remove(self.selected);
        self.choices.clear();
        let size = self.line.len() - item.replacement.len();
        if item.insert_text.len() > MAX_INPUT_BYTES - size {
            self.status = Some("Input exceeds the 1 MiB limit");
            return;
        }
        self.cursor = item.replacement.start + item.insert_text.len();
        self.line.replace_range(item.replacement, &item.insert_text);
        self.status = None;
    }

    fn completion_key(&mut self, key: KeyEvent) -> bool {
        if self.choices.is_empty() {
            return false;
        }
        if key.kind == KeyEventKind::Release {
            return true;
        }
        match key.code {
            KeyCode::Tab | KeyCode::Down if key.modifiers.is_empty() => {
                self.selected = (self.selected + 1) % self.choices.len();
            }
            KeyCode::BackTab | KeyCode::Up if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.selected = (self.selected + self.choices.len() - 1) % self.choices.len();
            }
            KeyCode::Enter if key.modifiers.is_empty() => self.accept_completion(),
            KeyCode::Esc => self.choices.clear(),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.choices.clear()
            }
            _ => {
                self.choices.clear();
                return false;
            }
        }
        true
    }

    fn replace(&mut self, line: String) {
        if line.len() > MAX_INPUT_BYTES {
            self.status = Some("Input exceeds the 1 MiB limit");
            return;
        }
        self.choices.clear();
        self.cursor = line.len();
        self.line = line;
        self.status = None;
    }

    fn edit_buffer(&mut self, edit: impl FnOnce(&mut scope_terminal::LineBuffer)) {
        let mut buffer =
            scope_terminal::LineBuffer::from_parts(std::mem::take(&mut self.line), self.cursor);
        edit(&mut buffer);
        (self.line, self.cursor) = buffer.into_parts();
    }

    fn insert(&mut self, text: &str) {
        self.choices.clear();
        if text.len() > MAX_INPUT_BYTES - self.line.len() {
            self.status = Some("Input exceeds the 1 MiB limit");
            return;
        }
        self.line.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.status = None;
    }

    fn line_start(&self) -> usize {
        self.line[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        self.line[self.cursor..]
            .find('\n')
            .map_or(self.line.len(), |i| self.cursor + i)
    }

    fn vertical(&mut self, up: bool) -> bool {
        let start = self.line_start();
        let end = self.line_end();
        let column = self.line[start..self.cursor].chars().count();
        let (target_start, target_end) = if up && start > 0 {
            let end = start - 1;
            (self.line[..end].rfind('\n').map_or(0, |i| i + 1), end)
        } else if !up && end < self.line.len() {
            let start = end + 1;
            (
                start,
                self.line[start..]
                    .find('\n')
                    .map_or(self.line.len(), |i| start + i),
            )
        } else {
            return false;
        };
        self.cursor = self.line[target_start..target_end]
            .char_indices()
            .nth(column)
            .map_or(target_end, |(i, _)| target_start + i);
        true
    }

    fn submit(
        &mut self,
        history: &mut CommandHistory,
        force_invalid: bool,
        inspect: &impl Fn(&str) -> crate::language::Completeness,
    ) -> Action {
        use crate::language::Completeness;
        match inspect(&self.line) {
            Completeness::Incomplete => {
                if force_invalid {
                    self.status = Some("Incomplete source cannot be submitted");
                } else {
                    self.insert("\n");
                }
                Action::Continue
            }
            Completeness::Invalid { .. } if !force_invalid => {
                self.status = Some("Invalid source; edit it or press Ctrl+S to submit");
                Action::Continue
            }
            _ => {
                history.reset_navigation();
                self.cursor = 0;
                self.status = None;
                Action::Submit(std::mem::take(&mut self.line))
            }
        }
    }

    #[cfg(test)]
    fn key(&mut self, key: KeyEvent, history: &mut CommandHistory) -> Action {
        self.key_with(key, history, &|_| crate::language::Completeness::Complete)
    }

    fn key_with(
        &mut self,
        key: KeyEvent,
        history: &mut CommandHistory,
        inspect: &impl Fn(&str) -> crate::language::Completeness,
    ) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::Continue;
        }
        if self.completion_key(key) {
            return Action::Continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('p') => {
                    if let Some(line) = history.previous(&self.line) {
                        self.replace(line);
                    }
                }
                KeyCode::Char('n') => {
                    if let Some(line) = history.next() {
                        self.replace(line);
                    }
                }
                KeyCode::Char('j') => self.insert("\n"),
                KeyCode::Char('s') => return self.submit(history, true, inspect),
                KeyCode::Char('c') => {
                    self.replace(String::new());
                    history.reset_navigation();
                    return Action::Cancel;
                }
                KeyCode::Char('d') if self.line.is_empty() => {
                    history.reset_navigation();
                    return Action::Eof;
                }
                KeyCode::Char('d') => self.edit_buffer(scope_terminal::LineBuffer::delete),
                KeyCode::Char('a') => self.cursor = 0,
                KeyCode::Char('e') => self.cursor = self.line.len(),
                KeyCode::Char('k') => {
                    let end = self.line_end();
                    self.line.drain(self.cursor..end);
                    self.status = None;
                }
                KeyCode::Char('u') => {
                    self.line.drain(..self.cursor);
                    self.cursor = 0;
                }
                KeyCode::Char('w') => {
                    let end = self.cursor;
                    while self.cursor > 0
                        && self.line[..self.cursor]
                            .chars()
                            .next_back()
                            .unwrap()
                            .is_whitespace()
                    {
                        self.edit_buffer(scope_terminal::LineBuffer::left);
                    }
                    while self.cursor > 0
                        && !self.line[..self.cursor]
                            .chars()
                            .next_back()
                            .unwrap()
                            .is_whitespace()
                    {
                        self.edit_buffer(scope_terminal::LineBuffer::left);
                    }
                    self.line.drain(self.cursor..end);
                }
                _ => {}
            }
            return Action::Continue;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return Action::Continue;
        }
        match key.code {
            KeyCode::Enter => return self.submit(history, false, inspect),
            KeyCode::Left => self.edit_buffer(scope_terminal::LineBuffer::left),
            KeyCode::Right => self.edit_buffer(scope_terminal::LineBuffer::right),
            KeyCode::Home => self.cursor = self.line_start(),
            KeyCode::End => self.cursor = self.line_end(),
            KeyCode::Backspace => self.edit_buffer(scope_terminal::LineBuffer::backspace),
            KeyCode::Delete => self.edit_buffer(scope_terminal::LineBuffer::delete),
            KeyCode::Up => {
                if !self.vertical(true)
                    && let Some(line) = history.previous(&self.line)
                {
                    self.replace(line);
                }
            }
            KeyCode::Down => {
                if !self.vertical(false)
                    && let Some(line) = history.next()
                {
                    self.replace(line);
                }
            }
            KeyCode::Char(ch) if !ch.is_control() => {
                self.insert(ch.encode_utf8(&mut [0; 4]));
            }
            _ => {}
        }
        Action::Continue
    }

    #[cfg(test)]
    fn viewport(&self, columns: u16) -> (String, u16) {
        self.row_viewport(&self.line, self.cursor, PROMPT, columns)
    }

    fn row_viewport(
        &self,
        line: &str,
        position: usize,
        prompt: &str,
        columns: u16,
    ) -> (String, u16) {
        // ASCII-only rendering makes every byte exactly one cell. Reserve the
        // last terminal column to avoid autowrap, including on narrow terminals.
        let cells = usize::from(columns.saturating_sub(1));
        let prompt_len = prompt.len().min(cells.saturating_sub(1));
        let capacity = cells.saturating_sub(prompt_len);
        let mut displayed = String::new();
        let mut cursor = 0;
        for (index, ch) in line.char_indices() {
            let text = if ch.is_ascii() && !ch.is_control() {
                ch.to_string()
            } else {
                ch.escape_unicode().to_string()
            };
            if index < position {
                cursor += text.len();
            }
            displayed.push_str(&text);
        }
        let start = cursor.saturating_sub(capacity.saturating_sub(1));
        let end = (start + capacity).min(displayed.len());
        let mut row = prompt[..prompt_len].to_owned();
        row.push_str(&displayed[start..end]);
        (row, (prompt_len + cursor - start).min(cells) as u16)
    }

    #[cfg(test)]
    fn rows(&self, columns: u16, height: u16) -> (Vec<String>, u16, u16) {
        self.rows_with_prompt(columns, height, PROMPT)
    }

    fn rows_with_prompt(&self, columns: u16, height: u16, prompt: &str) -> (Vec<String>, u16, u16) {
        let active = self.line[..self.cursor]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let capacity = usize::from(height.saturating_sub(1).max(1));
        let first = active.saturating_sub(capacity - 1);
        let mut offset = 0;
        let mut rows = Vec::new();
        let mut cursor_column = 0;
        for (index, line) in self.line.split('\n').enumerate() {
            if index >= first + capacity {
                break;
            }
            if index >= first {
                let position = if index == active {
                    self.cursor - offset
                } else {
                    0
                };
                let (row, column) = self.row_viewport(
                    line,
                    position,
                    if index == 0 { prompt } else { CONTINUATION },
                    columns,
                );
                rows.push(row);
                if index == active {
                    cursor_column = column;
                }
            }
            offset += line.len() + 1;
        }
        (rows, cursor_column, (active - first) as u16)
    }
}

fn escaped(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii() && !ch.is_control() {
                ch.to_string()
            } else {
                ch.escape_unicode().to_string()
            }
        })
        .collect()
}

fn primary_prompt(session: &crate::language::InteractiveSession) -> String {
    session
        .language_id()
        .map_or_else(|| PROMPT.to_owned(), |id| format!("{}> ", escaped(id)))
}

fn inspect_buffer(
    session: &crate::language::InteractiveSession,
    buffer: &str,
) -> crate::language::Completeness {
    match session.route(buffer) {
        crate::language::InputRoute::ScopeCommand(_) => crate::language::Completeness::Complete,
        crate::language::InputRoute::Source(source) => session
            .completeness(source)
            .unwrap_or(crate::language::Completeness::Incomplete),
    }
}

struct InlineViewport {
    anchor: u16,
    cursor_row: u16,
    size: (u16, u16),
}

impl InlineViewport {
    fn new() -> io::Result<Self> {
        let (_, anchor) = cursor::position()?;
        Ok(Self {
            anchor,
            cursor_row: 0,
            size: terminal::size()?,
        })
    }

    fn resized_anchor(cursor_y: u16, relative_row: u16, height: u16) -> u16 {
        cursor_y
            .saturating_sub(relative_row)
            .min(height.saturating_sub(1))
    }

    fn reserve(&mut self, rows: u16) -> u16 {
        let overflow = self.anchor.saturating_add(rows).saturating_sub(self.size.1);
        self.anchor = self.anchor.saturating_sub(overflow);
        overflow
    }

    fn render(&mut self, input: &Input, prompt: &str, output: &mut impl Write) -> io::Result<()> {
        let size = terminal::size()?;
        if size != self.size {
            // Cursor reports account for terminal-side reflow/scrolling. Do not
            // reuse an absolute row from the old terminal geometry.
            output.flush()?;
            self.anchor = Self::resized_anchor(cursor::position()?.1, self.cursor_row, size.1);
            self.size = size;
        }
        self.render_at_size(input, prompt, output)
    }

    fn render_at_size(
        &mut self,
        input: &Input,
        prompt: &str,
        output: &mut impl Write,
    ) -> io::Result<()> {
        let (columns, height) = self.size;
        let bounded_height = height.clamp(1, MAX_VIEWPORT_ROWS);
        let (rows, column, row) = input.rows_with_prompt(columns, bounded_height, prompt);
        let completion = if input.choices.is_empty() {
            None
        } else {
            // One bounded inline row; navigation reveals every candidate.
            let item = &input.choices[input.selected];
            let others = input
                .choices
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != input.selected)
                .map(|(_, item)| item.label.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            Some(format!(
                "[{}/{}] >{}{} | {}",
                input.selected + 1,
                input.choices.len(),
                item.label,
                item.detail
                    .as_ref()
                    .map(|s| format!(" — {s}"))
                    .unwrap_or_default(),
                others
            ))
        };
        let status = completion.as_deref().or(input.status);
        let status_rows = u16::from(status.is_some() && bounded_height > 1);
        let scroll = self.reserve(rows.len() as u16 + status_rows);
        if scroll > 0 {
            queue!(output, ScrollUp(scroll))?;
        }
        // Everything below the anchor belongs to this editor. Never clear the
        // screen or the shell output above it.
        queue!(
            output,
            MoveTo(0, self.anchor),
            Clear(ClearType::FromCursorDown)
        )?;
        for (index, text) in rows.iter().enumerate() {
            queue!(output, MoveTo(0, self.anchor + index as u16), Print(text))?;
        }
        if status_rows > 0 {
            let status = escaped(status.unwrap());
            let end = status.len().min(usize::from(columns.saturating_sub(1)));
            queue!(
                output,
                MoveTo(0, self.anchor + rows.len() as u16),
                Print(&status[..end])
            )?;
        }
        self.cursor_row = row;
        queue!(output, MoveTo(column, self.anchor + row))?;
        output.flush()
    }

    fn finish(
        &mut self,
        line: Option<&str>,
        prompt: &str,
        output: &mut impl Write,
    ) -> io::Result<()> {
        queue!(
            output,
            MoveTo(0, self.anchor),
            Clear(ClearType::FromCursorDown)
        )?;
        if let Some(line) = line {
            // Echo the full buffer, not just its viewport, into normal scrollback.
            for (index, text) in line.split('\n').enumerate() {
                queue!(
                    output,
                    Print(if index == 0 { prompt } else { CONTINUATION }),
                    Print(escaped(text)),
                    Print("\r\n")
                )?;
            }
        } else {
            queue!(output, Print(prompt), Print("\r\n"))?;
        }
        output.flush()
    }
}

/// Reads from the terminal; `output` must target that same terminal's stdout.
/// Returns `None` on empty Ctrl-D; Ctrl-C discards the draft and reprompts.
/// Redirected stdin/stdout are rejected rather than entering raw mode.
pub(crate) fn read_line(
    history: &mut CommandHistory,
    output: &mut impl io::Write,
    session: &crate::language::InteractiveSession,
) -> io::Result<Option<String>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "shell input requires terminal stdin and stdout",
        ));
    }
    output.flush()?;
    let _raw = RawGuard::enter()?;
    let _display = DisplayGuard::enter(output)?;
    let inspect = |buffer: &str| inspect_buffer(session, buffer);
    let prompt = primary_prompt(session);
    let mut viewport = InlineViewport::new()?;
    history.reset_navigation();
    let mut input = Input::default();
    viewport.render(&input, &prompt, output)?;
    loop {
        match event::read()? {
            Event::Key(key) => {
                match input.key_with_completion(key, history, &inspect, &|source, cursor| {
                    session.complete(source, cursor)
                }) {
                    Action::Submit(line) => {
                        viewport.finish(Some(&line), &prompt, output)?;
                        return Ok(Some(line));
                    }
                    Action::Eof => {
                        viewport.finish(None, &prompt, output)?;
                        return Ok(None);
                    }
                    Action::Cancel => {
                        viewport.finish(Some("^C"), &prompt, output)?;
                        viewport = InlineViewport::new()?;
                        viewport.render(&input, &prompt, output)?;
                    }
                    Action::Continue => viewport.render(&input, &prompt, output)?,
                }
            }
            Event::Paste(text) => {
                input.insert(&scope_terminal::sanitize_paste(&text));
                viewport.render(&input, &prompt, output)?;
            }
            Event::Resize(_, _) => viewport.render(&input, &prompt, output)?,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(range: std::ops::Range<usize>, text: &str) -> crate::language::CompletionItem {
        crate::language::CompletionItem {
            label: text.into(),
            replacement: range,
            insert_text: text.into(),
            detail: None,
        }
    }

    #[test]
    fn completion_empty_single_unicode_multiline_and_invalid_ranges() {
        let mut input = Input::default();
        input.replace("α\n  hé suffix\nlast".into());
        input.cursor = 8; // after hé, measured in UTF-8 bytes
        let original = input.line.clone();
        input.complete(Vec::new());
        assert_eq!(input.line, original);
        input.complete(vec![
            candidate(1..2, "invalid"),
            candidate(8..999, "invalid"),
        ]);
        assert_eq!(input.line, original);
        input.complete(vec![candidate(5..8, "héllo")]);
        assert_eq!(input.line, "α\n  héllo suffix\nlast");
        assert_eq!(input.cursor, 11);
        assert!(input.choices.is_empty());
    }

    #[test]
    fn completion_selection_cancellation_and_history_do_not_submit() {
        let mut input = Input::default();
        let mut history = CommandHistory::new(10);
        history.record("old");
        input.replace("draft\n  x tail".into());
        input.cursor = 9;
        let original = input.line.clone();
        let inspect = |_: &str| -> crate::language::Completeness {
            panic!("completion inspected submission")
        };
        let complete = |source: &str, cursor: usize| {
            assert_eq!(source, original);
            assert_eq!(cursor, 9);
            vec![candidate(8..9, "one"), candidate(8..9, "two")]
        };
        assert_eq!(
            input.key_with_completion(
                KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                &mut history,
                &inspect,
                &complete
            ),
            Action::Continue
        );
        assert_eq!(input.line, original);
        assert_eq!(
            input.key_with(
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
                &mut history,
                &inspect
            ),
            Action::Continue
        );
        assert_eq!(input.selected, 1);
        assert_eq!(
            input.key_with(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                &mut history,
                &inspect
            ),
            Action::Continue
        );
        assert_eq!((input.line.as_str(), input.cursor), (original.as_str(), 9));
        input.complete(complete(&original, 9));
        input.key_with(
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
            &mut history,
            &inspect,
        );
        assert_eq!(input.selected, 1);
        input.key_with(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut history,
            &inspect,
        );
        assert_eq!(input.line, "draft\n  two tail");
        assert_eq!(input.cursor, 11);
        let draft = input.line.clone();
        key(&mut input, &mut history, KeyCode::Up);
        ctrl(&mut input, &mut history, 'p');
        ctrl(&mut input, &mut history, 'n');
        assert_eq!(input.line, draft);
        assert_eq!(
            key(&mut input, &mut history, KeyCode::Enter),
            Action::Submit(draft)
        );
    }

    #[test]
    fn command_completion_and_menu_rendering_are_bounded() {
        let mut input = Input::default();
        input.replace(":scope pro".into());
        input.complete(crate::cli::complete_commands(&input.line, input.cursor, true).unwrap());
        assert!(input.choices.len() > 1);
        let mut viewport = InlineViewport {
            anchor: 0,
            cursor_row: 0,
            size: (80, 8),
        };
        let mut output = Vec::new();
        viewport
            .render_at_size(&input, PROMPT, &mut output)
            .unwrap();
        assert!(String::from_utf8(output).unwrap().contains("proposal"));
        input.replace("no-match".into());
        input.complete(crate::cli::complete_commands(&input.line, input.cursor, false).unwrap());
        assert_eq!(input.line, "no-match");
        assert!(input.choices.is_empty());
    }

    fn key(input: &mut Input, history: &mut CommandHistory, code: KeyCode) -> Action {
        input.key(KeyEvent::new(code, KeyModifiers::NONE), history)
    }

    fn ctrl(input: &mut Input, history: &mut CommandHistory, ch: char) -> Action {
        input.key(
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL),
            history,
        )
    }

    struct RoutingProvider;
    struct RoutingSession;
    static ROUTING_PROVIDER: RoutingProvider = RoutingProvider;
    static INSPECTED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    impl crate::language::InteractiveSessionProvider for RoutingProvider {
        fn language_id(&self) -> &'static str {
            "mock.shell"
        }
        fn create_session(
            &self,
        ) -> Result<
            Box<dyn crate::language::InteractiveLanguageSession>,
            crate::language::SessionError,
        > {
            Ok(Box::new(RoutingSession))
        }
    }

    impl crate::language::InteractiveLanguageSession for RoutingSession {
        fn language_id(&self) -> &str {
            "mock.shell"
        }
        fn completeness(&self, source: &str) -> crate::language::Completeness {
            INSPECTED.lock().unwrap().push(source.to_owned());
            match source {
                "open" => crate::language::Completeness::Incomplete,
                "bad\nsource" => crate::language::Completeness::Invalid {
                    diagnostics: vec![],
                },
                _ => crate::language::Completeness::Complete,
            }
        }
        fn evaluate(
            &mut self,
            _: &str,
        ) -> Result<crate::language::EvaluationResult, crate::language::SessionError> {
            panic!("editing must never evaluate source")
        }
        fn reset(&mut self) -> Result<(), crate::language::SessionError> {
            Ok(())
        }
    }

    #[test]
    fn real_session_routes_whole_buffers_without_evaluation() {
        use crate::language::{Completeness, InteractiveSession, LanguageRegistry};
        let mut session = InteractiveSession::new();
        assert_eq!(primary_prompt(&session), PROMPT);
        assert_eq!(inspect_buffer(&session, "open"), Completeness::Complete);
        let registry = LanguageRegistry::empty().with_interactive_provider(&ROUTING_PROVIDER);
        session.select(&registry, "mock.shell").unwrap();
        assert_eq!(primary_prompt(&session), "mock.shell> ");
        INSPECTED.lock().unwrap().clear();
        let mut input = Input::default();
        let mut history = CommandHistory::default();
        let inspect = |text: &str| inspect_buffer(&session, text);
        input.insert(":scope quit\nopen");
        assert_eq!(
            input.submit(&mut history, false, &inspect),
            Action::Submit(":scope quit\nopen".into())
        );
        assert!(INSPECTED.lock().unwrap().is_empty());
        input.insert("open");
        assert_eq!(
            input.submit(&mut history, false, &inspect),
            Action::Continue
        );
        assert_eq!(input.line, "open\n");
        input.replace("bad\nsource".into());
        assert_eq!(
            input.submit(&mut history, false, &inspect),
            Action::Continue
        );
        assert_eq!(input.line, "bad\nsource");
        assert_eq!(
            input.submit(&mut history, true, &inspect),
            Action::Submit("bad\nsource".into())
        );
        input.insert("done\nsource extra");
        input.cursor = "done\nsource".len();
        assert_eq!(ctrl(&mut input, &mut history, 'k'), Action::Continue);
        assert!(matches!(
            session.route(&input.line),
            crate::language::InputRoute::Source("done\nsource")
        ));
        assert_eq!(
            input.submit(&mut history, false, &inspect),
            Action::Submit("done\nsource".into())
        );
        assert_eq!(
            *INSPECTED.lock().unwrap(),
            ["open", "bad\nsource", "bad\nsource", "done\nsource"]
        );
    }

    #[test]
    fn explicit_history_keys_restore_multiline_draft() {
        let mut history = CommandHistory::default();
        history.record("one\nλ");
        history.record("two\n界");
        let mut input = Input::default();
        input.insert("draft\ntext");
        input.cursor = 2;
        ctrl(&mut input, &mut history, 'p');
        assert_eq!(input.line, "two\n界");
        key(&mut input, &mut history, KeyCode::Home);
        ctrl(&mut input, &mut history, 'p');
        assert_eq!(input.line, "one\nλ");
        ctrl(&mut input, &mut history, 'n');
        assert_eq!(input.line, "two\n界");
        input.cursor = 0;
        ctrl(&mut input, &mut history, 'n');
        assert_eq!(input.line, "draft\ntext");
        assert_eq!(history.entries(), &["one\nλ", "two\n界"]);
    }

    #[test]
    fn inline_viewport_scrolls_bounds_and_echoes_complete_submission() {
        let mut input = Input::default();
        input.insert("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nλ\x1b");
        input.status = Some("Invalid source");
        for height in 1..20 {
            let mut viewport = InlineViewport {
                anchor: height - 1,
                cursor_row: 0,
                size: (30, height),
            };
            let mut output = Vec::new();
            viewport
                .render_at_size(&input, "mock.shell> ", &mut output)
                .unwrap();
            assert!(viewport.anchor + viewport.cursor_row < height);
            let text = String::from_utf8(output).unwrap();
            assert!(!text.contains("\x1b[2J"));
            assert!(!text.contains("?1049"));
            assert!(text.contains("Invalid source") || height == 1);
            assert!(viewport.cursor_row < MAX_VIEWPORT_ROWS);
            assert!(InlineViewport::resized_anchor(height - 1, 7, height) < height);
        }
        let mut viewport = InlineViewport {
            anchor: 10,
            cursor_row: 0,
            size: (80, 24),
        };
        let mut output = Vec::new();
        viewport
            .render_at_size(&input, "mock.shell> ", &mut output)
            .unwrap();
        assert_eq!(viewport.anchor, 10);
        output.clear();
        viewport
            .finish(Some(&input.line), "mock.shell> ", &mut output)
            .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("mock.shell> one\r\n"));
        assert!(text.contains("     > two\r\n"));
        assert!(text.contains("\\u{3bb}\\u{1b}\r\n"));
        assert_eq!(text.matches("\r\n").count(), 10);
        assert_eq!(InlineViewport::resized_anchor(3, 7, 4), 0);
        assert_eq!(InlineViewport::resized_anchor(100, 0, 4), 3);
    }

    #[test]
    fn completeness_controls_entire_buffer_submission() {
        use crate::language::Completeness;
        let mut input = Input::default();
        let mut history = CommandHistory::default();
        input.insert("first\nsecond");
        let incomplete = |text: &str| {
            assert!(text.starts_with("first\nsecond"));
            Completeness::Incomplete
        };
        assert_eq!(
            input.submit(&mut history, false, &incomplete),
            Action::Continue
        );
        assert_eq!(input.line, "first\nsecond\n");
        let before = input.line.clone();
        assert_eq!(
            input.submit(&mut history, true, &incomplete),
            Action::Continue
        );
        assert_eq!(input.line, before);
        let invalid = |_: &str| Completeness::Invalid {
            diagnostics: vec![],
        };
        assert_eq!(
            input.submit(&mut history, false, &invalid),
            Action::Continue
        );
        assert_eq!(input.line, before);
        assert!(input.status.is_some());
        assert_eq!(
            input.submit(&mut history, true, &invalid),
            Action::Submit(before)
        );
        input.insert("complete\nsource");
        assert_eq!(
            input.submit(&mut history, false, &|_| Completeness::Complete),
            Action::Submit("complete\nsource".into())
        );
    }

    #[test]
    fn multiline_navigation_and_atomic_paste_limits() {
        let mut history = CommandHistory::default();
        history.record("saved\nλ界");
        let mut input = Input::default();
        input.insert("draft");
        key(&mut input, &mut history, KeyCode::Up);
        key(&mut input, &mut history, KeyCode::Home);
        assert_eq!(input.cursor, 6);
        key(&mut input, &mut history, KeyCode::Up);
        assert_eq!(input.cursor, 0);
        key(&mut input, &mut history, KeyCode::Down);
        assert_eq!(input.cursor, 6);
        key(&mut input, &mut history, KeyCode::Down);
        assert_eq!(input.line, "draft");
        ctrl(&mut input, &mut history, 'j');
        input.insert("λ\n:scope quit\n");
        assert_eq!(input.line, "draft\nλ\n:scope quit\n");
        let before = input.line.clone();
        let cursor = input.cursor;
        input.insert(&"x".repeat(MAX_INPUT_BYTES));
        assert_eq!(input.line, before);
        assert_eq!(input.cursor, cursor);
        assert!(input.status.is_some());
        input.replace("x".repeat(MAX_INPUT_BYTES - 1));
        input.insert("λ");
        assert_eq!(input.line.len(), MAX_INPUT_BYTES - 1);
        input.insert("x");
        assert_eq!(input.line.len(), MAX_INPUT_BYTES);
        input.insert("\n");
        assert_eq!(input.line.len(), MAX_INPUT_BYTES);
    }

    #[test]
    fn multiline_viewport_is_bounded_across_resizes() {
        let mut input = Input::default();
        input.replace("one\nλ界\nthree\nfour\nfive".into());
        for height in 1..8 {
            for columns in 1..25 {
                for cursor in input
                    .line
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain([input.line.len()])
                    .collect::<Vec<_>>()
                {
                    input.cursor = cursor;
                    let (rows, column, row) = input.rows(columns, height);
                    assert!(rows.len() <= usize::from(height));
                    assert!(row < height);
                    assert!(column < columns);
                    for text in rows {
                        assert!(text.len() < usize::from(columns));
                        assert!(text.is_ascii());
                        assert!(!text.chars().any(char::is_control));
                    }
                }
            }
        }
        let (rows, _, _) = input.rows(80, 10);
        assert_eq!(rows[0], "scope> one");
        assert!(rows[1].starts_with(CONTINUATION));
    }

    #[test]
    fn navigation_restores_draft_without_changing_history() {
        let mut history = CommandHistory::new(10);
        history.record("one");
        history.record("two");
        let mut input = Input::default();
        input.replace("draft λ".into());
        key(&mut input, &mut history, KeyCode::Up);
        key(&mut input, &mut history, KeyCode::Char('!'));
        assert_eq!(input.line, "two!");
        key(&mut input, &mut history, KeyCode::Up);
        key(&mut input, &mut history, KeyCode::Up);
        assert_eq!(input.line, "one");
        key(&mut input, &mut history, KeyCode::Down);
        assert_eq!(input.line, "two");
        key(&mut input, &mut history, KeyCode::Down);
        key(&mut input, &mut history, KeyCode::Down);
        assert_eq!(input.line, "draft λ");
        assert_eq!(input.cursor, input.line.len());
        assert_eq!(history.entries(), &["one", "two"]);
    }

    #[test]
    fn editing_uses_character_boundaries() {
        let mut history = CommandHistory::default();
        let mut input = Input::default();
        for ch in "aλ界".chars() {
            key(&mut input, &mut history, KeyCode::Char(ch));
        }
        key(&mut input, &mut history, KeyCode::Left);
        key(&mut input, &mut history, KeyCode::Backspace);
        key(&mut input, &mut history, KeyCode::Char('é'));
        assert_eq!(input.line, "aé界");
        key(&mut input, &mut history, KeyCode::Delete);
        key(&mut input, &mut history, KeyCode::Home);
        key(&mut input, &mut history, KeyCode::Right);
        key(&mut input, &mut history, KeyCode::Delete);
        assert_eq!(input.line, "a");
        key(&mut input, &mut history, KeyCode::End);
        ctrl(&mut input, &mut history, 'u');
        assert_eq!(input.line, "");
        input.replace("one two  ".into());
        ctrl(&mut input, &mut history, 'w');
        assert_eq!(input.line, "one ");
        ctrl(&mut input, &mut history, 'a');
        assert_eq!(input.cursor, 0);
        ctrl(&mut input, &mut history, 'e');
        assert_eq!(input.cursor, 4);
    }

    #[test]
    fn kill_line_preserves_cursor_newlines_and_unicode_boundaries() {
        let mut history = CommandHistory::default();
        let mut input = Input::default();
        for (text, cursor, expected) in [
            ("", 0, ""),
            ("abc", 0, ""),
            ("abc", 1, "a"),
            ("abc", 3, "abc"),
            ("first\naλ界\nlast", 9, "first\naλ\nlast"),
            ("first\nlast", 5, "first\nlast"),
            ("first\nlast", 6, "first\n"),
        ] {
            input.replace(text.into());
            input.cursor = cursor;
            input.status = Some("Invalid source; edit it or press Ctrl+S to submit");
            assert_eq!(ctrl(&mut input, &mut history, 'k'), Action::Continue);
            assert_eq!(input.line, expected);
            assert_eq!(input.cursor, cursor);
            assert_eq!(input.status, None);
        }
    }

    #[test]
    fn kill_line_edits_recalled_draft_without_changing_history_and_recovers() {
        let mut history = CommandHistory::default();
        history.record("help extra");
        let mut input = Input::default();
        input.insert("draft\ntext");
        ctrl(&mut input, &mut history, 'p');
        input.cursor = 4;
        ctrl(&mut input, &mut history, 'k');
        assert_eq!(input.line, "help");
        ctrl(&mut input, &mut history, 'n');
        assert_eq!(input.line, "draft\ntext");
        assert_eq!(history.entries(), &["help extra"]);
        assert_eq!(ctrl(&mut input, &mut history, 'c'), Action::Cancel);
        input.insert(":scope help extra");
        input.cursor = 11;
        ctrl(&mut input, &mut history, 'k');
        let session = crate::language::InteractiveSession::new();
        assert_eq!(
            input.key_with(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &mut history,
                &|text| inspect_buffer(&session, text),
            ),
            Action::Submit(":scope help".into())
        );
    }

    #[test]
    fn resubmit_eof_and_cancel() {
        let mut history = CommandHistory::default();
        history.record("saved");
        let mut input = Input::default();
        key(&mut input, &mut history, KeyCode::Up);
        assert_eq!(
            key(&mut input, &mut history, KeyCode::Enter),
            Action::Submit("saved".into())
        );
        assert_eq!(history.next(), None);
        assert_eq!(history.entries(), &["saved"]);
        assert_eq!(ctrl(&mut input, &mut history, 'd'), Action::Eof);
        input.replace("draft".into());
        assert_eq!(ctrl(&mut input, &mut history, 'd'), Action::Continue);
        key(&mut input, &mut history, KeyCode::Up);
        assert_eq!(ctrl(&mut input, &mut history, 'c'), Action::Cancel);
        assert!(input.line.is_empty());
        assert_eq!(history.next(), None);
        key(&mut input, &mut history, KeyCode::Char('x'));
        assert_eq!(
            key(&mut input, &mut history, KeyCode::Enter),
            Action::Submit("x".into())
        );
    }

    #[test]
    fn viewport_never_reaches_wrap_column_or_emits_controls() {
        let mut input = Input::default();
        input.replace("abc界\n\x1bdef".into());
        for columns in 1..40 {
            for cursor in input
                .line
                .char_indices()
                .map(|(index, _)| index)
                .chain([input.line.len()])
                .collect::<Vec<_>>()
            {
                input.cursor = cursor;
                let (row, column) = input.viewport(columns);
                assert!(row.is_ascii());
                assert!(!row.chars().any(char::is_control));
                assert!(row.len() < usize::from(columns));
                assert!(column < columns);
            }
        }
    }
}
