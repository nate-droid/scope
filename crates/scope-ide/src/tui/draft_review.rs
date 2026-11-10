//! Draft-local formatting preview. This owns evidence, never a source target.
use super::{EditorDraftSnapshot, TrustedClosureProvider};
use crate::{app::AppState, scratch::ScratchBuffers};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph},
};
use scope_language::{
    LanguageDocument, LanguagePath, LanguageService, LineIndex, SemanticOperationEffect,
    SemanticOperationRequest, SemanticOperationResult, SemanticTargetRequirement,
};

use std::cell::Cell;
use unicode_segmentation::UnicodeSegmentation;

const MAX_PREVIEW_BYTES: usize = 256 * 1024;

pub(super) struct DraftReview {
    pub(super) pinned: EditorDraftSnapshot,
    pub(super) replacement: Option<String>,
    message: String,
    scroll: usize,
    max_scroll: Cell<usize>,
}

pub(super) fn format_supported(service: &dyn LanguageService) -> bool {
    let operations = service.semantic_operations();
    let mut matching = operations.iter().filter(|op| op.id == "format");
    let Some(operation) = matching.next() else {
        return false;
    };
    matching.next().is_none()
        && operation.effect == SemanticOperationEffect::ProposeChanges
        && operation.target == SemanticTargetRequirement::Document
        && operation
            .arguments
            .iter()
            .all(|argument| !argument.required)
        && operation.validate_arguments(&serde_json::json!({})).is_ok()
}

impl DraftReview {
    pub(super) fn capture<P: TrustedClosureProvider>(
        app: &AppState<P>,
        sources: Option<&ScratchBuffers>,
        app_editor: bool,
        pinned: EditorDraftSnapshot,
    ) -> Self {
        let result = (|| {
            let path = if app_editor {
                app.open_document().map(|document| document.path())
            } else {
                sources
                    .and_then(|sources| sources.get(pinned.baseline.scratch_id).ok())
                    .and_then(|source| source.destination_path())
            }
            .ok_or_else(|| "Format requires an existing document language path".to_string())?;
            let service = app
                .languages()
                .language_for_path(path)
                .ok_or_else(|| "No registered language service for this path".to_string())?;
            if !format_supported(service) {
                return Err(
                    "Language does not support whole-document Format with optional arguments"
                        .into(),
                );
            }
            if pinned.text.len() > MAX_PREVIEW_BYTES {
                return Err("Format preview source exceeds the 256 KiB UI limit".into());
            }
            let result = scope_language::execute_semantic_operation(
                service,
                SemanticOperationRequest {
                    operation_id: "format",
                    document: LanguageDocument::new(
                        LanguagePath::new(path.to_string_lossy()),
                        &pinned.text,
                        pinned.baseline.version,
                    ),
                    selection: None,
                    arguments: serde_json::json!({}),
                },
            )
            .map_err(|error| format!("Format failed: {error:?}"))?;
            Self::candidate(&pinned.text, result)
        })();
        let (replacement, message) = match result {
            Ok((replacement, message)) => (replacement, message),
            Err(error) => (None, error),
        };
        Self {
            pinned,
            replacement,
            message: bounded(&message),
            scroll: 0,
            max_scroll: Cell::new(0),
        }
    }

    fn candidate(
        source: &str,
        result: SemanticOperationResult,
    ) -> Result<(Option<String>, String), String> {
        let SemanticOperationResult::Changes { edits, rationale } = result else {
            return Err("Format returned an unexpected result; nothing changed".into());
        };
        match edits.as_slice() {
            [] => Ok((
                None,
                "Already canonically formatted; no changes proposed.".into(),
            )),
            [edit]
                if Some(edit.range) == LineIndex::new(source).range_for_span(0..source.len()) =>
            {
                if edit.replacement.len() > MAX_PREVIEW_BYTES {
                    return Err(
                        "Format result exceeds the 256 KiB UI preview limit; nothing changed"
                            .into(),
                    );
                }
                if edit.replacement == source {
                    return Ok((
                        None,
                        "Already canonically formatted; no changes proposed.".into(),
                    ));
                }
                Ok((Some(edit.replacement.clone()), rationale))
            }
            _ => Err(
                "Format must return one whole-document replacement or no edits; nothing changed"
                    .into(),
            ),
        }
    }

    pub(super) fn fail(&mut self, error: String) {
        self.replacement = None;
        self.message = bounded(&error);
        self.scroll = 0;
    }

    /// Only an explicit Enter applies. Other input cannot reach the editor.
    pub(super) fn decision(&mut self, key: KeyEvent) -> Option<super::DraftApplyDecision> {
        self.scroll = self.scroll.min(self.max_scroll.get());
        match key.code {
            KeyCode::Esc => Some(super::DraftApplyDecision::Cancel),
            KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
                Some(super::DraftApplyDecision::Cancel)
            }
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE && self.replacement.is_some() => {
                Some(super::DraftApplyDecision::Apply)
            }
            KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                None
            }
            KeyCode::Down => {
                self.scroll = self.scroll.saturating_add(1);
                None
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
                None
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
                None
            }
            KeyCode::Home => {
                self.scroll = 0;
                None
            }
            KeyCode::End => {
                self.scroll = self.max_scroll.get();
                None
            }
            _ => None,
        }
    }

    pub(super) fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let area = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        let mut text = String::from(if self.replacement.is_some() {
            "Format preview — Enter: Apply • Esc / c: Cancel • PgUp/PgDn: scroll\nLocal editor only; does not publish, save, check or run.\n\n"
        } else {
            "Format preview — Esc / c: Close • PgUp/PgDn: scroll\nNo applicable replacement; editor source is unchanged.\n\n"
        });
        text.push_str(&self.message);
        if let Some(replacement) = &self.replacement {
            text.push_str("\n\n--- Current editor draft\n");
            text.push_str(&self.pinned.text);
            text.push_str("\n\n+++ Proposed formatted draft\n");
            text.push_str(replacement);
        }
        // Never forward source control characters to the terminal backend.
        let text: String = text
            .chars()
            .map(|c| {
                if c.is_control() && c != '\n' {
                    '�'
                } else {
                    c
                }
            })
            .collect();
        // Slice a usize-indexed, cell-wrapped viewport rather than narrowing
        // logical offsets to Paragraph's u16 scroll. Both source and replacement
        // remain reachable even for newline-heavy or narrowly wrapped previews.
        let rows = preview_rows(&text, usize::from(area.width.saturating_sub(2)).max(1));
        let height = usize::from(area.height.saturating_sub(2));
        let max_scroll = rows.len().saturating_sub(height.max(1));
        self.max_scroll.set(max_scroll);
        let visible = rows
            .into_iter()
            .skip(self.scroll.min(max_scroll))
            .take(height)
            .map(ratatui::text::Line::raw)
            .collect::<Vec<_>>();
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(visible).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Format preview"),
            ),
            area,
        );
    }
}

fn preview_rows(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for line in text.split('\n') {
        let mut row = String::new();
        let mut cells = 0;
        for grapheme in line.graphemes(true) {
            let size = ratatui::text::Line::raw(grapheme).width();
            if cells + size > width && !row.is_empty() {
                rows.push(std::mem::take(&mut row));
                cells = 0;
            }
            if size > width {
                row.push('�');
                cells += 1;
            } else {
                row.push_str(grapheme);
                cells += size;
            }
        }
        rows.push(row);
    }
    rows
}

fn bounded(text: &str) -> String {
    let mut end = text.len().min(8192);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = text[..end].to_string();
    if end < text.len() {
        result.push_str("\n[message truncated]");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_preview_is_reachable_past_u16_rows_and_wraps_unicode() {
        let text = format!("{}proposed replacement", "\n".repeat(70_000));
        let rows = preview_rows(&text, 8);
        assert!(rows.len() > u16::MAX as usize);
        assert_eq!(rows[70_000..].concat(), "proposed replacement");
        assert_eq!(preview_rows("é猫e\u{301}🙂", 3), ["é猫", "e\u{301}🙂"]);
        assert_eq!(preview_rows("", 1), [""]);
    }
}
