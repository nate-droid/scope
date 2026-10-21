//! Generic, synchronous read inspection. Opening this modal never executes a provider.
use crate::{
    app::App,
    semantic_inspection::{self, InspectionBinding, InspectionResult},
    tool::ToolValue,
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use scope_language::SemanticOperationResult;
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct SemanticInspection {
    selected: usize,
    pending: Option<InspectionBinding>,
    result: Option<InspectionResult>,
    error: Option<String>,
    scroll: u16,
}

impl SemanticInspection {
    /// Returns true when the modal should close.
    pub fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> bool {
        match key.code {
            KeyCode::Esc => {
                if self.pending.take().is_none() {
                    return true;
                }
            }
            KeyCode::Up | KeyCode::Down => {
                self.pending = None;
                let count = semantic_inspection::discover_app(app).map_or(0, |ops| ops.len());
                self.selected = if key.code == KeyCode::Up {
                    self.selected.saturating_sub(1)
                } else {
                    self.selected.saturating_add(1).min(count.saturating_sub(1))
                };
                self.scroll = 0;
            }
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::Home => self.scroll = 0,
            KeyCode::Enter => {
                self.error = None;
                self.scroll = 0;
                if let Some(binding) = self.pending.take() {
                    // The second Enter grants only this captured request. The bridge
                    // independently checks every field before consulting this policy.
                    match binding.invoke_app(app, |request| {
                        request.operation == binding.operation()
                            && request.source.id == binding.source_id()
                            && request.source.version == binding.version()
                            && request.source.text == binding.text()
                            && request.source.language == binding.language()
                            && request.arguments == &json!({})
                    }) {
                        Ok(result) => self.result = Some(result),
                        Err(error) => {
                            self.result = None;
                            self.error = Some(error.to_string());
                        }
                    }
                } else {
                    match semantic_inspection::discover_app(app).and_then(|ops| {
                        let operation = ops.get(self.selected).ok_or_else(|| {
                            semantic_inspection::InspectionError::UnknownOperation(
                                "no selected operation; reopen inspection".into(),
                            )
                        })?;
                        InspectionBinding::capture_app(app, &operation.id)
                    }) {
                        Ok(binding) => self.pending = Some(binding),
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
            }
            _ => {}
        }
        false
    }

    pub fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
        let area = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        let mut text = String::from(
            "Read semantic operations — ↑/↓ select • Enter review/confirm • PgUp/PgDn scroll • Esc cancel/close\nDiscovery is not permission. Read is not an OS sandbox.\n\n",
        );
        if let Some(document) = app.open_document() {
            text.push_str(&format!(
                "Document: {} — {:?} — revision {}\n",
                document.path().display(),
                document.id(),
                document.version()
            ));
        }
        match semantic_inspection::discover_app(app) {
            Ok(ops) => {
                if ops.is_empty() {
                    text.push_str("No supported read operations.\n");
                }
                for (index, operation) in ops.iter().enumerate() {
                    text.push_str(&format!(
                        "{} {} — {}\n",
                        if index == self.selected { ">" } else { " " },
                        operation.id,
                        operation.description
                    ));
                }
            }
            Err(error) => text.push_str(&format!("{error}\n")),
        }
        if let Some(binding) = &self.pending {
            text.push_str(&format!("\nConfirm read: {} / {} / {:?} / revision {} / arguments {{}}\nEnter authorizes exactly this snapshot; Esc cancels.\n", binding.language(), binding.operation().id, binding.source_id(), binding.version()));
            if !binding.is_fresh_app(app) {
                text.push_str("STALE confirmation — invocation will be rejected.\n");
            }
        }
        if let Some(error) = &self.error {
            text.push_str(&format!("\nInvocation error: {error}\n"));
        }
        if let Some(result) = &self.result {
            text.push_str(&format!(
                "\n{} — operation {} / language {} / {:?} / observed revision {}\n",
                if result.is_fresh_app(app) {
                    "CURRENT"
                } else {
                    "STALE (source changed or no longer open)"
                },
                result.binding.operation().id,
                result.binding.language(),
                result.binding.source_id(),
                result.binding.version()
            ));
            // Native JSON is authoritative; the agent ToolValue protocol cannot
            // represent floating-point or unsigned values above i64::MAX.
            let value = match &result.native_result {
                Some(SemanticOperationResult::ReadOnly { value, rationale }) => {
                    json!({"status":"read_only", "value":value, "rationale":rationale})
                }
                Some(SemanticOperationResult::ReadOnlyEvaluation {
                    value,
                    rationale,
                    evaluation,
                }) => {
                    let checks: Vec<_> = evaluation.checks.iter().map(|(id, check)| json!({"id":id,"outcome":format!("{:?}", check.outcome),"actual":check.actual})).collect();
                    json!({"status":"read_only","value":value,"rationale":rationale,"evaluation":{"checks":checks}})
                }
                Some(SemanticOperationResult::Evaluated {
                    value,
                    rationale,
                    evaluation,
                }) => {
                    json!({"status":"read_only","value":value,"rationale":rationale,"evaluation":evaluation})
                }
                _ => tool_json(&result.result.value),
            };
            text.push_str("Invocation feedback (not a Scope validation certificate):\n");
            text.push_str(&serde_json::to_string_pretty(&value).expect("JSON values serialize"));
        }
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Semantic inspection — F8"),
        );
        let max = paragraph
            .line_count(area.width.saturating_sub(2).max(1))
            .saturating_sub(area.height.saturating_sub(2) as usize)
            .min(u16::MAX as usize) as u16;
        frame.render_widget(Clear, area);
        frame.render_widget(paragraph.scroll((self.scroll.min(max), 0)), area);
    }
}

fn tool_json(value: &ToolValue) -> Value {
    match value {
        ToolValue::Null => Value::Null,
        ToolValue::Boolean(value) => (*value).into(),
        ToolValue::Integer(value) => (*value).into(),
        ToolValue::String(value) => value.clone().into(),
        ToolValue::Array(values) => Value::Array(values.iter().map(tool_json).collect()),
        ToolValue::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), tool_json(value)))
                .collect(),
        ),
    }
}
