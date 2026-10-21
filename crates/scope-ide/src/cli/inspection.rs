//! Shell presentation/state for the existing revision-bound inspection bridge.
use super::{ShellCommand, inspection_text, inspection_text_limit};
use crate::{
    app::App,
    scratch::ScratchBuffers,
    semantic_inspection::{InspectionBinding, InspectionError, InspectionResult, discover_app},
    tool::ToolValue,
};
use scope_language::SemanticOperationResult;
use serde_json::{Value, json};
use std::io::{self, Write};

struct Capture {
    binding: InspectionBinding,
    path: String,
    stale: bool,
}
impl Capture {
    fn observe(&mut self, app: &App) {
        // Latch staleness; displaying history must not renew an old observation.
        if !self.stale {
            self.stale = !self.binding.is_fresh_app(app)
                || !InspectionBinding::capture_app(app, &self.binding.operation().id)
                    .is_ok_and(|current| current == self.binding);
        }
    }
    fn label(&self) -> String {
        format!(
            "App document {:?} / path {} / version {} / language {} / operation {} / binding {}",
            self.binding.source_id(),
            inspection_text(&self.path),
            self.binding.version(),
            inspection_text(self.binding.language()),
            inspection_text(&self.binding.operation().id),
            if self.stale { "STALE" } else { "CURRENT" },
        )
    }
}
struct Record {
    capture: Capture,
    outcome: Result<InspectionResult, InspectionError>,
}
#[derive(Default)]
pub(super) struct Inspections {
    next_token: u64,
    pending: Option<(u64, Capture)>,
    result: Option<Record>,
}
impl Inspections {
    pub(super) fn observe(&mut self, app: &App) {
        if let Some((_, capture)) = &mut self.pending {
            capture.observe(app);
        }
        if let Some(record) = &mut self.result {
            record.capture.observe(app);
        }
    }

    pub(super) fn run(
        &mut self,
        command: ShellCommand,
        app: &mut App,
        scratches: &ScratchBuffers,
        output: &mut impl Write,
    ) -> io::Result<()> {
        self.observe(app);
        if let Ok(buffer) = scratches.selected() {
            writeln!(
                output,
                "Selected shell buffer {} is not the inspection target (App document only).",
                buffer.id()
            )?;
        }
        match command {
            ShellCommand::Inspect => {
                if !print_source(app, output)? {
                    return Ok(());
                }
                match discover_app(app) {
                    Ok(operations) => {
                        if operations.is_empty() {
                            writeln!(
                                output,
                                "No eligible semantic operations (requires Read, Document target, no declared arguments)."
                            )?;
                        }
                        for operation in operations {
                            writeln!(
                                output,
                                "{} — {} [Read / Document / arguments {{}}]",
                                terminal_safe_json_line(
                                    &serde_json::to_string(&operation.id)
                                        .expect("strings serialize")
                                ),
                                inspection_text(&operation.description)
                            )?;
                        }
                        writeln!(
                            output,
                            "Capture without executing: :scope inspect operation <id>"
                        )?;
                    }
                    Err(error) => print_error(&error, output)?,
                }
            }
            ShellCommand::InspectOperation(id) => {
                // A failed replacement must not leave an older approval armed.
                self.pending = None;
                if !print_source(app, output)? {
                    return Ok(());
                }
                match InspectionBinding::capture_app(app, &id) {
                    Ok(binding) => {
                        let Some(token) = self.next_token.checked_add(1) else {
                            return writeln!(
                                output,
                                "error: inspection token space exhausted; restart the shell"
                            );
                        };
                        self.next_token = token;
                        let capture = Capture {
                            binding,
                            path: app.open_document().unwrap().path().to_string(),
                            stale: false,
                        };
                        writeln!(output, "Captured request {token}: {}", capture.label())?;
                        writeln!(
                            output,
                            "{}",
                            inspection_text(&capture.binding.operation().description)
                        )?;
                        writeln!(
                            output,
                            "Read / Document / no selection / arguments {{}}. Uses exact in-memory source; no operation executed. Native Read is not an OS sandbox guarantee."
                        )?;
                        writeln!(
                            output,
                            "Authorize once: :scope inspect confirm {token}; cancel: :scope inspect cancel"
                        )?;
                        self.pending = Some((token, capture));
                    }
                    Err(error) => print_error(&error, output)?,
                }
            }
            ShellCommand::InspectConfirm(token) => {
                let Some((expected, _)) = &self.pending else {
                    return writeln!(
                        output,
                        "error: No pending inspection; use :scope inspect operation <id>"
                    );
                };
                if token != *expected {
                    return writeln!(
                        output,
                        "error: confirmation token mismatch; request not authorized"
                    );
                }
                let (_, mut capture) = self.pending.take().unwrap();
                capture.observe(app);
                let outcome = if capture.stale {
                    Err(InspectionError::SnapshotMismatch)
                } else {
                    // The explicit matching confirmation grants only this call. The
                    // bridge rechecks the complete descriptor, source and empty args.
                    capture.binding.invoke_app(app, |_| true)
                };
                capture.observe(app);
                if matches!(outcome, Err(InspectionError::SnapshotMismatch)) {
                    capture.stale = true;
                }
                self.result = Some(Record { capture, outcome });
                self.print_result(false, output)?;
            }
            ShellCommand::InspectCancel => {
                self.pending = None;
                writeln!(output, "Inspection cancelled; source unchanged.")?;
            }
            ShellCommand::InspectResult { full } => self.print_result(full, output)?,
            _ => unreachable!("only inspection commands are routed here"),
        }
        Ok(())
    }

    fn print_result(&self, full: bool, output: &mut impl Write) -> io::Result<()> {
        let Some(record) = &self.result else {
            return writeln!(
                output,
                "No inspection result; capture and confirm an operation first."
            );
        };
        writeln!(output, "Last inspection result: {}", record.capture.label())?;
        let feedback = match &record.outcome {
            Ok(result) => {
                let transport = tool_json(&result.result.value);
                let status = transport
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                writeln!(
                    output,
                    "Transport: ok; tool projection status: {}{}",
                    inspection_text(status),
                    if result.native_result.is_some() {
                        "; native read: read_only (authoritative observation)"
                    } else {
                        ""
                    }
                )?;
                json!({"transport": transport, "native": native_json(result)})
            }
            Err(error) => {
                writeln!(output, "Transport: error")?;
                json!({"error": error.to_string()})
            }
        };
        writeln!(
            output,
            "Invocation feedback, not a Scope validation certificate:"
        )?;
        if full {
            // JSON escapes C0 controls; additionally escape bidi/C1 controls without
            // truncating native values or routing them through the agent protocol.
            let text = serde_json::to_string_pretty(&feedback).expect("JSON values serialize");
            for line in text.lines() {
                writeln!(output, "{}", terminal_safe_json_line(line))?;
            }
        } else {
            let text = serde_json::to_string(&feedback).expect("JSON values serialize");
            writeln!(output, "{}", inspection_text_limit(&text, 240))?;
        }
        writeln!(
            output,
            "Retrieve last result: :scope inspect result --full (replaced by the next confirmed invocation)"
        )
    }
}

fn print_source(app: &App, output: &mut impl Write) -> io::Result<bool> {
    let Some(document) = app.open_document() else {
        writeln!(
            output,
            "error: no App document. Use bare open <path> with no active language session (first :scope session none if active). :scope open selects a separate shell buffer, not the inspection target."
        )?;
        return Ok(false);
    };
    let service = app.semantic_service_for_document(document);
    writeln!(
        output,
        "App document {:?} / path {} / version {} / language {} ({})",
        crate::source_edit::SourceId::Document(document.id()),
        inspection_text(&document.path().to_string()),
        document.version(),
        inspection_text(service.language_id()),
        inspection_text(service.language_name())
    )?;
    Ok(true)
}
fn print_error(error: &InspectionError, output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "error: {}", inspection_text(&error.to_string()))
}
fn terminal_safe_json_line(line: &str) -> String {
    let mut safe = String::new();
    for character in line.chars() {
        if character.is_control()
            || matches!(character, '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            // serde_json already escaped C0 controls. Remaining controls are BMP
            // code points inside JSON strings: keep them lossless and valid JSON.
            use std::fmt::Write as _;
            write!(safe, "\\u{:04x}", character as u32).expect("String writes cannot fail");
        } else {
            safe.push(character);
        }
    }
    safe
}
fn native_json(result: &InspectionResult) -> Value {
    match &result.native_result {
        Some(SemanticOperationResult::ReadOnly { value, rationale }) => {
            json!({"status":"read_only", "value":value,"rationale":rationale})
        }
        Some(SemanticOperationResult::ReadOnlyEvaluation {
            value,
            rationale,
            evaluation,
        }) => {
            let checks: Vec<_> = evaluation.checks.iter().map(|(id, check)| json!({"id":id,"outcome":format!("{:?}",check.outcome),"actual":check.actual})).collect();
            json!({"status":"read_only","value":value,"rationale":rationale,"evaluation":{"checks":checks}})
        }
        Some(SemanticOperationResult::Evaluated {
            value,
            rationale,
            evaluation,
        }) => {
            json!({"status":"read_only","value":value,"rationale":rationale,"evaluation":evaluation})
        }
        _ => Value::Null,
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
