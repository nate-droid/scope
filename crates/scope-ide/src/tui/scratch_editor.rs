//! Explicit, in-memory scratch editing authority. No App, filesystem or execution access.
use std::{io, ops::Range};

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span as RenderSpan},
    widgets::{Block, Borders, Paragraph},
};
use scope_terminal::DocumentEditor;
use unicode_segmentation::UnicodeSegmentation;

use super::{Pane, TuiContext};
use crate::{
    scratch::{BoundSource, ScratchBuffers},
    text::{Span, TextBuffer, TextEdit},
};

/// An owned optimistic-concurrency baseline, including owner-independent identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchEditSnapshot {
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
            Self::Unbound => "no scratch target bound",
            Self::Closed => "scratch target closed",
            Self::Rebound => "scratch target rebound",
            Self::Stale => "conflict: scratch changed",
            Self::Edit(message) => message,
        })
    }
}
impl std::error::Error for ScratchEditError {}

/// Short-lived capability supplied only to explicitly authorized panes.
/// It cannot select, bind, save, evaluate, or access application state.
pub struct ScratchEditAccess<'a> {
    sources: &'a mut ScratchBuffers,
    selected: bool,
}
impl<'a> ScratchEditAccess<'a> {
    #[cfg(test)]
    pub(super) fn new(sources: &'a mut ScratchBuffers) -> Self {
        Self {
            sources,
            selected: false,
        }
    }

    /// Edits the selected scratch independently of the semantic binding.
    pub fn selected(sources: &'a mut ScratchBuffers) -> Self {
        Self {
            sources,
            selected: true,
        }
    }

    fn target_id(&self) -> Option<u64> {
        if self.selected {
            self.sources.selected().ok().map(|buffer| buffer.id())
        } else {
            self.sources.semantic_target_id()
        }
    }

    pub fn snapshot(&self) -> Result<ScratchEditSnapshot, ScratchEditError> {
        let id = self.target_id().ok_or(ScratchEditError::Unbound)?;
        let buffer = self.sources.get(id).map_err(|_| ScratchEditError::Closed)?;
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
            self.sources
                .get_mut(expected.scratch_id)
                .map_err(|_| ScratchEditError::Closed)?
                .apply_edit(TextEdit::replace(range, text))
                .map_err(|error| ScratchEditError::Edit(error.to_string()))?;
        }
        self.snapshot()
    }
}

struct Draft {
    baseline: ScratchEditSnapshot,
    text: TextBuffer,
    cursor: usize,
    preferred: Option<usize>,
    editing: LocalEditing,
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
                        KeyCode::Enter | KeyCode::Tab | KeyCode::Backspace | KeyCode::Delete
                    )
                }));
        let before = self.state();
        let had_selection = self.selection().is_some();
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
        if mutation {
            if let Some(range) = self.selection() {
                self.replace(range.clone(), "")?;
                self.cursor = range.start;
            }
            self.editing.anchor = None;
        }
        let mut preferred = self.preferred.take();
        let result = if deleting && had_selection {
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
                        text: TextBuffer::new(baseline.source.text.as_ref()),
                        baseline,
                        cursor: 0,
                        preferred: None,
                        editing: LocalEditing::default(),
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
        let result = draft.event(event);
        match result {
            Ok(Some(false)) => {
                self.draft = None;
                self.status = Some("draft cancelled".into());
            }
            Ok(Some(true)) => match access.commit(&draft.baseline, draft.text.as_str()) {
                Ok(baseline) => {
                    draft.baseline = baseline;
                    self.status = Some("committed to scratch (not saved)".into());
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
    fn has_dirty_draft(&self) -> bool {
        self.draft
            .iter()
            .chain(self.parked.iter())
            .any(|draft| draft.text.as_str() != draft.baseline.source.text.as_ref())
    }

    fn dirty_draft_sources(&self) -> Vec<(u64, String)> {
        self.draft
            .iter()
            .chain(self.parked.iter())
            .filter(|draft| draft.text.as_str() != draft.baseline.source.text.as_ref())
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
        let selected = ctx.selected_source();
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
            (_, Some(BoundSource::Unbound)) => "UNBOUND — no scratch target bound",
            (_, None) => "UNAVAILABLE — no scratch owner supplied",
        };
        let title = format!(
            "Editor {} | {} | {} | Ctrl-S publish · F5 run · Ctrl-C discard",
            if ctx.is_focused() {
                "FOCUSED"
            } else {
                "unfocused"
            },
            if draft.is_some_and(|draft| draft.text.as_str() != draft.baseline.source.text.as_ref())
            {
                "LOCAL DRAFT"
            } else {
                "type to edit"
            },
            conflict
                .or(self.status.as_deref())
                .unwrap_or(if draft.is_some() {
                    "local draft"
                } else {
                    "ready"
                })
        );
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let cursor = draft.map(|draft| {
            let prefix = &text[..draft.cursor];
            (
                prefix.bytes().filter(|byte| *byte == b'\n').count(),
                Line::raw(prefix.rsplit('\n').next().unwrap_or("")).width(),
            )
        });
        let (top, left) = cursor
            .map(|(row, column)| {
                (
                    row.saturating_sub(usize::from(inner.height) - 1),
                    column.saturating_sub(usize::from(inner.width) - 1),
                )
            })
            .unwrap_or((usize::from(ctx.scroll()), 0));
        // Slice before rendering rather than narrowing large source offsets to u16.
        let lines: Vec<Line<'_>> = text
            .split('\n')
            .skip(top)
            .take(usize::from(inner.height))
            .map(|line| {
                let mut width = 0;
                let mut start = 0;
                for (byte, ch) in line.grapheme_indices(true) {
                    let next = width + Line::raw(ch.to_string()).width();
                    if next > left {
                        break;
                    }
                    width = next;
                    start = byte + ch.len();
                }
                // A viewport cutting a wide scalar leaves blank cells, not half a glyph.
                let padding = if width < left {
                    let ch = line[start..].graphemes(true).next();
                    if let Some(ch) = ch {
                        start += ch.len();
                        (width + Line::raw(ch.to_string()).width()).saturating_sub(left)
                    } else {
                        0
                    }
                } else {
                    0
                };
                let line_offset = line.as_ptr() as usize - text.as_ptr() as usize;
                let selection = draft.and_then(Draft::selection);
                let mut spans = vec![RenderSpan::raw(" ".repeat(padding))];
                spans.extend(
                    line[start..]
                        .grapheme_indices(true)
                        .map(|(offset, grapheme)| {
                            let byte = line_offset + start + offset;
                            if selection
                                .as_ref()
                                .is_some_and(|range| range.contains(&byte))
                            {
                                RenderSpan::styled(
                                    grapheme,
                                    Style::default().add_modifier(Modifier::REVERSED),
                                )
                            } else {
                                RenderSpan::raw(grapheme)
                            }
                        }),
                );
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
                assert!(cursor.x >= 1 && cursor.x < width - 1);
                assert!(cursor.y >= 1 && cursor.y < height - 1);
                if width == 40 {
                    assert_eq!(cursor.x, 7);
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
        assert_eq!(access.sources.selected().unwrap().text(), "other");
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
            assert_eq!(
                pane.status.as_deref(),
                Some("committed to scratch (not saved)")
            );
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
}
