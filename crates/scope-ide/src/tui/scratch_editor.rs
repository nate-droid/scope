//! Explicit, in-memory source editing authority. No filesystem or execution access.
use std::{cell::Cell, io, ops::Range};

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as RenderSpan},
    widgets::Paragraph,
};
use scope_terminal::DocumentEditor;
use unicode_segmentation::UnicodeSegmentation;

use super::{Pane, TuiContext};
use crate::{
    scratch::{BoundSource, ScratchBuffers},
    text::{Span, TextBuffer, TextEdit},
};

// Source byte offsets stay intact; terminal control characters must never reach
// the cell backend. Tabs use source-relative stops, not terminal HT movement.
fn display_graphemes(line: &str) -> Vec<(usize, String, usize)> {
    let mut column = 0;
    line.grapheme_indices(true)
        .map(|(byte, grapheme)| {
            let display = if grapheme == "\t" {
                " ".repeat(4 - column % 4)
            } else if grapheme.chars().any(char::is_control) {
                "�".to_owned()
            } else {
                grapheme.to_owned()
            };
            let width = Line::raw(display.as_str()).width();
            column += width;
            (byte, display, width)
        })
        .collect()
}

// Retain the viewport until the cursor enters its context margin. Do not clamp
// to document length: blank rows below EOF provide the same breathing room.
fn contextual_top(top: usize, row: usize, height: usize) -> usize {
    let margin = 3.min(height.saturating_sub(1) / 2);
    if row < top.saturating_add(margin) {
        row.saturating_sub(margin)
    } else if row >= top.saturating_add(height.saturating_sub(margin)) {
        row.saturating_sub(height.saturating_sub(margin + 1))
    } else {
        top
    }
}

/// Find a lexical pair at the cursor, preferring the delimiter under it over
/// the one immediately before it. Offsets are UTF-8 bytes, like DocumentEditor.
/// Language services expose no string/comment ranges, so all delimiters count.
fn matching_delimiters(text: &str, cursor: usize) -> Option<[usize; 2]> {
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let bytes = text.as_bytes();
    let is_delimiter = |byte: u8| b"()[]{}".contains(&byte);
    let at = if bytes.get(cursor).is_some_and(|b| is_delimiter(*b)) {
        cursor
    } else {
        let previous = cursor.checked_sub(1)?;
        is_delimiter(*bytes.get(previous)?).then_some(previous)?
    };
    let opening = |b| matches!(b, b'(' | b'[' | b'{');
    let counterpart = |b| match b {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b')' => b'(',
        b']' => b'[',
        b'}' => b'{',
        _ => unreachable!(),
    };
    let forward = opening(bytes[at]);
    let mut stack = vec![bytes[at]];
    let mut index = at;
    loop {
        index = if forward {
            index.checked_add(1)?
        } else {
            index.checked_sub(1)?
        };
        let byte = *bytes.get(index)?;
        if !is_delimiter(byte) {
            continue;
        }
        if opening(byte) == forward {
            stack.push(byte);
        } else {
            if counterpart(stack.pop()?) != byte {
                return None;
            }
            if stack.is_empty() {
                return Some([at, index]);
            }
        }
    }
}

/// An owned optimistic-concurrency baseline, including owner-independent identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchEditSnapshot {
    /// Legacy numeric owner key: scratch ID or App DocumentId. Compare the full
    /// source identity as well; numeric keys from different owners may overlap.
    pub scratch_id: u64,
    pub source: scope_language::SourceSnapshot,
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScratchEditError {
    Unbound,
    Closed,
    Rebound,
    Stale,
    Edit(String),
}
impl std::fmt::Display for ScratchEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unbound => "no editor source target bound",
            Self::Closed => "editor source target closed",
            Self::Rebound => "editor source target rebound",
            Self::Stale => "conflict: editor source changed",
            Self::Edit(message) => message,
        })
    }
}
impl std::error::Error for ScratchEditError {}

/// Short-lived capability supplied only to explicitly authorized panes.
/// It can snapshot/publish only its borrowed source, never select, bind, save,
/// evaluate, or access other application state.
pub struct ScratchEditAccess<'a> {
    sources: Option<&'a mut ScratchBuffers>,
    app_document: Option<&'a mut crate::document::Document>,
    selected: bool,
}
impl<'a> ScratchEditAccess<'a> {
    #[cfg(test)]
    pub(super) fn new(sources: &'a mut ScratchBuffers) -> Self {
        Self {
            sources: Some(sources),
            app_document: None,
            selected: false,
        }
    }

    /// Edits the selected scratch independently of the semantic binding.
    pub fn selected(sources: &'a mut ScratchBuffers) -> Self {
        Self {
            sources: Some(sources),
            app_document: None,
            selected: true,
        }
    }

    pub(super) fn app(document: &'a mut crate::document::Document) -> Self {
        Self {
            sources: None,
            app_document: Some(document),
            selected: true,
        }
    }

    pub(super) fn document_snapshot(document: &crate::document::Document) -> ScratchEditSnapshot {
        ScratchEditSnapshot {
            scratch_id: document.id().as_u64(),
            source: scope_language::SourceSnapshot {
                source_id: document.evaluation_source_id().clone(),
                revision_id: scope_language::RevisionId::from_version(document.version()),
                text: document.text().into(),
            },
            version: document.version(),
        }
    }

    fn target_id(&self) -> Option<u64> {
        if let Some(document) = self.app_document.as_ref() {
            return Some(document.id().as_u64());
        }
        let sources = self.sources.as_ref()?;
        if self.selected {
            sources.selected().ok().map(|buffer| buffer.id())
        } else {
            sources.semantic_target_id()
        }
    }

    pub fn snapshot(&self) -> Result<ScratchEditSnapshot, ScratchEditError> {
        if let Some(document) = self.app_document.as_ref() {
            return Ok(Self::document_snapshot(document));
        }
        let id = self.target_id().ok_or(ScratchEditError::Unbound)?;
        let buffer = self
            .sources
            .as_ref()
            .ok_or(ScratchEditError::Unbound)?
            .get(id)
            .map_err(|_| ScratchEditError::Closed)?;
        let source = scope_language::SourceSnapshot {
            source_id: buffer.evaluation_source_id().clone(),
            revision_id: scope_language::RevisionId::from_version(buffer.version()),
            text: buffer.text().into(),
        };
        Ok(ScratchEditSnapshot {
            scratch_id: id,
            source,
            version: buffer.version(),
        })
    }

    /// Atomically checks the binding and exact baseline before replacing any text.
    pub fn commit(
        &mut self,
        expected: &ScratchEditSnapshot,
        text: &str,
    ) -> Result<ScratchEditSnapshot, ScratchEditError> {
        if self.target_id() != Some(expected.scratch_id) {
            return Err(ScratchEditError::Rebound);
        }
        let current = self.snapshot()?;
        if current.source.source_id != expected.source.source_id {
            return Err(ScratchEditError::Rebound);
        }
        if current != *expected {
            return Err(ScratchEditError::Stale);
        }
        if text != current.source.text.as_ref() {
            let range = TextBuffer::new(current.source.text.as_ref())
                .range_for_span(Span::new(0, current.source.text.len()))
                .map_err(|error| ScratchEditError::Edit(error.to_string()))?;
            let edit = TextEdit::replace(range, text);
            if let Some(document) = self.app_document.as_mut() {
                document
                    .apply_edit(edit)
                    .map_err(|error| ScratchEditError::Edit(error.to_string()))?;
            } else {
                self.sources
                    .as_mut()
                    .ok_or(ScratchEditError::Unbound)?
                    .get_mut(expected.scratch_id)
                    .map_err(|_| ScratchEditError::Closed)?
                    .apply_edit(edit)
                    .map_err(|error| ScratchEditError::Edit(error.to_string()))?;
            }
        }
        self.snapshot()
    }
}

struct Draft {
    edit_identity: u64,
    app_document: bool,
    baseline: ScratchEditSnapshot,
    text: TextBuffer,
    cursor: usize,
    preferred: Option<usize>,
    editing: LocalEditing,
    viewport_top: Cell<usize>,
}

#[derive(Clone)]
struct LocalState {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
}

#[derive(Default)]
struct LocalEditing {
    anchor: Option<usize>,
    undo: Vec<LocalState>,
    redo: Vec<LocalState>,
    typing: bool,
}

impl Draft {
    fn state(&self) -> LocalState {
        LocalState {
            text: self.text.as_str().into(),
            cursor: self.cursor,
            anchor: self.editing.anchor,
        }
    }

    fn restore(&mut self, state: LocalState) {
        self.text = TextBuffer::new(state.text);
        self.cursor = state.cursor;
        self.editing.anchor = state.anchor;
        self.preferred = None;
    }

    fn selection(&self) -> Option<Range<usize>> {
        self.editing
            .anchor
            .filter(|anchor| *anchor != self.cursor)
            .map(|anchor| anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    fn event(&mut self, event: Event) -> io::Result<Option<bool>> {
        if matches!(&event, Event::Key(key) if key.kind == crossterm::event::KeyEventKind::Release)
        {
            return Ok(None);
        }
        let key = match &event {
            Event::Key(key) => Some(*key),
            _ => None,
        };
        if let Some(key) = key
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('z' | 'y'))
        {
            self.editing.typing = false;
            let redo = key.code == KeyCode::Char('y');
            let state = if redo {
                self.editing.redo.pop()
            } else {
                self.editing.undo.pop()
            };
            if let Some(state) = state {
                let current = self.state();
                if redo {
                    self.editing.undo.push(current);
                } else {
                    self.editing.redo.push(current);
                }
                self.restore(state);
            }
            return Ok(None);
        }
        let plain = key.is_some_and(|key| {
            !key.modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        });
        let navigation = plain
            && key.is_some_and(|key| {
                matches!(
                    key.code,
                    KeyCode::Left
                        | KeyCode::Right
                        | KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::Home
                        | KeyCode::End
                )
            });
        let typing =
            plain && key.is_some_and(|key| matches!(key.code, KeyCode::Char(c) if !c.is_control()));
        let mutation = matches!(&event, Event::Paste(_))
            || typing
            || (plain
                && key.is_some_and(|key| {
                    matches!(
                        key.code,
                        KeyCode::Enter
                            | KeyCode::Tab
                            | KeyCode::BackTab
                            | KeyCode::Backspace
                            | KeyCode::Delete
                    )
                }));
        let before = self.state();
        let had_selection = self.selection().is_some();
        let indent_selection = plain
            && had_selection
            && key.is_some_and(|key| matches!(key.code, KeyCode::Tab | KeyCode::BackTab));
        if navigation {
            if key.unwrap().modifiers.contains(KeyModifiers::SHIFT) {
                self.editing.anchor.get_or_insert(self.cursor);
            } else {
                self.editing.anchor = None;
                if let Some(range) = before
                    .anchor
                    .filter(|anchor| *anchor != before.cursor)
                    .map(|anchor| anchor.min(before.cursor)..anchor.max(before.cursor))
                    && matches!(key.unwrap().code, KeyCode::Left | KeyCode::Right)
                {
                    self.cursor = if key.unwrap().code == KeyCode::Left {
                        range.start
                    } else {
                        range.end
                    };
                    self.preferred = None;
                    self.editing.typing = false;
                    return Ok(None);
                }
            }
        }
        let deleting = plain
            && key.is_some_and(|key| matches!(key.code, KeyCode::Backspace | KeyCode::Delete));
        if mutation && !indent_selection {
            if let Some(range) = self.selection() {
                self.replace(range.clone(), "")?;
                self.cursor = range.start;
            }
            self.editing.anchor = None;
        }
        let mut preferred = self.preferred.take();
        let result = if indent_selection {
            let selection = self.selection().unwrap();
            let source = self.text.as_str();
            let start = source[..selection.start].rfind('\n').map_or(0, |p| p + 1);
            let dedent = key.is_some_and(|key| {
                key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT)
            });
            let mut replacement = String::new();
            let mut cursor = self.cursor;
            let mut anchor = self.editing.anchor.unwrap();
            let mut end = start;
            // A selection ending at column zero does not include that next line.
            for line in source[start..].split_inclusive('\n') {
                if end >= selection.end {
                    break;
                }
                let spaces = line.bytes().take_while(|b| *b == b' ').count();
                let removed = if dedent && spaces > 0 {
                    (spaces - 1) % 4 + 1
                } else {
                    0
                };
                if dedent {
                    replacement.push_str(&line[removed..]);
                } else {
                    replacement.push_str("    ");
                    replacement.push_str(line);
                }
                // Map both endpoints in original coordinates, retaining direction.
                for (original, mapped) in [
                    (before.cursor, &mut cursor),
                    (before.anchor.unwrap(), &mut anchor),
                ] {
                    if original >= end {
                        if dedent {
                            *mapped -= (original - end).min(removed);
                        } else {
                            *mapped += 4;
                        }
                    }
                }
                end += line.len();
            }
            if replacement != source[start..end] {
                self.replace(start..end, &replacement)?;
                self.cursor = cursor;
                self.editing.anchor = Some(anchor);
            }
            preferred = None;
            Ok(None)
        } else if deleting && had_selection {
            Ok(None)
        } else {
            scope_terminal::apply_document_event(self, event, &mut preferred)
        };
        self.preferred = preferred;
        if self.text.as_str() != before.text {
            if !(typing && self.editing.typing && !had_selection) {
                self.editing.undo.push(before);
                // History is disposable and bounded, not committed revision history.
                if self.editing.undo.len() > 256 {
                    self.editing.undo.remove(0);
                }
            }
            self.editing.redo.clear();
        }
        self.editing.typing = typing && !had_selection;
        result
    }
}

impl DocumentEditor for Draft {
    fn text(&self) -> &str {
        self.text.as_str()
    }
    fn cursor(&self) -> usize {
        self.cursor
    }
    fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
    }
    fn replace(&mut self, range: Range<usize>, text: &str) -> io::Result<()> {
        let range = self
            .text
            .range_for_span(Span::new(range.start, range.end))
            .map_err(io::Error::other)?;
        self.text
            .apply_edit(TextEdit::replace(range, text))
            .map_err(io::Error::other)
    }
}

/// A local draft survives redraws, layout changes and temporary shell handoff.
#[derive(Default)]
pub struct EditableScratchPane {
    draft: Option<Draft>,
    parked: Vec<Draft>,
    cancelled: Vec<(scope_language::SourceId, u64)>,
    status: Option<String>,
}
impl EditableScratchPane {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_source_event(
        &mut self,
        event: Event,
        access: &mut ScratchEditAccess<'_>,
    ) -> bool {
        match &event {
            Event::Paste(_) => {}
            Event::Key(key) if key.kind != KeyEventKind::Release => {}
            _ => return false,
        }
        if access.selected {
            let current = match access.snapshot() {
                Ok(current) => current,
                Err(error) => {
                    self.status = Some(error.to_string());
                    return true;
                }
            };
            if self
                .draft
                .as_ref()
                .is_some_and(|draft| draft.baseline.source.source_id != current.source.source_id)
            {
                self.parked.push(self.draft.take().unwrap());
                self.status = None;
            }
            if self.draft.is_none()
                && let Some(index) = self
                    .parked
                    .iter()
                    .position(|draft| draft.baseline.source.source_id == current.source.source_id)
            {
                self.draft = Some(self.parked.remove(index));
            }
        }
        if self.draft.is_none() {
            match access.snapshot() {
                Ok(baseline) => {
                    self.draft = Some(Draft {
                        edit_identity: self
                            .cancelled
                            .iter()
                            .find(|(source, _)| source == &baseline.source.source_id)
                            .map_or(0, |(_, identity)| *identity),
                        app_document: access.app_document.is_some(),
                        text: TextBuffer::new(baseline.source.text.as_ref()),
                        baseline,
                        cursor: 0,
                        preferred: None,
                        editing: LocalEditing::default(),
                        viewport_top: Cell::new(0),
                    })
                }
                Err(error) => {
                    self.status = Some(error.to_string());
                    return true;
                }
            }
        }
        let draft = self.draft.as_mut().unwrap();
        // Clean working text can follow a new committed revision. Dirty text must
        // retain its exact baseline so publication still detects conflicts.
        if draft.text.as_str() == draft.baseline.source.text.as_ref()
            && let Ok(current) = access.snapshot()
            && current.source.source_id == draft.baseline.source.source_id
            && current != draft.baseline
        {
            draft.cursor = draft.cursor.min(current.source.text.len());
            while !current.source.text.is_char_boundary(draft.cursor) {
                draft.cursor -= 1;
            }
            draft.text = TextBuffer::new(current.source.text.as_ref());
            draft.baseline = current;
            draft.preferred = None;
            draft.editing = LocalEditing::default();
            draft.cursor = draft
                .text
                .as_str()
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain(std::iter::once(draft.text.as_str().len()))
                .take_while(|i| *i <= draft.cursor)
                .last()
                .unwrap_or(0);
            self.status = None;
        }
        let before = draft.text.as_str().to_owned();
        let result = draft.event(event);
        if before != draft.text.as_str() {
            static NEXT_EDIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            draft.edit_identity = NEXT_EDIT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        match result {
            Ok(Some(false)) => {
                static NEXT_CANCEL: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(u64::MAX);
                let identity = NEXT_CANCEL.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                self.cancelled
                    .retain(|(source, _)| source != &draft.baseline.source.source_id);
                self.cancelled
                    .push((draft.baseline.source.source_id.clone(), identity));
                self.draft = None;
                self.status = Some("draft cancelled".into());
            }
            Ok(Some(true)) => match access.commit(&draft.baseline, draft.text.as_str()) {
                Ok(baseline) => {
                    draft.baseline = baseline;
                    self.status = Some("Published in memory".into());
                }
                Err(error) => {
                    self.status = Some(format!("{error}; draft retained — Ctrl-C cancels"))
                }
            },
            Ok(None) => {
                if draft.text.as_str() != draft.baseline.source.text.as_ref() {
                    self.status = None;
                }
            }
            Err(error) => self.status = Some(error.to_string()),
        }
        true
    }
}
impl Pane for EditableScratchPane {
    fn editor_draft(
        &self,
        baseline: &super::ScratchEditSnapshot,
    ) -> Option<super::EditorDraftSnapshot> {
        let draft = self
            .draft
            .iter()
            .chain(self.parked.iter())
            .find(|draft| draft.baseline.source.source_id == baseline.source.source_id);
        let modified =
            draft.is_some_and(|draft| draft.text.as_str() != draft.baseline.source.text.as_ref());
        if modified && draft.is_some_and(|draft| &draft.baseline != baseline) {
            return None;
        }
        Some(super::EditorDraftSnapshot {
            baseline: baseline.clone(),
            text: if modified {
                draft.unwrap().text.as_str().to_owned()
            } else {
                baseline.source.text.to_string()
            },
            edit_identity: draft.map_or_else(
                || {
                    self.cancelled
                        .iter()
                        .find(|(source, _)| source == &baseline.source.source_id)
                        .map_or(0, |(_, identity)| *identity)
                },
                |draft| draft.edit_identity,
            ),
            editor: String::new(),
            editor_generation: 0,
            selection_identity: 0,
            source_selection_identity: 0,
            owner_binding_generation: 0,
        })
    }

    fn has_dirty_draft(&self) -> bool {
        self.draft
            .iter()
            .chain(self.parked.iter())
            .any(|draft| draft.text.as_str() != draft.baseline.source.text.as_ref())
    }

    fn has_dirty_app_draft(&self) -> bool {
        self.draft.iter().chain(self.parked.iter()).any(|draft| {
            draft.app_document && draft.text.as_str() != draft.baseline.source.text.as_ref()
        })
    }

    fn dirty_draft_sources(&self) -> Vec<(u64, String)> {
        self.draft
            .iter()
            .chain(self.parked.iter())
            .filter(|draft| {
                !draft.app_document && draft.text.as_str() != draft.baseline.source.text.as_ref()
            })
            .map(|draft| {
                (
                    draft.baseline.scratch_id,
                    format!(
                        "{:?} revision {:?}",
                        draft.baseline.source.source_id, draft.baseline.source.revision_id
                    ),
                )
            })
            .collect()
    }

    fn id(&self) -> &'static str {
        "scratch-editor"
    }
    fn handle_source_event(&mut self, event: Event, access: &mut ScratchEditAccess<'_>) -> bool {
        EditableScratchPane::handle_source_event(self, event, access)
    }
    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        let selected = ctx.editor_source();
        let draft = selected.as_ref().and_then(|selected| {
            self.draft
                .iter()
                .chain(self.parked.iter())
                .find(|draft| draft.baseline.source.source_id == selected.source.source_id)
                .filter(|draft| {
                    draft.text.as_str() != draft.baseline.source.text.as_ref()
                        || draft.baseline.source == selected.source
                })
        });
        let bound = selected.map(|snapshot| BoundSource::Available(snapshot.source));
        let conflict = match (draft, &bound) {
            (Some(draft), Some(BoundSource::Available(current)))
                if current != &draft.baseline.source =>
            {
                Some(if current.source_id != draft.baseline.source.source_id {
                    "REBOUND — draft retained"
                } else {
                    "CONFLICT — draft retained"
                })
            }
            (Some(_), Some(BoundSource::Unavailable { .. })) => Some("CLOSED — draft retained"),
            (Some(_), None | Some(BoundSource::Unbound)) => Some("UNBOUND — draft retained"),
            _ => None,
        };
        let text = match (draft, &bound) {
            (Some(draft), _) => draft.text.as_str(),
            (_, Some(BoundSource::Available(source))) => source.text.as_ref(),
            (_, Some(BoundSource::Unavailable { .. })) => {
                "UNAVAILABLE — bound scratch target is closed"
            }
            (_, Some(BoundSource::Unbound)) => "UNBOUND — no editor source target bound",
            (_, None) => "UNAVAILABLE — no scratch owner supplied",
        };
        let status = conflict.or(self.status.as_deref()).or_else(|| {
            draft
                .is_some_and(|draft| draft.text.as_str() != draft.baseline.source.text.as_ref())
                .then_some("Local draft")
        });
        let inner = if let Some(status) = status {
            let [source_area, status_area] =
                Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
            frame.render_widget(
                Paragraph::new(status).style(Style::default().fg(if conflict.is_some() {
                    Color::Yellow
                } else {
                    Color::DarkGray
                })),
                status_area,
            );
            source_area
        } else {
            area
        };
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let cursor = draft.map(|draft| {
            let prefix = &text[..draft.cursor];
            (
                prefix.bytes().filter(|byte| *byte == b'\n').count(),
                display_graphemes(prefix.rsplit('\n').next().unwrap_or(""))
                    .iter()
                    .map(|(_, _, width)| width)
                    .sum::<usize>(),
            )
        });
        let (top, left) = cursor
            .map(|(row, column)| {
                (
                    draft.map_or(0, |draft| {
                        let top = contextual_top(
                            draft.viewport_top.get(),
                            row,
                            usize::from(inner.height),
                        );
                        draft.viewport_top.set(top);
                        top
                    }),
                    column.saturating_sub(usize::from(inner.width) - 1),
                )
            })
            .unwrap_or((usize::from(ctx.scroll()), 0));
        let matching = draft.and_then(|draft| matching_delimiters(text, draft.cursor));
        // Slice before rendering rather than narrowing large source offsets to u16.
        let lines: Vec<Line<'_>> = text
            .split('\n')
            .skip(top)
            .take(usize::from(inner.height))
            .map(|line| {
                let mut column = 0;
                let line_offset = line.as_ptr() as usize - text.as_ptr() as usize;
                let selection = draft.and_then(Draft::selection);
                let mut spans = Vec::new();
                spans.extend(display_graphemes(line).into_iter().filter_map(
                    |(offset, grapheme, width)| {
                        let start = column;
                        column += width;
                        if column <= left {
                            return None;
                        }
                        // Never render half a wide glyph at either viewport edge.
                        let grapheme = if start < left {
                            " ".repeat(column - left)
                        } else if column > left + usize::from(inner.width) {
                            " ".repeat((left + usize::from(inner.width)).saturating_sub(start))
                        } else {
                            grapheme
                        };
                        let byte = line_offset + offset;
                        if selection
                            .as_ref()
                            .is_some_and(|range| range.contains(&byte))
                        {
                            Some(RenderSpan::styled(
                                grapheme,
                                Style::default().add_modifier(Modifier::REVERSED),
                            ))
                        } else if matching.is_some_and(|pair| pair.contains(&byte)) {
                            Some(RenderSpan::styled(
                                grapheme,
                                Style::default()
                                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                            ))
                        } else {
                            Some(RenderSpan::raw(grapheme))
                        }
                    },
                ));
                Line::from(spans)
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
        if ctx.is_focused()
            && let Some((row, column)) = cursor
        {
            frame.set_cursor_position((
                inner.x + (column - left) as u16,
                inner.y + (row - top) as u16,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedent_clears_collapsed_selection_anchor() {
        let mut sources = ScratchBuffers::default();
        let id = sources.create("test", "    x");
        sources.bind_semantic_target(Some(id)).unwrap();
        let mut pane = EditableScratchPane::new();
        for (code, modifiers) in [
            (KeyCode::End, KeyModifiers::NONE),
            (KeyCode::Left, KeyModifiers::SHIFT),
            (KeyCode::Right, KeyModifiers::SHIFT),
            (KeyCode::BackTab, KeyModifiers::SHIFT),
            (KeyCode::Char('y'), KeyModifiers::NONE),
        ] {
            local_key(&mut pane, &mut sources, code, modifiers);
        }
        let draft = pane.draft.as_ref().unwrap();
        assert_eq!(draft.text.as_str(), "xy");
        assert_eq!(draft.cursor, 2);
        assert_eq!(draft.editing.anchor, None);
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('z'),
            KeyModifiers::CONTROL,
        );
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('z'),
            KeyModifiers::CONTROL,
        );
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "    x");
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        );
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        );
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "xy");
        assert_eq!(pane.draft.as_ref().unwrap().editing.anchor, None);
    }

    #[test]
    fn display_cells_expand_tabs_and_never_emit_controls() {
        let source = "猫\te\u{301}\t\r\u{1b}";
        let displayed = display_graphemes(source);
        assert_eq!(
            displayed
                .iter()
                .map(|(_, s, _)| s.as_str())
                .collect::<String>(),
            "猫  e\u{301}   ��"
        );
        assert_eq!(
            displayed.iter().map(|(_, _, width)| width).sum::<usize>(),
            10
        );
        for (offset, _, _) in displayed {
            assert!(source.is_char_boundary(offset));
        }
    }

    fn local_key(
        pane: &mut EditableScratchPane,
        sources: &mut ScratchBuffers,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) {
        pane.handle_source_event(
            Event::Key(crossterm::event::KeyEvent::new(code, modifiers)),
            &mut ScratchEditAccess::new(sources),
        );
    }

    #[test]
    fn local_history_selection_and_publication_conflict() {
        let mut sources = sources();
        let mut pane = EditableScratchPane::new();
        let none = KeyModifiers::NONE;
        let ctrl = KeyModifiers::CONTROL;
        let shift = KeyModifiers::SHIFT;
        for c in ['a', 'b'] {
            local_key(&mut pane, &mut sources, KeyCode::Char(c), none);
        }
        local_key(&mut pane, &mut sources, KeyCode::Char('z'), ctrl);
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "é\n");
        local_key(&mut pane, &mut sources, KeyCode::Char('y'), ctrl);
        assert_eq!(pane.draft.as_ref().unwrap().cursor, 2);
        local_key(&mut pane, &mut sources, KeyCode::End, none);
        local_key(&mut pane, &mut sources, KeyCode::Down, shift);
        local_key(&mut pane, &mut sources, KeyCode::Home, shift);
        assert!(pane.draft.as_ref().unwrap().selection().is_some());
        pane.handle_source_event(
            Event::Paste("X\n👩‍💻".into()),
            &mut ScratchEditAccess::new(&mut sources),
        );
        let replaced = pane.draft.as_ref().unwrap().text.as_str().to_owned();
        local_key(&mut pane, &mut sources, KeyCode::Char('z'), ctrl);
        assert!(pane.draft.as_ref().unwrap().selection().is_some());
        local_key(&mut pane, &mut sources, KeyCode::Delete, none);
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "abé");
        local_key(&mut pane, &mut sources, KeyCode::Char('y'), ctrl);
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "abé");
        local_key(&mut pane, &mut sources, KeyCode::Char('z'), ctrl);
        local_key(&mut pane, &mut sources, KeyCode::Char('y'), ctrl);
        local_key(&mut pane, &mut sources, KeyCode::Char('s'), ctrl);
        let published = sources.bound_source();
        local_key(&mut pane, &mut sources, KeyCode::Char('z'), ctrl);
        assert_eq!(sources.bound_source(), published);
        let baseline = ScratchEditAccess::new(&mut sources).snapshot().unwrap();
        ScratchEditAccess::new(&mut sources)
            .commit(&baseline, "external")
            .unwrap();
        local_key(&mut pane, &mut sources, KeyCode::Char('s'), ctrl);
        assert!(pane.status.as_ref().unwrap().contains("conflict"));
        assert_ne!(pane.draft.as_ref().unwrap().text.as_str(), "external");
        assert_ne!(replaced, "external");
    }

    #[test]
    fn clean_external_refresh_drops_obsolete_local_history() {
        let mut sources = sources();
        let mut pane = EditableScratchPane::new();
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        );
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('s'),
            KeyModifiers::CONTROL,
        );
        let baseline = ScratchEditAccess::new(&mut sources).snapshot().unwrap();
        ScratchEditAccess::new(&mut sources)
            .commit(&baseline, "new 👩‍💻")
            .unwrap();
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('z'),
            KeyModifiers::CONTROL,
        );
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "new 👩‍💻");
        assert!(pane.draft.as_ref().unwrap().editing.undo.is_empty());
        assert!(!pane.has_dirty_draft());
    }
    #[test]
    fn lexical_pairs_cover_nesting_adjacency_unicode_and_unmatched_input() {
        let text = "猫({[é]}) ";
        for (cursor, pair) in [
            (3, [3, 10]),
            (4, [4, 9]),
            (5, [5, 8]),
            (8, [8, 5]),
            (9, [9, 4]),
            (10, [10, 3]),
            (11, [10, 3]),
        ] {
            assert_eq!(matching_delimiters(text, cursor), Some(pair));
        }
        for (text, cursor) in [
            ("([)]", 0),
            ("([)]", 3),
            ("(", 1),
            ("]", 0),
            ("abc", 1),
            ("() x", 4),
            ("猫()", 1),
            ("()", 99),
        ] {
            assert_eq!(
                matching_delimiters(text, cursor),
                None,
                "{text:?} at {cursor}"
            );
        }
        assert_eq!(matching_delimiters("(\n[]\n)", 0), Some([0, 5]));
        assert_eq!(matching_delimiters("()", 2), Some([1, 0]));
    }

    #[test]
    fn editor_uses_source_area_without_chrome_and_preserves_status() {
        let mut sources = sources();
        let mut pane = EditableScratchPane::new();
        let root = crate::test_support::TempWorkspace::new("editor_presentation");
        let app = crate::app::App::new(root.workspace()).unwrap();
        let render = |pane: &EditableScratchPane, sources: &ScratchBuffers, app_editor| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 6)).unwrap();
            terminal
                .draw(|frame| {
                    pane.render(
                        frame,
                        frame.area(),
                        &TuiContext::with_view_sources(&app, true, 0, Some(sources))
                            .with_app_editor(app_editor),
                    )
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let initial = render(&pane, &sources, false);
        assert_eq!(initial[(0, 0)].symbol(), "é");
        let screen = |buffer: &ratatui::buffer::Buffer| {
            buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };
        let displayed = screen(&initial);
        for absent in ["Editor", "FOCUSED", "Ctrl-S", "┌", "│", "└"] {
            assert!(!displayed.contains(absent));
        }
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        );
        assert!(screen(&render(&pane, &sources, false)).contains("Local draft"));
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('s'),
            KeyModifiers::CONTROL,
        );
        let published = screen(&render(&pane, &sources, false));
        assert!(published.contains("Published in memory"));
        assert!(!published.contains("not saved"));
        pane.status = Some("write denied; draft retained".into());
        // App mode must not hide errors, even when its document is unavailable.
        assert!(screen(&render(&pane, &sources, true)).contains("write denied; draft retained"));
        local_key(
            &mut pane,
            &mut sources,
            KeyCode::Char('y'),
            KeyModifiers::NONE,
        );
        let baseline = ScratchEditAccess::selected(&mut sources)
            .snapshot()
            .unwrap();
        ScratchEditAccess::selected(&mut sources)
            .commit(&baseline, "external")
            .unwrap();
        let conflict = render(&pane, &sources, false);
        assert!(screen(&conflict).contains("CONFLICT — draft retained"));
        assert_eq!(conflict[(0, 5)].fg, Color::Yellow);
        assert_eq!(sources.selected().unwrap().text(), "external");
    }

    #[test]
    fn delimiter_rendering_tracks_cursor_without_publishing_or_editing() {
        let mut sources = ScratchBuffers::default();
        let id = sources.create("test", "(x) z");
        sources.bind_semantic_target(Some(id)).unwrap();
        let mut pane = EditableScratchPane::new();
        let root = crate::test_support::TempWorkspace::new("delimiter_rendering");
        let app = crate::app::App::new(root.workspace()).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 6)).unwrap();
        // Initialize the draft with navigation, then move onto and after the
        // closing delimiter, away from it, and back to the opening delimiter.
        for (key, highlighted) in [
            (KeyCode::Home, true),
            (KeyCode::Right, true),
            (KeyCode::Right, true),
            (KeyCode::Right, true),
            (KeyCode::Right, false),
            (KeyCode::Home, true),
        ] {
            local_key(&mut pane, &mut sources, key, KeyModifiers::NONE);
            terminal
                .draw(|frame| {
                    pane.render(
                        frame,
                        frame.area(),
                        &TuiContext::with_view_sources(&app, true, 0, Some(&sources)),
                    )
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            for x in [0, 2] {
                assert_eq!(
                    buffer[(x, 0)].modifier.contains(Modifier::UNDERLINED),
                    highlighted
                );
            }
            assert!(!buffer[(1, 0)].modifier.contains(Modifier::UNDERLINED));
            let draft = pane.draft.as_ref().unwrap();
            assert_eq!(draft.text.as_str(), "(x) z");
            assert!(draft.editing.undo.is_empty());
            assert!(draft.editing.redo.is_empty());
            assert_eq!(sources.get(id).unwrap().text(), "(x) z");
            assert_eq!(sources.get(id).unwrap().version(), 0);
            assert_eq!(
                ScratchEditAccess::new(&mut sources)
                    .snapshot()
                    .unwrap()
                    .source,
                draft.baseline.source
            );
        }
    }

    fn sources() -> ScratchBuffers {
        let mut sources = ScratchBuffers::default();
        let id = sources.create("test", "é\n");
        sources.bind_semantic_target(Some(id)).unwrap();
        sources
    }
    #[test]
    fn keys_edit_unicode_newlines_and_empty_document_then_cancel() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut sources = ScratchBuffers::default();
        let id = sources.create("test", "");
        sources.bind_semantic_target(Some(id)).unwrap();
        let mut pane = EditableScratchPane::new();
        for (key, expected) in [
            (KeyCode::Backspace, ""),
            (KeyCode::Delete, ""),
            (KeyCode::Char('猫'), "猫"),
            (KeyCode::Char('é'), "猫é"),
            (KeyCode::Enter, "猫é\n"),
            (KeyCode::Char('x'), "猫é\nx"),
            (KeyCode::Backspace, "猫é\n"),
            (KeyCode::Backspace, "猫é"),
            (KeyCode::Left, "猫é"),
            (KeyCode::Delete, "猫"),
            (KeyCode::Backspace, ""),
        ] {
            assert!(pane.handle_source_event(
                Event::Key(KeyEvent::new(key, KeyModifiers::NONE)),
                &mut ScratchEditAccess::new(&mut sources)
            ));
            let draft = pane.draft.as_ref().unwrap();
            assert_eq!(draft.text.as_str(), expected);
            assert!(expected.is_char_boundary(draft.cursor));
        }
        pane.handle_source_event(
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            &mut ScratchEditAccess::new(&mut sources),
        );
        assert!(pane.draft.is_none());
        assert_eq!(sources.get(id).unwrap().text(), "");
        assert_eq!(sources.get(id).unwrap().version(), 0);
    }

    #[test]
    fn contextual_margin_retains_viewport_and_adapts_to_height() {
        assert_eq!(contextual_top(0, 6, 10), 0);
        assert_eq!(contextual_top(0, 7, 10), 1);
        assert_eq!(contextual_top(1, 7, 10), 1);
        assert_eq!(contextual_top(10, 12, 10), 9);
        assert_eq!(contextual_top(10, 0, 10), 0);
        assert_eq!(contextual_top(0, 59, 31), 32);
        assert_eq!(contextual_top(32, 59, 25), 38);
        assert_eq!(contextual_top(38, 59, 31), 38);
        for height in 1..8 {
            let top = contextual_top(0, 20, height);
            assert!(top <= 20 && 20 < top + height);
        }
    }

    #[test]
    fn cursor_follows_unicode_draft_in_small_viewports() {
        let mut sources = sources();
        let mut pane = EditableScratchPane::new();
        pane.handle_source_event(
            Event::Paste("first\n猫é\nlast猫".into()),
            &mut ScratchEditAccess::new(&mut sources),
        );
        let root = crate::test_support::TempWorkspace::new("scratch_cursor");
        let app = crate::app::App::new(root.workspace()).unwrap();
        for (width, height) in [(0, 0), (1, 1), (2, 2), (3, 3), (6, 4), (40, 10)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    pane.render(
                        frame,
                        frame.area(),
                        &TuiContext::with_view_sources(&app, true, u16::MAX, Some(&sources)),
                    )
                })
                .unwrap();
            if width > 2 && height > 2 {
                use ratatui::backend::Backend;
                let cursor = terminal.backend_mut().get_cursor_position().unwrap();
                assert!(cursor.x < width);
                assert!(cursor.y < height - 1);
                if width == 40 {
                    assert_eq!(cursor.x, 6);
                }
            }
        }
    }

    #[test]
    fn commit_checks_baseline_and_does_not_follow_selection() {
        let mut sources = sources();
        let baseline = ScratchEditAccess::new(&mut sources).snapshot().unwrap();
        sources.create("test", "other");
        let mut access = ScratchEditAccess::new(&mut sources);
        let next = access.commit(&baseline, "猫\n").unwrap();
        assert_eq!(next.version, baseline.version + 1);
        assert_eq!(
            access.commit(&baseline, "lost"),
            Err(ScratchEditError::Stale)
        );
        assert_eq!(
            access.sources.as_ref().unwrap().selected().unwrap().text(),
            "other"
        );
    }
    #[test]
    fn clean_draft_follows_committed_changes_before_typing() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut sources = sources();
        let mut pane = EditableScratchPane::new();
        pane.handle_source_event(
            Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
            &mut ScratchEditAccess::new(&mut sources),
        );
        let baseline = ScratchEditAccess::new(&mut sources).snapshot().unwrap();
        let updated = ScratchEditAccess::new(&mut sources)
            .commit(&baseline, "猫")
            .unwrap();
        let root = crate::test_support::TempWorkspace::new("clean-draft-refresh");
        let app = crate::app::App::new(root.workspace()).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 8)).unwrap();
        terminal
            .draw(|frame| {
                pane.render(
                    frame,
                    frame.area(),
                    &TuiContext::with_view_sources(&app, true, 0, Some(&sources)),
                );
            })
            .unwrap();
        let displayed: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(displayed.contains('猫'));
        assert!(!displayed.contains('é'));
        assert!(!displayed.contains("CONFLICT"));
        // The old cursor was byte 2; refreshing must not split the new scalar.
        pane.handle_source_event(
            Event::Paste("new ".into()),
            &mut ScratchEditAccess::new(&mut sources),
        );
        let draft = pane.draft.as_ref().unwrap();
        assert_eq!(draft.baseline, updated);
        assert_eq!(draft.text.as_str(), "new 猫");
        assert_eq!(
            ScratchEditAccess::new(&mut sources).snapshot().unwrap(),
            updated
        );
        pane.handle_source_event(
            Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            &mut ScratchEditAccess::new(&mut sources),
        );
        let published = ScratchEditAccess::new(&mut sources).snapshot().unwrap();
        assert_eq!(published.source.source_id, baseline.source.source_id);
        assert_eq!(published.version, updated.version + 1);
        assert_eq!(published.source.text.as_ref(), "new 猫");
    }

    #[test]
    fn draft_survives_render_and_conflict_then_cancels() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut sources = sources();
        let mut pane = EditableScratchPane::new();
        pane.handle_source_event(
            Event::Paste("猫".into()),
            &mut ScratchEditAccess::new(&mut sources),
        );
        assert_eq!(sources.semantic_target().unwrap().unwrap().text(), "é\n");
        let root = crate::test_support::TempWorkspace::new("scratch_editor_render");
        let app = crate::app::App::new(root.workspace()).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 8)).unwrap();
        for _ in 0..2 {
            terminal
                .draw(|frame| {
                    pane.render(
                        frame,
                        frame.area(),
                        &TuiContext::with_view_sources(&app, true, 0, Some(&sources)),
                    )
                })
                .unwrap();
        }
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "猫é\n");
        let baseline = ScratchEditAccess::new(&mut sources).snapshot().unwrap();
        ScratchEditAccess::new(&mut sources)
            .commit(&baseline, "external")
            .unwrap();
        pane.handle_source_event(
            Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            &mut ScratchEditAccess::new(&mut sources),
        );
        assert!(pane.status.as_ref().unwrap().contains("conflict"));
        assert!(pane.draft.is_some());
        pane.handle_source_event(
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            &mut ScratchEditAccess::new(&mut sources),
        );
        assert!(pane.draft.is_none());
        assert_eq!(
            sources.semantic_target().unwrap().unwrap().text(),
            "external"
        );
    }

    #[test]
    fn render_follows_selection_before_input_and_restores_parked_draft() {
        let mut sources = sources();
        let first = sources.selected().unwrap().id();
        let mut pane = EditableScratchPane::new();
        pane.handle_source_event(
            Event::Paste("LOCAL".into()),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        let second = sources.create("test", "SECOND");
        let root = crate::test_support::TempWorkspace::new("selected_editor_render");
        let app = crate::app::App::new(root.workspace()).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 8)).unwrap();
        for (id, expected) in [(second, "SECOND"), (first, "LOCALé")] {
            sources.select(id).unwrap();
            terminal
                .draw(|frame| {
                    pane.render(
                        frame,
                        frame.area(),
                        &TuiContext::with_view_sources(&app, true, 0, Some(&sources)),
                    )
                })
                .unwrap();
            let rendered: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(rendered.contains(expected));
        }
        sources.select(second).unwrap();
        pane.handle_source_event(
            Event::Paste("DRAFT".into()),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        sources.select(first).unwrap();
        terminal
            .draw(|frame| {
                pane.render(
                    frame,
                    frame.area(),
                    &TuiContext::with_view_sources(&app, true, 0, Some(&sources)),
                )
            })
            .unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("LOCALé"));
        assert!(!rendered.contains("DRAFTSECOND"));
        assert!(TuiContext::new(&app).selected_source().is_none());
    }

    #[test]
    fn selected_drafts_restore_text_cursor_and_leave_binding_unchanged() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut sources = sources();
        let first = sources.selected().unwrap().id();
        let second = sources.create("test", "second");
        let binding = sources.bound_source();
        let mut pane = EditableScratchPane::new();
        sources.select(first).unwrap();
        pane.handle_source_event(
            Event::Paste("猫".into()),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        let first_cursor = pane.draft.as_ref().unwrap().cursor;
        sources.select(second).unwrap();
        pane.handle_source_event(
            Event::Paste("other".into()),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        let second_cursor = pane.draft.as_ref().unwrap().cursor;
        sources.select(first).unwrap();
        pane.handle_source_event(
            Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "猫é\n");
        assert_eq!(
            pane.draft.as_ref().unwrap().cursor,
            first_cursor + "é".len()
        );
        sources.select(second).unwrap();
        pane.handle_source_event(
            Event::Paste("!".into()),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        assert_eq!(pane.draft.as_ref().unwrap().cursor, second_cursor + 1);
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "other!second");
        pane.handle_source_event(
            Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        assert_eq!(sources.get(second).unwrap().text(), "other!second");
        assert_eq!(sources.bound_source(), binding);
        sources.bind_semantic_target(None).unwrap();
        let baseline = ScratchEditAccess::selected(&mut sources)
            .snapshot()
            .unwrap();
        ScratchEditAccess::selected(&mut sources)
            .commit(&baseline, "unbound edit")
            .unwrap();
        assert_eq!(sources.semantic_target_id(), None);
    }

    #[test]
    fn commit_preserves_editor_state_and_refreshes_baseline() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut sources = sources();
        let mut pane = EditableScratchPane::new();
        pane.handle_source_event(
            Event::Paste("猫".into()),
            &mut ScratchEditAccess::new(&mut sources),
        );
        let draft = pane.draft.as_mut().unwrap();
        draft.preferred = Some(7);
        let cursor = draft.cursor;
        for _ in 0..2 {
            pane.handle_source_event(
                Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
                &mut ScratchEditAccess::new(&mut sources),
            );
            let draft = pane.draft.as_ref().unwrap();
            assert_eq!(draft.cursor, cursor);
            assert_eq!(draft.preferred, Some(7));
            assert_eq!(draft.text.as_str(), "猫é\n");
            assert_eq!(
                draft.baseline,
                ScratchEditAccess::new(&mut sources).snapshot().unwrap()
            );
            assert!(!pane.has_dirty_draft());
            assert!(pane.dirty_draft_sources().is_empty());
            assert_eq!(pane.status.as_deref(), Some("Published in memory"));
        }
        pane.handle_source_event(
            Event::Paste("!".into()),
            &mut ScratchEditAccess::new(&mut sources),
        );
        assert_eq!(pane.draft.as_ref().unwrap().text.as_str(), "猫!é\n");
        assert!(pane.has_dirty_draft());
    }

    #[test]
    fn dirty_sources_include_active_and_parked_but_not_clean_drafts() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut sources = sources();
        let first = sources.selected().unwrap().id();
        let mut pane = EditableScratchPane::new();
        assert!(pane.dirty_draft_sources().is_empty());
        pane.handle_source_event(
            Event::Paste("first".into()),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        let first_source = pane.dirty_draft_sources().remove(0);
        assert_eq!(first_source.0, first);
        let second = sources.create("test", "second");
        pane.handle_source_event(
            Event::Paste("dirty".into()),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        let baseline = &pane.draft.as_ref().unwrap().baseline;
        assert_eq!(
            pane.dirty_draft_sources(),
            vec![
                (
                    second,
                    format!(
                        "{:?} revision {:?}",
                        baseline.source.source_id, baseline.source.revision_id
                    )
                ),
                first_source.clone(),
            ]
        );
        pane.handle_source_event(
            Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        assert!(pane.has_dirty_draft());
        assert_eq!(pane.dirty_draft_sources(), vec![first_source]);
        sources.select(first).unwrap();
        pane.handle_source_event(
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            &mut ScratchEditAccess::selected(&mut sources),
        );
        assert!(!pane.has_dirty_draft());
        assert!(pane.dirty_draft_sources().is_empty());
    }

    #[test]
    fn closed_rebound_and_other_owner_are_rejected() {
        let mut sources = sources();
        let baseline = ScratchEditAccess::new(&mut sources).snapshot().unwrap();
        let mut other = self::sources();
        assert_eq!(
            ScratchEditAccess::new(&mut other).commit(&baseline, "bad"),
            Err(ScratchEditError::Rebound)
        );
        sources.discard_selected().unwrap();
        assert_eq!(
            ScratchEditAccess::new(&mut sources).commit(&baseline, "bad"),
            Err(ScratchEditError::Closed)
        );
        let id = sources.create("test", "new");
        sources.bind_semantic_target(Some(id)).unwrap();
        assert_eq!(
            ScratchEditAccess::new(&mut sources).commit(&baseline, "bad"),
            Err(ScratchEditError::Rebound)
        );
    }

    #[test]
    fn selected_indentation_is_atomic_preserves_direction_and_never_publishes() {
        for reverse in [false, true] {
            for (code, modifiers, original, expected, mapped) in [
                (
                    KeyCode::Tab,
                    KeyModifiers::NONE,
                    "é\nx\nz",
                    "    é\n    x\nz",
                    (4, 13),
                ),
                (
                    KeyCode::BackTab,
                    KeyModifiers::SHIFT,
                    "  é\n    x\nz",
                    "é\nx\nz",
                    (0, 5),
                ),
                (
                    KeyCode::Tab,
                    KeyModifiers::SHIFT,
                    "  é\n    x\nz",
                    "é\nx\nz",
                    (0, 5),
                ),
                (
                    KeyCode::BackTab,
                    KeyModifiers::SHIFT,
                    "é\nx\nz",
                    "é\nx\nz",
                    (0, 5),
                ),
            ] {
                let mut sources = sources();
                let mut pane = EditableScratchPane::new();
                local_key(&mut pane, &mut sources, KeyCode::Home, KeyModifiers::NONE);
                let revision = sources.selected().unwrap().version();
                let published = sources.selected().unwrap().text().to_owned();
                let draft = pane.draft.as_mut().unwrap();
                draft.text = TextBuffer::new(original);
                let end = original.rfind('z').unwrap();
                draft.cursor = if reverse { 0 } else { end };
                draft.editing.anchor = Some(if reverse { end } else { 0 });
                let before = draft.state();
                local_key(&mut pane, &mut sources, code, modifiers);
                let draft = pane.draft.as_ref().unwrap();
                assert_eq!(draft.text.as_str(), expected);
                assert_eq!(draft.cursor, if reverse { mapped.0 } else { mapped.1 });
                assert_eq!(
                    draft.editing.anchor,
                    Some(if reverse { mapped.1 } else { mapped.0 })
                );
                let changed = original != expected;
                assert_eq!(draft.editing.undo.len(), usize::from(changed));
                let after = draft.state();
                local_key(
                    &mut pane,
                    &mut sources,
                    KeyCode::Char('z'),
                    KeyModifiers::CONTROL,
                );
                let draft = pane.draft.as_ref().unwrap();
                assert_eq!(draft.text.as_str(), before.text);
                assert_eq!(draft.cursor, before.cursor);
                assert_eq!(draft.editing.anchor, before.anchor);
                assert_eq!(draft.editing.redo.len(), usize::from(changed));
                local_key(
                    &mut pane,
                    &mut sources,
                    KeyCode::Char('y'),
                    KeyModifiers::CONTROL,
                );
                let draft = pane.draft.as_ref().unwrap();
                assert_eq!(draft.text.as_str(), after.text);
                assert_eq!(draft.cursor, after.cursor);
                assert_eq!(draft.editing.anchor, after.anchor);
                assert!(draft.editing.redo.is_empty());
                assert_eq!(sources.selected().unwrap().version(), revision);
                assert_eq!(sources.selected().unwrap().text(), published);
            }
        }
    }
}

#[cfg(test)]
mod app_target_tests {
    use super::*;
    use crate::document::{Document, DocumentId};

    #[test]
    fn app_target_rejects_stale_and_rebound_baselines_without_writes() {
        let mut document = Document::new(DocumentId::new(1), "file.txt", "original λ");
        let baseline = ScratchEditAccess::app(&mut document).snapshot().unwrap();
        let range = document
            .buffer()
            .range_for_span(Span::new(0, document.text().len()))
            .unwrap();
        document
            .apply_edit(TextEdit::replace(range, "external"))
            .unwrap();
        assert_eq!(
            ScratchEditAccess::app(&mut document).commit(&baseline, "draft"),
            Err(ScratchEditError::Stale)
        );
        assert_eq!(document.text(), "external");
        let mut replacement = Document::new(DocumentId::new(1), "file.txt", "original λ");
        assert_eq!(
            ScratchEditAccess::app(&mut replacement).commit(&baseline, "draft"),
            Err(ScratchEditError::Rebound)
        );
        assert_eq!(replacement.text(), "original λ");
        let current = ScratchEditAccess::app(&mut replacement).snapshot().unwrap();
        let no_op = ScratchEditAccess::app(&mut replacement)
            .commit(&current, "original λ")
            .unwrap();
        assert_eq!(no_op, current);
        let published = ScratchEditAccess::app(&mut replacement)
            .commit(&current, "猫\nλ\n")
            .unwrap();
        assert_eq!(published.version, 1);
        assert_eq!(published.source.source_id, current.source.source_id);
        assert_eq!(replacement.text(), "猫\nλ\n");
    }
}
