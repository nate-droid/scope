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
    evidence: bool,
    candidate_only: bool,
}

impl SemanticInspection {
    pub(super) fn unrestricted(mut self) -> Self {
        self.candidate_only = false;
        self.selected = 0;
        self.pending = None;
        self
    }

    pub(super) fn candidate_available(app: &App) -> bool {
        Self::candidate_operation(app).is_ok()
    }

    fn candidate_operation(
        app: &App,
    ) -> Result<scope_language::SemanticOperationDescription, String> {
        let operations = semantic_inspection::discover_app(app).map_err(|e| e.to_string())?;
        let candidates: Vec<_> = operations
            .iter()
            .filter(|operation| {
                operation.purpose() == scope_language::SemanticOperationPurpose::CandidateEvaluation
            })
            .collect();
        let [operation] = candidates.as_slice() else {
            return Err("Run requires exactly one eligible zero-argument, read-only document CandidateEvaluation operation; use Evaluate / inspect to choose explicitly".into());
        };
        Ok((*operation).clone())
    }

    pub(super) fn candidate(app: &App) -> Result<Self, String> {
        let operation = Self::candidate_operation(app)?;
        Ok(Self {
            pending: Some(
                InspectionBinding::capture_app(app, &operation.id).map_err(|e| e.to_string())?,
            ),
            candidate_only: true,
            ..Self::default()
        })
    }

    fn operations(
        &self,
        app: &App,
    ) -> Result<
        Vec<scope_language::SemanticOperationDescription>,
        semantic_inspection::InspectionError,
    > {
        Ok(semantic_inspection::discover_app(app)?
            .into_iter()
            .filter(|operation| {
                !self.candidate_only
                    || operation.purpose()
                        == scope_language::SemanticOperationPurpose::CandidateEvaluation
            })
            .collect())
    }

    pub(super) fn result(&self) -> Option<&InspectionResult> {
        self.result.as_ref()
    }

    /// Consume only the captured request; repeated calls never capture or execute again.
    pub(super) fn run(&mut self, app: &mut App) {
        let Some(binding) = self.pending.take() else {
            return;
        };
        self.error = None;
        self.scroll = 0;
        // Direct Run and confirmed inspection grant exactly the same request.
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
    }

    pub(super) fn is_stale(&self, app: &App) -> bool {
        self.result
            .as_ref()
            .is_some_and(|result| !result.is_fresh_app(app))
    }

    /// Ordinary output never interprets opaque provider JSON; Details retains it.
    pub(super) fn concise_output(&self, app: &App) -> Option<String> {
        if let Some(error) = &self.error {
            let short: String = error
                .lines()
                .next()
                .unwrap_or("Run failed")
                .chars()
                .take(240)
                .collect();
            return Some(format!("Run failed: {short}"));
        }
        let result = self.result.as_ref()?;
        let summary = match &result.native_result {
            Some(SemanticOperationResult::Evaluated { evaluation, .. }) => {
                evaluation.summary.clone()
            }
            Some(SemanticOperationResult::ReadOnlyEvaluation { .. }) => {
                "Run completed with check results; inspect for details.".into()
            }
            Some(SemanticOperationResult::Blocked { blocker }) => {
                let mut lines = vec![format!("Run blocked: {}", blocker.explanation)];
                lines.extend(blocker.diagnostics.iter().map(|diagnostic| {
                    format!("{:?}: {}", diagnostic.severity, diagnostic.message)
                }));
                lines.join("\n")
            }
            Some(_) => "Run completed; inspect for results and diagnostics.".into(),
            None => {
                // Only the bridge envelope is protocol, never the provider's opaque value.
                let envelope = tool_json(&result.result.value);
                let status = envelope
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let mut lines = vec![format!("Run: {status}")];
                if let Some(message) = envelope.get("message").and_then(Value::as_str) {
                    lines.push(message.into());
                }
                if let Some(diagnostics) = envelope.get("diagnostics").and_then(Value::as_array) {
                    for diagnostic in diagnostics {
                        if let Some(message) = diagnostic.get("message").and_then(Value::as_str) {
                            let severity = diagnostic
                                .get("severity")
                                .and_then(Value::as_str)
                                .unwrap_or("Diagnostic");
                            lines.push(format!("{severity}: {message}"));
                        }
                    }
                }
                lines.join("\n")
            }
        };
        Some(if self.is_stale(app) {
            format!("STALE (source changed or no longer open)\n{summary}")
        } else {
            summary
        })
    }

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
                let count = self.operations(app).map_or(0, |ops| ops.len());
                self.selected = if key.code == KeyCode::Up {
                    self.selected.saturating_sub(1)
                } else {
                    self.selected.saturating_add(1).min(count.saturating_sub(1))
                };
                self.scroll = 0;
            }
            KeyCode::Char('d' | 'e') if key.modifiers.is_empty() => {
                self.evidence = !self.evidence;
                self.scroll = 0;
            }
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::Home => self.scroll = 0,
            KeyCode::Enter => {
                self.error = None;
                self.scroll = 0;
                if self.pending.is_some() {
                    self.run(app);
                } else {
                    match self.operations(app).and_then(|ops| {
                        if self.candidate_only && ops.len() != 1 {
                            return Err(semantic_inspection::InspectionError::NotInspectable(
                                "Run requires exactly one candidate evaluation".into(),
                            ));
                        }
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
        let mut text = String::from(if self.candidate_only {
            "Run — Enter review/confirm • PgUp/PgDn scroll • Esc cancel/close\nRuns in memory; does not save. Read is not an OS sandbox.\n\n"
        } else {
            "Read semantic operations — ↑/↓ select • Enter review/confirm • PgUp/PgDn scroll • Esc cancel/close\nDiscovery is not permission. Read is not an OS sandbox.\n\n"
        });
        if (self.result.is_none() || self.evidence)
            && let Some(document) = app.open_document()
        {
            text.push_str(&format!(
                "Document: {} — revision {}\n",
                document.path().display(),
                document.version()
            ));
        }
        if !self.candidate_only && (self.result.is_none() || self.evidence) {
            match self.operations(app) {
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
        }
        if let Some(binding) = &self.pending {
            let question = if self.candidate_only {
                "Run current program?"
            } else {
                "Inspect current document?"
            };
            text.push_str(&format!("\n{question}\n{} — revision {}\nEnter authorizes exactly this snapshot; Esc cancels.\nSource preview (captured):\n", binding.operation().description, binding.version()));
            for line in binding.text().lines().take(4) {
                text.push_str(&line.chars().take(120).collect::<String>());
                text.push('\n');
            }
            if binding.text().lines().count() > 4
                || binding
                    .text()
                    .lines()
                    .any(|line| line.chars().count() > 120)
            {
                text.push_str("… source preview truncated\n");
            }
            if self.evidence {
                text.push_str(&format!(
                    "Identity: {} / {} / {:?} / arguments {{}}\n",
                    binding.language(),
                    binding.operation().id,
                    binding.source_id()
                ));
            }
            if !binding.is_fresh_app(app) {
                text.push_str("STALE confirmation — invocation will be rejected.\n");
            }
        }
        if let Some(error) = &self.error {
            text.push_str(&format!("\nInvocation error: {error}\n"));
        }
        if let Some(result) = &self.result {
            text.push_str(&format!(
                "\n{} — {} — observed revision {}\n",
                if result.is_fresh_app(app) {
                    "CURRENT"
                } else {
                    "STALE (source changed or no longer open)"
                },
                result.binding.operation().description,
                result.binding.version()
            ));
            if let Some(SemanticOperationResult::Evaluated { evaluation, .. }) =
                &result.native_result
            {
                let summary: String = evaluation.summary.chars().take(240).collect();
                text.push_str(&format!("\nOutcome: {:?}\n{summary}\n", evaluation.outcome));
                if evaluation.summary.chars().count() > 240 {
                    text.push_str("… summary truncated; press d for Details.\n");
                }
            }
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
            text.push_str("d toggles Details (full retained evidence) • PgUp/PgDn scroll\n");
            let pretty = serde_json::to_string_pretty(&value).expect("JSON values serialize");
            if self.evidence {
                text.push_str(&format!(
                    "Identity: {} / {} / {:?}\n",
                    result.binding.language(),
                    result.binding.operation().id,
                    result.binding.source_id()
                ));
                text.push_str(&pretty);
            } else if !matches!(
                result.native_result,
                Some(SemanticOperationResult::Evaluated { .. })
            ) {
                for line in pretty.lines().take(12) {
                    let bounded: String = line.chars().take(160).collect();
                    text.push_str(&bounded);
                    text.push('\n');
                }
                if pretty.lines().count() > 12
                    || pretty.lines().any(|line| line.chars().count() > 160)
                {
                    text.push_str("… preview truncated; press d for Details.\n");
                }
            }
        }
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(if self.candidate_only {
                    "Run"
                } else {
                    "Semantic inspection — F8"
                }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use scope_language::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Service(AtomicUsize);
    impl LanguageService for Service {
        fn language_id(&self) -> &'static str {
            "inspection-fixture"
        }
        fn language_name(&self) -> &'static str {
            "Inspection fixture"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            true
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            LanguageAnalysis::default()
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            vec![
                SemanticOperationDescription {
                    id: "evaluate".into(),
                    description: "Evaluate fixture".into(),
                    arguments: vec![],
                    effect: SemanticOperationEffect::Read,
                    target: SemanticTargetRequirement::Document,
                    constraints: None,
                }
                .with_purpose(SemanticOperationPurpose::CandidateEvaluation)
                .unwrap(),
            ]
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.document.text(), "unsaved λ");
            Ok(SemanticOperationResult::ReadOnly {
                value: json!({"observed": request.document.text(), "evidence": "x".repeat(2000)}),
                rationale: "fixture evidence".into(),
            })
        }
    }
    struct FailureService(&'static str);
    impl LanguageService for FailureService {
        fn language_id(&self) -> &'static str {
            "failure-fixture"
        }
        fn language_name(&self) -> &'static str {
            "Failure fixture"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            true
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            LanguageAnalysis::default()
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            Service(AtomicUsize::new(0)).semantic_operations()
        }
        fn perform_semantic_operation(
            &self,
            _: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            let diagnostic = Diagnostic::error(
                TextRange {
                    start: Position { line: 0, column: 0 },
                    end: Position { line: 0, column: 1 },
                },
                "static type mismatch",
            );
            match self.0 {
                "static" => Err(SemanticOperationError::InvalidSource(vec![diagnostic])),
                "runtime" => Err(SemanticOperationError::InternalFailure(
                    "runtime division by zero".into(),
                )),
                "blocked" => Ok(SemanticOperationResult::Blocked {
                    blocker: SemanticBlocker {
                        operation_id: "evaluate".into(),
                        category: BlockerCategory::UnsatisfiedPrecondition,
                        explanation: "execution unavailable".into(),
                        recoverable: false,
                        missing_dependencies: vec![],
                        diagnostics: vec![diagnostic],
                        suggested_next_action: None,
                    },
                }),
                _ => panic!("cancelled inspection must not invoke provider"),
            }
        }
    }

    #[test]
    fn concise_output_reports_real_bridge_failures_and_typed_blockers() {
        for (mode, expected) in [
            ("static", "Error: static type mismatch"),
            ("runtime", "runtime division by zero"),
            ("blocked", "Run blocked: execution unavailable"),
        ] {
            let root = crate::test_support::TempWorkspace::new("run-failure-output");
            root.write_file("sample.fixture", "source");
            let service = Box::leak(Box::new(FailureService(mode)));
            let mut app = App::builder(root.workspace())
                .language(service)
                .build()
                .unwrap();
            app.open_path("sample.fixture").unwrap();
            let mut inspection = SemanticInspection::candidate(&app).unwrap();
            inspection.run(&mut app);
            let output = inspection.concise_output(&app).unwrap();
            assert!(output.contains(expected), "{output}");
            assert!(!output.contains("completed"), "{output}");
            if mode != "blocked" {
                assert!(inspection.result().unwrap().native_result.is_none());
                assert!(output.contains(if mode == "static" {
                    "invalid_source"
                } else {
                    "internal_failure"
                }));
            }
            assert!(!inspection.is_stale(&app));
        }
    }

    #[test]
    fn cancelled_run_has_no_invented_output_or_result() {
        let root = crate::test_support::TempWorkspace::new("run-cancelled-output");
        root.write_file("sample.fixture", "source");
        let service = Box::leak(Box::new(FailureService("cancelled")));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        app.open_path("sample.fixture").unwrap();
        let mut inspection = SemanticInspection::candidate(&app).unwrap();
        inspection.handle_key(
            KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE),
            &mut app,
        );
        inspection.run(&mut app);
        assert!(inspection.result().is_none());
        assert!(inspection.concise_output(&app).is_none());
    }

    struct Catalog(Vec<SemanticOperationDescription>);
    impl LanguageService for Catalog {
        fn language_id(&self) -> &'static str {
            "catalog"
        }
        fn language_name(&self) -> &'static str {
            "Catalog"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            true
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            LanguageAnalysis::default()
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            self.0.clone()
        }
    }

    #[test]
    fn primary_run_rejects_missing_ambiguous_and_ineligible_candidates() {
        let base = Service(AtomicUsize::new(0)).semantic_operations().remove(0);
        let exploration = base
            .clone()
            .with_purpose(SemanticOperationPurpose::Exploration)
            .unwrap();
        let mut with_arguments = base.clone();
        // Even an optional argument is not a zero-argument operation.
        with_arguments.arguments.push(SemanticArgument {
            name: "input".into(),
            description: "optional input".into(),
            required: false,
            kind: SemanticArgumentKind::String,
        });
        let mut second = base.clone();
        second.id = "second".into();
        for operations in [
            vec![],
            vec![exploration],
            vec![base.clone(), second],
            vec![with_arguments],
        ] {
            let root = crate::test_support::TempWorkspace::new("primary-run-catalog");
            root.write_file("sample.fixture", "source");
            let service = Box::leak(Box::new(Catalog(operations)));
            let mut app = App::builder(root.workspace())
                .language(service)
                .build()
                .unwrap();
            app.open_path("sample.fixture").unwrap();
            assert!(SemanticInspection::candidate(&app).is_err());
            assert_eq!(app.open_document().unwrap().text(), "source");
        }
    }

    #[test]
    fn direct_run_consumes_exact_snapshot_once_and_reports_staleness() {
        let root = crate::test_support::TempWorkspace::new("direct-run");
        root.write_file("sample.fixture", "saved");
        let service = Box::leak(Box::new(Service(AtomicUsize::new(0))));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        app.open_path("sample.fixture").unwrap();
        let replace = |app: &mut App, text: &str| {
            let doc = app.open_document_mut().unwrap();
            let range = doc
                .buffer()
                .range_for_span(crate::text::Span::new(0, doc.text().len()))
                .unwrap();
            doc.apply_edit(crate::text::TextEdit::replace(range, text))
                .unwrap();
        };
        replace(&mut app, "unsaved λ");
        let mut inspection = SemanticInspection::candidate(&app).unwrap();
        assert!(inspection.concise_output(&app).is_none());
        assert_eq!(service.0.load(Ordering::SeqCst), 0);
        inspection.run(&mut app);
        assert!(inspection.pending.is_none());
        assert!(inspection.result().unwrap().is_fresh_app(&app));
        assert!(!inspection.is_stale(&app));
        assert_eq!(
            inspection.concise_output(&app).unwrap(),
            "Run completed; inspect for results and diagnostics."
        );
        inspection.run(&mut app);
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
        assert_eq!(root.read_file("sample.fixture"), "saved");
        let mut stale = SemanticInspection::candidate(&app).unwrap();
        replace(&mut app, "changed");
        assert!(inspection.is_stale(&app));
        assert!(
            inspection
                .concise_output(&app)
                .unwrap()
                .starts_with("STALE")
        );
        stale.run(&mut app);
        assert!(stale.pending.is_none());
        assert!(stale.result().is_none());
        assert!(
            stale
                .concise_output(&app)
                .unwrap()
                .starts_with("Run failed:")
        );
        stale.run(&mut app);
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn primary_run_captures_without_execution_and_requires_one_shot_confirmation() {
        let root = crate::test_support::TempWorkspace::new("primary-run-modal");
        root.write_file("sample.fixture", "unsaved λ");
        let service = Box::leak(Box::new(Service(AtomicUsize::new(0))));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        app.open_path("sample.fixture").unwrap();
        let mut modal = SemanticInspection::candidate(&app).unwrap();
        assert_eq!(service.0.load(Ordering::SeqCst), 0);
        assert_eq!(modal.pending.as_ref().unwrap().text(), "unsaved λ");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 25)).unwrap();
        terminal
            .draw(|frame| modal.render(frame, frame.area(), &app))
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("Run current program?"));
        assert!(!screen.contains("Read semantic operations"));
        assert_eq!(service.0.load(Ordering::SeqCst), 0);
        modal.handle_key(
            KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
            &mut app,
        );
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
        assert!(modal.pending.is_none());
        modal.handle_key(
            KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
            &mut app,
        );
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
        assert!(modal.pending.is_some());
        modal.handle_key(
            KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE),
            &mut app,
        );
        assert!(modal.pending.is_none());
        assert_eq!(root.read_file("sample.fixture"), "unsaved λ");
    }

    #[test]
    fn modal_confirms_exact_unsaved_app_snapshot_and_rejects_stale_confirmation() {
        let root = crate::test_support::TempWorkspace::new("semantic-modal");
        root.write_file("sample.fixture", "saved");
        let service = Box::leak(Box::new(Service(AtomicUsize::new(0))));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        app.open_path("sample.fixture").unwrap();
        let replace = |app: &mut App, text: &str| {
            let doc = app.open_document_mut().unwrap();
            let range = doc
                .buffer()
                .range_for_span(crate::text::Span::new(0, doc.text().len()))
                .unwrap();
            doc.apply_edit(crate::text::TextEdit::replace(range, text))
                .unwrap();
        };
        replace(&mut app, "unsaved λ");
        let mut modal = SemanticInspection::default();
        let key = |code| KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        modal.handle_key(key(KeyCode::Enter), &mut app);
        assert!(modal.pending.is_some());
        let screen = |modal: &SemanticInspection, app: &App| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(150, 60)).unwrap();
            terminal
                .draw(|frame| modal.render(frame, frame.area(), app))
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let confirmation = screen(&modal, &app);
        assert!(confirmation.contains("sample.fixture"));
        assert!(confirmation.contains("revision 1"));
        assert!(confirmation.contains("unsaved λ"));
        assert!(!confirmation.contains("Identity:"));
        assert_eq!(service.0.load(Ordering::SeqCst), 0);
        modal.handle_key(key(KeyCode::Enter), &mut app);
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
        assert!(modal.result.as_ref().unwrap().is_fresh_app(&app));

        assert!(app.open_document().unwrap().is_dirty());
        assert_eq!(
            std::fs::read_to_string(
                std::path::Path::new(root.workspace().root()).join("sample.fixture")
            )
            .unwrap(),
            "saved"
        );
        modal.result.as_mut().unwrap().native_result = Some(SemanticOperationResult::Evaluated {
            value: json!({"evidence_marker": "unchanged", "unsigned": u64::MAX, "fraction": 1.25}),
            rationale: "runtime observation only".into(),
            evaluation: SemanticEvaluation {
                outcome: SemanticOutcome::Inconclusive,
                summary: format!(
                    "Evaluation complete\nmain() = 16\n{}summary_tail",
                    "λ".repeat(250)
                ),
                diagnostics: None,
                failed_criteria: vec![],
                contract_id: None,
            },
        });
        assert_eq!(
            modal.concise_output(&app).unwrap(),
            format!(
                "Evaluation complete\nmain() = 16\n{}summary_tail",
                "λ".repeat(250)
            )
        );
        assert!(!modal.is_stale(&app));
        let normal = screen(&modal, &app);
        assert!(normal.contains("Evaluation complete"));
        assert!(normal.contains("main() = 16"));
        assert!(normal.contains("Outcome: Inconclusive"));
        assert!(normal.contains("CURRENT"));
        assert!(normal.contains("observed revision 1"));
        assert!(normal.contains("d toggles Details"));
        assert!(!normal.contains("evidence_marker"));
        assert!(!normal.contains("Identity:"));
        assert!(!normal.contains("Document:"));
        modal.handle_key(
            KeyEvent::new(KeyCode::Char('d'), crossterm::event::KeyModifiers::CONTROL),
            &mut app,
        );
        assert!(!modal.evidence);
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
        assert!(!normal.contains("summary_tail"));
        assert!(normal.contains("summary truncated"));
        modal.scroll = 10;
        modal.handle_key(key(KeyCode::Char('d')), &mut app);
        assert!(modal.evidence);
        assert_eq!(modal.scroll, 0);
        let evidence = screen(&modal, &app);
        assert!(evidence.contains("Inconclusive"));
        assert!(evidence.contains("evidence_marker"));
        assert!(evidence.contains("summary_tail"));
        assert!(evidence.contains("18446744073709551615"));
        assert!(evidence.contains("1.25"));
        assert!(evidence.contains("Identity:"));
        assert!(evidence.contains("Document:"));
        assert!(evidence.contains("runtime observation only"));
        modal.handle_key(key(KeyCode::Char('d')), &mut app);
        assert!(!modal.evidence);
        assert!(!screen(&modal, &app).contains("evidence_marker"));
        let mut workspace = crate::tui::WorkspaceTui::new();
        let mut sources = crate::scratch::ScratchBuffers::default();
        sources.create("inspection-fixture", "unrelated scratch");
        workspace
            .invoke_action(
                crate::tui::WorkspaceAction::InspectDocument,
                &mut app,
                &mut sources,
                None,
            )
            .unwrap();
        workspace.handle_key(key(KeyCode::Enter), &mut app);
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
        workspace.handle_key(key(KeyCode::Enter), &mut app);
        assert_eq!(service.0.load(Ordering::SeqCst), 2);
        workspace.handle_key(key(KeyCode::Esc), &mut app);
        assert!(workspace.inspection_result().unwrap().is_fresh_app(&app));
        workspace
            .invoke_action(
                crate::tui::WorkspaceAction::InspectDocument,
                &mut app,
                &mut sources,
                None,
            )
            .unwrap();
        assert!(
            workspace
                .inspection_result()
                .unwrap()
                .native_result
                .is_some()
        );
        modal.handle_key(key(KeyCode::Enter), &mut app);
        replace(&mut app, "changed");
        assert!(!workspace.inspection_result().unwrap().is_fresh_app(&app));
        let stale = screen(&modal, &app);
        assert!(stale.contains("STALE (source changed or no longer open)"));
        assert!(stale.contains("Outcome: Inconclusive"));
        assert!(stale.contains("observed revision 1"));
        modal.handle_key(key(KeyCode::Enter), &mut app);
        assert!(modal.error.is_some());
        assert_eq!(service.0.load(Ordering::SeqCst), 2);
        modal.handle_key(key(KeyCode::Enter), &mut app);
        assert!(!modal.handle_key(key(KeyCode::Esc), &mut app));
        assert!(modal.pending.is_none());
        assert!(modal.handle_key(key(KeyCode::Esc), &mut app));
    }
}
