//! Generic, synchronous read inspection. Opening this modal never executes a provider.
use super::TrustedClosureProvider;
use crate::{
    app::AppState,
    semantic_inspection::{self, InspectionBinding, InspectionFreshness, InspectionResult},
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
    baseline: Option<InspectionResult>,
    comparison: Option<super::result_comparison::Comparison>,
    comparison_side: Option<bool>,
    error: Option<String>,
    scroll: u16,
    evidence: bool,
    candidate_only: bool,
    explorer: Option<super::result_explorer::ResultExplorer>,
}

impl SemanticInspection {
    pub(super) fn unrestricted(mut self) -> Self {
        self.candidate_only = false;
        self.selected = 0;
        self.pending = None;
        self.comparison = None;
        self.comparison_side = None;
        self
    }

    pub(super) fn candidate_available<P: TrustedClosureProvider>(app: &AppState<P>) -> bool {
        Self::candidate_operation(app).is_ok()
    }

    fn candidate_operation<P: TrustedClosureProvider>(
        app: &AppState<P>,
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

    pub(super) fn candidate<P: TrustedClosureProvider>(app: &AppState<P>) -> Result<Self, String> {
        let operation = Self::candidate_operation(app)?;
        Ok(Self {
            pending: Some(
                InspectionBinding::capture_app(app, &operation.id).map_err(|e| e.to_string())?,
            ),
            candidate_only: true,
            ..Self::default()
        })
    }

    fn operations<P: TrustedClosureProvider>(
        &self,
        app: &AppState<P>,
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
    pub(super) fn run<P: TrustedClosureProvider>(&mut self, app: &mut AppState<P>) {
        let Some(binding) = self.pending.take() else {
            return;
        };
        self.error = None;
        self.scroll = 0;
        self.explorer = None;
        // Direct Run and confirmed inspection grant exactly the same request.
        match binding.invoke_app(app, |request| {
            request.operation == binding.operation()
                && request.source.id == binding.source_id()
                && request.source.version == binding.version()
                && request.source.text == binding.text()
                && request.source.language == binding.language()
                && request.arguments == &json!({})
        }) {
            Ok(result) => {
                self.retain_completed(result);
            }
            Err(error) => {
                self.error = Some(error.to_string());
            }
        }
    }

    /// Accept completed evidence only; never captures, authorizes, or executes.
    pub(super) fn retain_completed(&mut self, result: InspectionResult) {
        self.baseline = self.result.replace(result);
        self.pending = None;
        self.comparison = None;
        self.comparison_side = None;
        self.explorer = None;
        self.error = None;
        self.scroll = 0;
    }

    fn currentness<P: TrustedClosureProvider>(
        result: &InspectionResult,
        app: &AppState<P>,
        draft: bool,
    ) -> &'static str {
        match result.freshness_app(app) {
            InspectionFreshness::Unknown => "UNKNOWN (closure freshness not established)",
            InspectionFreshness::Stale if result.document_binding().is_some() => {
                "STALE (source changed or no longer open)"
            }
            InspectionFreshness::Stale => "STALE (registered closure evidence changed)",
            InspectionFreshness::Current if draft => "STALE (unpublished editor draft)",
            InspectionFreshness::Current => "CURRENT",
        }
    }

    fn provenance(result: &InspectionResult) -> String {
        match &result.origin {
            semantic_inspection::InspectionOrigin::Document(binding) => format!(
                "observed revision {}\nIdentity: {} / {} / {:?}",
                binding.version(),
                binding.language(),
                binding.operation().id,
                binding.source_id()
            ),
            #[cfg(feature = "experimental-sprout-closure")]
            semantic_inspection::InspectionOrigin::Closure(observation) => format!(
                "Registered closure observation\nBinding: {:?}\nService: {:?}",
                observation.binding(),
                observation.service()
            ),
        }
    }

    pub(super) fn inherit_completed(&mut self, previous: Self) {
        self.baseline = previous.baseline;
        self.result = previous.result;
    }

    pub(super) fn comparison_disabled(&self) -> Option<&'static str> {
        let (Some(baseline), Some(latest)) = (&self.baseline, &self.result) else {
            return Some("Compare requires two completed inspection results");
        };
        if Self::native_value(baseline).is_none() || Self::native_value(latest).is_none() {
            return Some(
                "Compare unsupported: both results require retained native JSON; use Details",
            );
        }
        None
    }

    pub(super) fn open_comparison(&mut self) {
        if self.comparison_disabled().is_some() {
            return;
        }
        self.comparison = Some(super::result_comparison::Comparison::new(
            Self::native_value(self.baseline.as_ref().unwrap()).unwrap(),
            Self::native_value(self.result.as_ref().unwrap()).unwrap(),
        ));
        self.comparison_side = None;
        self.pending = None;
        self.explorer = None;
        self.scroll = 0;
    }

    fn displayed_result(&self) -> Option<&InspectionResult> {
        if self.comparison_side == Some(false) {
            self.baseline.as_ref()
        } else {
            self.result.as_ref()
        }
    }

    pub(super) fn is_stale<P: TrustedClosureProvider>(&self, app: &AppState<P>) -> bool {
        self.displayed_result()
            .is_some_and(|result| result.freshness_app(app) == InspectionFreshness::Stale)
    }

    /// Ordinary output never interprets opaque provider JSON; Details retains it.
    pub(super) fn concise_output<P: TrustedClosureProvider>(
        &self,
        app: &AppState<P>,
    ) -> Option<String> {
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
        let result = self.displayed_result()?;
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
        let state = Self::currentness(result, app, false);
        Some(if state == "CURRENT" {
            summary
        } else {
            format!("{state}\n{summary}")
        })
    }

    pub(super) fn open_explorer(&mut self) {
        if self.result.is_none() {
            return;
        }
        self.pending = None;
        self.comparison = None;
        self.comparison_side = None;
        self.explorer = Some(Default::default());
        self.scroll = 0;
    }

    pub(super) fn open_details(&mut self) {
        self.comparison = None;
        self.comparison_side = None;
        self.pending = None;
        self.explorer = None;
        self.evidence = true;
        self.scroll = 0;
    }

    /// Returns true when the modal should close.
    pub fn handle_key<P: TrustedClosureProvider>(
        &mut self,
        key: KeyEvent,
        app: &mut AppState<P>,
    ) -> bool {
        if self.comparison.is_some() && self.comparison_side.is_none() {
            match key.code {
                KeyCode::Esc => self.comparison = None,
                KeyCode::Char('1' | '2') => {
                    self.comparison_side = Some(key.code == KeyCode::Char('2'));
                    self.explorer = Some(Default::default());
                    self.evidence = true;
                    self.scroll = 0;
                }
                code => self.comparison.as_mut().unwrap().key(code),
            }
            return false;
        }
        if self.comparison_side.is_some() && self.explorer.is_none() {
            match key.code {
                KeyCode::Esc => self.comparison_side = None,
                KeyCode::Char('x') => self.explorer = Some(Default::default()),
                KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
                KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
                KeyCode::Home => self.scroll = 0,
                _ => {}
            }
            return false;
        }
        if self.explorer.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('x') => {
                    self.explorer = None;
                    if self.comparison.is_some() {
                        self.comparison_side = None;
                    }
                }
                KeyCode::Char('d') => {
                    self.explorer = None;
                    self.evidence = true;
                    self.scroll = 0;
                }
                code => {
                    let mut explorer = self.explorer.take().expect("explorer open");
                    if let Some(value) = self.structured_value() {
                        explorer.key(code, value);
                    }
                    self.explorer = Some(explorer);
                }
            }
            return false;
        }
        match key.code {
            KeyCode::Char('x') if self.result.is_some() && self.pending.is_none() => {
                self.open_explorer();
            }
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

    fn structured_value(&self) -> Option<&Value> {
        Self::native_value(self.displayed_result()?)
    }

    fn native_value(result: &InspectionResult) -> Option<&Value> {
        match result.native_result.as_ref()? {
            SemanticOperationResult::ReadOnly { value, .. }
            | SemanticOperationResult::ReadOnlyEvaluation { value, .. }
            | SemanticOperationResult::Evaluated { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn render<P: TrustedClosureProvider>(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        app: &AppState<P>,
        unpublished_draft: bool,
    ) {
        let area = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        if let Some(comparison) = &self.comparison
            && self.comparison_side.is_none()
        {
            let mut text = String::from(
                "Structural comparison only; different origins are not semantic equivalence.\n1 inspect baseline • 2 inspect latest • Esc back • Enter is passive\n",
            );
            for (label, result) in [
                ("Baseline", self.baseline.as_ref().unwrap()),
                ("Latest", self.result.as_ref().unwrap()),
            ] {
                let state = Self::currentness(result, app, unpublished_draft);
                let bounded = |s: String| -> String {
                    s.chars().take(160).flat_map(char::escape_default).collect()
                };
                let outcome = match &result.native_result {
                    Some(SemanticOperationResult::Evaluated { evaluation, .. }) => {
                        format!("{:?}", evaluation.outcome)
                    }
                    Some(SemanticOperationResult::ReadOnlyEvaluation { .. }) => {
                        "read-only evaluation".into()
                    }
                    _ => "read-only completed".into(),
                };
                text.push_str(&format!("{label}: {state} — {}\nOutcome: {} • Retained invocation feedback (not a validation certificate).\n",
                    Self::provenance(result), bounded(outcome)));
            }
            // Measure the fixed provenance and comparison lines, excluding the
            // single row in this probe, so the footer fits inside both borders.
            let fixed_lines =
                text.lines().count() + comparison.text(1).lines().count().saturating_sub(1);
            let row_budget = usize::from(area.height.saturating_sub(2)).saturating_sub(fixed_lines);
            text.push_str(&comparison.text(row_budget));
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(text).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Compare Results"),
                ),
                area,
            );
            return;
        }
        let currentness = self.displayed_result().map_or("CURRENT", |result| {
            Self::currentness(result, app, unpublished_draft)
        });
        if let Some(explorer) = &self.explorer {
            let result = self
                .displayed_result()
                .expect("explorer requires retained result");
            let mut text = format!(
                "{} — {}\nRetained invocation feedback; observation grants no authority.\n",
                currentness,
                Self::provenance(result)
            );
            if let Some(SemanticOperationResult::Evaluated { evaluation, .. }) =
                &result.native_result
            {
                text.push_str(&format!("Outcome: {:?}\n", evaluation.outcome));
            }
            if !matches!(
                result.native_result,
                Some(SemanticOperationResult::Evaluated { .. })
            ) {
                let feedback = self.concise_output(app).unwrap_or_default();
                text.push_str(&format!(
                    "Feedback: {}\n",
                    feedback.chars().take(240).collect::<String>()
                ));
            }
            match self.structured_value() {
                Some(value) => text.push_str(&explorer.text(value, area.height.saturating_sub(11) as usize)),
                None => text.push_str("Unsupported result: no native JSON value retained. Provider text is opaque.\nEsc back • d Details\n"),
            }
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(text).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Result Explorer"),
                ),
                area,
            );
            return;
        }
        let mut text = String::from(if self.comparison_side.is_some() {
            "Comparison side — retained Details • x Explorer • PgUp/PgDn scroll • Esc comparison\nRead-only evidence; Enter is passive.\n\n"
        } else if self.candidate_only {
            "Run — Enter review/confirm • PgUp/PgDn scroll • Esc cancel/close\nRuns in memory; does not save. Read is not an OS sandbox.\n\n"
        } else {
            "Read semantic operations — ↑/↓ select • Enter review/confirm • PgUp/PgDn scroll • Esc cancel/close\nDiscovery is not permission. Read is not an OS sandbox.\n\n"
        });
        if self.comparison_side.is_none()
            && (self.result.is_none() || self.evidence)
            && let Some(document) = app.open_document()
        {
            text.push_str(&format!(
                "Document: {} — revision {}\n",
                document.path().display(),
                document.version()
            ));
        }
        if self.comparison_side.is_none()
            && !self.candidate_only
            && (self.result.is_none() || self.evidence)
        {
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
        if let Some(result) = self.displayed_result() {
            let heading = if let Some(binding) = result.document_binding() {
                format!(
                    "{} — observed revision {}",
                    binding.operation().description,
                    binding.version()
                )
            } else {
                Self::provenance(result)
            };
            text.push_str(&format!("\n{currentness} — {heading}\n"));
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
            text.push_str("d toggles Details (full retained evidence) • x Result Explorer • PgUp/PgDn scroll\n");
            let pretty = serde_json::to_string_pretty(&value).expect("JSON values serialize");
            if self.evidence {
                text.push_str(&format!("{}\n", Self::provenance(result)));
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
    use crate::app::App;
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

    fn explorer_screen(modal: &SemanticInspection, app: &App, unpublished_draft: bool) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(180, 60)).unwrap();
        terminal
            .draw(|frame| modal.render(frame, frame.area(), app, unpublished_draft))
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
    }

    #[test]
    fn explorer_navigation_is_read_only_and_retains_stale_provenance() {
        let root = crate::test_support::TempWorkspace::new("explorer-read-only");
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
        let mut modal = SemanticInspection::candidate(&app).unwrap();
        modal.run(&mut app);
        let key = |code| KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        modal.handle_key(key(KeyCode::Char('d')), &mut app);
        let details = explorer_screen(&modal, &app, false);
        let result = modal.result().unwrap();
        let identity = format!(
            "Identity: {} / {} / {:?}",
            result.document_binding().unwrap().language(),
            result.document_binding().unwrap().operation().id,
            result.document_binding().unwrap().source_id()
        );
        let revision = format!(
            "observed revision {}",
            result.document_binding().unwrap().version()
        );
        let captured_text = result.document_binding().unwrap().text().to_owned();
        let native = format!("{:?}", result.native_result);
        let feedback = tool_json(&result.result.value);
        let version = app.open_document().unwrap().version();
        assert!(details.contains(&identity));
        modal.handle_key(key(KeyCode::Char('x')), &mut app);
        assert!(modal.explorer.is_some());
        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Enter,
            KeyCode::Enter,
        ] {
            assert!(!modal.handle_key(key(code), &mut app));
            assert!(modal.explorer.is_some());
            assert!(modal.pending.is_none());
            assert_eq!(service.0.load(Ordering::SeqCst), 1);
            assert_eq!(app.open_document().unwrap().text(), "unsaved λ");
            assert_eq!(app.open_document().unwrap().version(), version);
            assert!(app.open_document().unwrap().is_dirty());
            assert_eq!(root.read_file("sample.fixture"), "saved");
            let result = modal.result().unwrap();
            assert_eq!(format!("{:?}", result.native_result), native);
            assert_eq!(tool_json(&result.result.value), feedback);
            assert_eq!(result.document_binding().unwrap().text(), captured_text);
            let screen = explorer_screen(&modal, &app, false);
            assert!(screen.contains(&identity), "{screen}");
            assert!(screen.contains(&revision), "{screen}");
        }
        assert!(!modal.handle_key(key(KeyCode::Esc), &mut app));
        assert!(modal.explorer.is_none());
        assert!(modal.evidence);
        assert_eq!(explorer_screen(&modal, &app, false), details);
        modal.handle_key(key(KeyCode::Char('x')), &mut app);
        assert!(!modal.is_stale(&app));
        let unpublished = explorer_screen(&modal, &app, true);
        assert!(unpublished.contains("STALE (unpublished editor draft)"));
        assert!(unpublished.contains(&identity));
        assert!(unpublished.contains(&revision));
        assert!(!explorer_screen(&modal, &app, false).contains("STALE"));
        replace(&mut app, "changed");
        let stale = explorer_screen(&modal, &app, false);
        assert!(stale.contains("STALE (source changed or no longer open)"));
        assert!(stale.contains(&identity));
        assert!(stale.contains(&revision));
        assert_eq!(
            modal.result().unwrap().document_binding().unwrap().text(),
            captured_text
        );
        assert_eq!(
            format!("{:?}", modal.result().unwrap().native_result),
            native
        );
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
        assert_eq!(root.read_file("sample.fixture"), "saved");
    }

    #[test]
    fn explorer_unsupported_results_preserve_actual_failure_details_without_parsing() {
        for (mode, expected) in [
            ("static", "invalid_source"),
            ("runtime", "internal_failure"),
            ("blocked", "execution unavailable"),
        ] {
            let root = crate::test_support::TempWorkspace::new("explorer-unsupported");
            root.write_file("sample.fixture", "source");
            let service = Box::leak(Box::new(FailureService(mode)));
            let mut app = App::builder(root.workspace())
                .language(service)
                .build()
                .unwrap();
            app.open_path("sample.fixture").unwrap();
            let mut modal = SemanticInspection::candidate(&app).unwrap();
            modal.run(&mut app);
            let key = |code| KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
            let native = format!("{:?}", modal.result().unwrap().native_result);
            let feedback = tool_json(&modal.result().unwrap().result.value);
            if mode == "blocked" {
                assert!(matches!(
                    modal.result().unwrap().native_result,
                    Some(SemanticOperationResult::Blocked { .. })
                ));
            } else {
                assert!(modal.result().unwrap().native_result.is_none());
            }
            assert!(modal.structured_value().is_none());
            modal.handle_key(key(KeyCode::Char('d')), &mut app);
            let details = explorer_screen(&modal, &app, false);
            assert!(details.contains(expected), "{details}");
            modal.handle_key(key(KeyCode::Char('x')), &mut app);
            for code in [KeyCode::Right, KeyCode::Down, KeyCode::Enter] {
                assert!(!modal.handle_key(key(code), &mut app));
            }
            let unsupported = explorer_screen(&modal, &app, false);
            assert!(unsupported.contains("Unsupported result: no native JSON value retained"));
            assert!(unsupported.contains("Provider text is opaque"));
            assert!(modal.pending.is_none());
            assert!(modal.structured_value().is_none());
            assert_eq!(
                format!("{:?}", modal.result().unwrap().native_result),
                native
            );
            assert_eq!(tool_json(&modal.result().unwrap().result.value), feedback);
            assert!(!modal.handle_key(key(KeyCode::Esc), &mut app));
            assert!(modal.evidence);
            assert_eq!(explorer_screen(&modal, &app, false), details);
            assert_eq!(app.open_document().unwrap().text(), "source");
            assert_eq!(root.read_file("sample.fixture"), "source");
        }
    }

    #[test]
    fn retained_palette_actions_are_repeatable_passive_and_preserve_identity() {
        use super::super::{WorkspaceAction, WorkspaceTui};

        let root = crate::test_support::TempWorkspace::new("retained-palette-actions");
        root.write_file("sample.fixture", "unsaved λ");
        root.write_file("other.fixture", "different source");
        let service = Box::leak(Box::new(Service(AtomicUsize::new(0))));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        let mut ui = WorkspaceTui::new();
        let mut sources = crate::scratch::ScratchBuffers::default();
        let actions = [WorkspaceAction::InspectResult, WorkspaceAction::ViewDetails];
        for action in &actions {
            assert!(
                ui.actions(&sources, false)
                    .iter()
                    .any(|info| { info.action == *action && info.disabled_reason.is_some() })
            );
            assert!(
                ui.invoke_action(action.clone(), &mut app, &mut sources, None)
                    .is_err()
            );
        }
        assert_eq!(service.0.load(Ordering::SeqCst), 0);
        app.open_path("sample.fixture").unwrap();
        ui.invoke_action(
            WorkspaceAction::InspectDocument,
            &mut app,
            &mut sources,
            None,
        )
        .unwrap();
        let key = |code| KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        ui.handle_key(key(KeyCode::Enter), &mut app);
        ui.handle_key(key(KeyCode::Enter), &mut app);
        let binding = ui.inspection_result().unwrap().document_binding().unwrap();
        let identity = binding.source_id();
        let revision = binding.version();
        ui.handle_key(key(KeyCode::Esc), &mut app);
        app.open_path("other.fixture").unwrap();

        let screen = |ui: &mut WorkspaceTui, app: &App| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(180, 60)).unwrap();
            terminal
                .draw(|frame| ui.render(frame, frame.area(), app))
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
        for action in [
            WorkspaceAction::InspectResult,
            WorkspaceAction::InspectResult,
            WorkspaceAction::ViewDetails,
            WorkspaceAction::ViewDetails,
            WorkspaceAction::InspectResult,
        ] {
            assert!(
                ui.actions(&sources, false)
                    .iter()
                    .any(|info| { info.action == action && info.disabled_reason.is_none() })
            );
            ui.invoke_action(action.clone(), &mut app, &mut sources, None)
                .unwrap();
            let rendered = screen(&mut ui, &app);
            assert!(rendered.contains("STALE"), "{rendered}");
            assert_eq!(
                rendered.contains("┌Result Explorer"),
                action == WorkspaceAction::InspectResult
            );
            let result = ui.inspection_result().unwrap();
            assert_eq!(result.document_binding().unwrap().source_id(), identity);
            assert_eq!(result.document_binding().unwrap().version(), revision);
            assert_eq!(result.document_binding().unwrap().text(), "unsaved λ");
            assert_eq!(service.0.load(Ordering::SeqCst), 1);
        }
        ui.handle_key(key(KeyCode::Esc), &mut app);
        let summary = screen(&mut ui, &app);
        assert!(!summary.contains("┌Result Explorer"));
        assert!(summary.contains("Semantic inspection"), "{summary}");
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
        assert_eq!(app.open_document().unwrap().text(), "different source");
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

    #[cfg(feature = "experimental-sprout-closure")]
    #[test]
    fn closure_observations_have_independent_passive_freshness_and_retention() {
        use crate::{
            registered_document_closure::*,
            text::{Position, TextEdit},
            tool::{
                ToolResult,
                semantic::{HostAuthorizationRequest, HostSemanticPolicy},
            },
        };
        use scope_language::{
            SemanticOperationDescription, SemanticOperationEffect, SemanticTargetRequirement,
        };
        use std::sync::Arc;

        struct FixtureDiscovery(Arc<AtomicUsize>);
        impl Discovery for FixtureDiscovery {
            fn capture(
                &self,
                root: &str,
                sources: Vec<RegisteredSource>,
                limits: Limits,
            ) -> Result<DiscoveredClosure, DiscoveryError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                let compiler_sources = sources
                    .iter()
                    .map(|source| source.identity.clone())
                    .collect();
                DiscoveredClosure::new(DiscoveryEvidence {
                    root: root.into(),
                    sources,
                    graph: vec![("root".into(), vec!["dep".into()]), ("dep".into(), vec![])],
                    exports: vec![("root".into(), vec![]), ("dep".into(), vec![])],
                    compiler_sources,
                    compiler_identity: "semantic-ui-fixture-v1".into(),
                    identity_version: 1,
                    portable_identity: vec![1],
                    limits,
                })
            }
        }
        struct Policy(usize);
        impl HostSemanticPolicy for Policy {
            fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
                self.0 += 1;
                matches!(request, HostAuthorizationRequest::Closure { .. })
            }
        }
        fn complete(app: &mut App, policy: &mut Policy) -> InspectionResult {
            let binding = app
                .capture_registered_execution("root", "inspect")
                .unwrap()
                .unwrap();
            let result = app
                .invoke_registered_execution(binding, policy)
                .unwrap()
                .unwrap();
            InspectionResult::from_closure_observation(
                result.observation(),
                ToolResult::new((*result.output() as i64).into()),
                Some(SemanticOperationResult::ReadOnly {
                    value: json!({"dispatch": result.output()}),
                    rationale: "inert fixture output".into(),
                }),
            )
        }
        let root = crate::test_support::TempWorkspace::new("closure-semantic-retention");
        root.write_file("root.fixture", "root source");
        root.write_file("dep.fixture", "dependency source");
        let discovery_calls = Arc::new(AtomicUsize::new(0));
        let mut app = App::new(root.workspace()).unwrap();
        assert!(
            app.enable_registered_session(RegisteredDocuments::new(
                FixtureDiscovery(discovery_calls.clone()),
                TrustedLoadedArtifact::default(),
                vec![SemanticOperationDescription {
                    id: "inspect".into(),
                    description: "Inert fixture inspection".into(),
                    arguments: vec![],
                    effect: SemanticOperationEffect::Read,
                    target: SemanticTargetRequirement::Document,
                    constraints: None,
                }],
                json!({}),
                Limits::default(),
                8,
            ))
            .is_ok()
        );
        for module in ["root", "dep"] {
            app.open_path(format!("{module}.fixture")).unwrap();
            app.enroll_open_document(module.into(), "fixture".into())
                .unwrap();
        }
        let mut policy = Policy(0);
        let first = complete(&mut app, &mut policy);
        assert!(first.document_binding().is_none());
        assert_eq!(first.freshness_app(&app), InspectionFreshness::Current);
        let detached = App::new(root.workspace()).unwrap();
        assert_eq!(first.freshness_app(&detached), InspectionFreshness::Unknown);
        assert!(!first.is_fresh_app(&detached));
        let mut modal = SemanticInspection::default();
        modal.retain_completed(first);
        app.select_registered_document("dep").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), "changed "))
            .unwrap();
        app.select_registered_document("root").unwrap();
        assert_eq!(
            modal.result().unwrap().freshness_app(&app),
            InspectionFreshness::Stale
        );
        modal.retain_completed(complete(&mut app, &mut policy));
        assert_eq!(
            modal.baseline.as_ref().unwrap().freshness_app(&app),
            InspectionFreshness::Stale
        );
        assert_eq!(
            modal.result().unwrap().freshness_app(&app),
            InspectionFreshness::Current
        );
        let calls = discovery_calls.load(Ordering::SeqCst);
        for _ in 0..2 {
            modal.open_comparison();
            let screen = explorer_screen(&modal, &app, false);
            assert!(screen.contains("Baseline: STALE"), "{screen}");
            assert!(screen.contains("Latest: CURRENT"), "{screen}");
            let unknown = explorer_screen(&modal, &detached, false);
            assert!(unknown.contains("Baseline: UNKNOWN"), "{unknown}");
            assert!(unknown.contains("Latest: UNKNOWN"), "{unknown}");
            modal.comparison_side = Some(false);
            modal.explorer = Some(Default::default());
            assert!(explorer_screen(&modal, &app, false).contains("STALE (registered closure"));
            modal.comparison_side = Some(true);
            assert!(explorer_screen(&modal, &app, false).contains("CURRENT"));
            assert!(explorer_screen(&modal, &detached, false).contains("UNKNOWN"));
        }
        for (result, expected_text, dispatch) in [
            (modal.baseline.as_ref().unwrap(), "dependency source", 1),
            (modal.result().unwrap(), "changed dependency source", 2),
        ] {
            let semantic_inspection::InspectionOrigin::Closure(observation) = &result.origin else {
                panic!("closure provenance was replaced");
            };
            assert_eq!(
                observation
                    .discovered()
                    .sources()
                    .iter()
                    .find(|source| source.module == "dep")
                    .unwrap()
                    .text,
                expected_text
            );
            assert_eq!(
                SemanticInspection::native_value(result).unwrap()["dispatch"],
                dispatch
            );
        }
        app.registered_documents_mut()
            .unwrap()
            .set_policy(json!({"changed": true}));
        assert_eq!(
            modal.result().unwrap().freshness_app(&app),
            InspectionFreshness::Stale
        );
        assert_eq!(policy.0, 2);
        assert_eq!(app.registered_documents().unwrap().dispatches(), 2);
        assert_eq!(discovery_calls.load(Ordering::SeqCst), calls);
    }

    #[test]
    fn retaining_completed_evidence_is_passive_and_advances_existing_pair() {
        let root = crate::test_support::TempWorkspace::new("passive-retention");
        root.write_file("sample.fixture", "unsaved λ");
        let service = Box::leak(Box::new(Service(AtomicUsize::new(0))));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        app.open_path("sample.fixture").unwrap();
        let mut modal = SemanticInspection::candidate(&app).unwrap();
        modal.run(&mut app);
        let retained = modal.result().unwrap().clone();
        modal.open_explorer();
        modal.error = Some("previous attempt".into());
        modal.pending = Some(InspectionBinding::capture_app(&app, "evaluate").unwrap());
        modal.retain_completed(retained);
        assert!(modal.pending.is_none());
        assert!(modal.explorer.is_none());
        assert!(modal.error.is_none());
        assert!(modal.baseline.is_some());
        assert!(modal.comparison_disabled().is_none());
        modal.run(&mut app);
        modal.open_comparison();
        modal.open_details();
        modal.open_explorer();
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
        assert_eq!(app.open_document().unwrap().text(), "unsaved λ");
    }

    #[test]
    fn comparison_pair_advances_moves_and_preserves_failed_attempts() {
        let root = crate::test_support::TempWorkspace::new("comparison-pair");
        root.write_file("first.fixture", "unsaved λ");
        root.write_file("second.fixture", "unsaved λ");
        let service = Box::leak(Box::new(Service(AtomicUsize::new(0))));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        app.open_path("first.fixture").unwrap();
        let mut first = SemanticInspection::candidate(&app).unwrap();
        assert!(
            first
                .comparison_disabled()
                .unwrap()
                .contains("two completed")
        );
        first.run(&mut app);
        let original_text = first
            .result()
            .unwrap()
            .document_binding()
            .unwrap()
            .text()
            .as_ptr();
        app.open_path("second.fixture").unwrap();
        let mut next = SemanticInspection::candidate(&app).unwrap();
        next.inherit_completed(first);
        next.run(&mut app);
        next.run(&mut app);
        assert_eq!(service.0.load(Ordering::SeqCst), 2);
        assert_eq!(
            next.baseline
                .as_ref()
                .unwrap()
                .document_binding()
                .unwrap()
                .text()
                .as_ptr(),
            original_text
        );
        assert!(next.comparison_disabled().is_none());
        assert!(!next.baseline.as_ref().unwrap().is_fresh_app(&app));
        assert!(next.result().unwrap().is_fresh_app(&app));
        let baseline_id = next
            .baseline
            .as_ref()
            .unwrap()
            .document_binding()
            .unwrap()
            .source_id();
        let latest_id = next
            .result()
            .unwrap()
            .document_binding()
            .unwrap()
            .source_id();
        next.open_comparison();
        let screen = explorer_screen(&next, &app, true);
        assert!(
            screen.contains("Baseline: STALE (source changed"),
            "{screen}"
        );
        assert!(
            screen.contains("Latest: STALE (unpublished editor draft)"),
            "{screen}"
        );
        assert!(screen.contains("Structural comparison only"));
        let key = |code| KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        for side in ['1', '2'] {
            next.handle_key(key(KeyCode::Char(side)), &mut app);
            let expected = if side == '1' { baseline_id } else { latest_id };
            assert_eq!(
                next.displayed_result()
                    .unwrap()
                    .document_binding()
                    .unwrap()
                    .source_id(),
                expected
            );
            next.handle_key(key(KeyCode::Enter), &mut app);
            next.handle_key(key(KeyCode::Char('d')), &mut app);
            next.handle_key(key(KeyCode::Enter), &mut app);
            assert_eq!(
                next.displayed_result()
                    .unwrap()
                    .document_binding()
                    .unwrap()
                    .source_id(),
                expected
            );
            assert!(explorer_screen(&next, &app, false).contains("Identity:"));
            next.handle_key(key(KeyCode::Esc), &mut app);
            assert!(next.comparison_side.is_none());
        }
        assert_eq!(service.0.load(Ordering::SeqCst), 2);
        assert!(next.pending.is_none());
        assert!(!next.handle_key(key(KeyCode::Esc), &mut app));
        let previous_latest_text = next
            .result()
            .unwrap()
            .document_binding()
            .unwrap()
            .text()
            .as_ptr();
        next.pending = Some(InspectionBinding::capture_app(&app, "evaluate").unwrap());
        app.open_path("first.fixture").unwrap();
        next.run(&mut app);
        assert!(next.error.is_some());
        assert_eq!(
            next.baseline
                .as_ref()
                .unwrap()
                .document_binding()
                .unwrap()
                .text()
                .as_ptr(),
            original_text
        );
        assert_eq!(
            next.result()
                .unwrap()
                .document_binding()
                .unwrap()
                .text()
                .as_ptr(),
            previous_latest_text
        );
        assert_eq!(service.0.load(Ordering::SeqCst), 2);
        next.pending = Some(InspectionBinding::capture_app(&app, "evaluate").unwrap());
        next.run(&mut app);
        assert_eq!(
            next.baseline
                .as_ref()
                .unwrap()
                .document_binding()
                .unwrap()
                .text()
                .as_ptr(),
            previous_latest_text
        );
        assert_eq!(service.0.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn comparison_end_row_navigation_and_bounds_fit_80_by_24() {
        let root = crate::test_support::TempWorkspace::new("comparison-small-terminal");
        root.write_file("sample.fixture", "unsaved λ");
        let service = Box::leak(Box::new(Service(AtomicUsize::new(0))));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        app.open_path("sample.fixture").unwrap();
        let mut modal = SemanticInspection::candidate(&app).unwrap();
        modal.run(&mut app);
        modal.pending = Some(InspectionBinding::capture_app(&app, "evaluate").unwrap());
        modal.run(&mut app);
        let baseline = json!((0..20).collect::<Vec<_>>());
        let mut latest = baseline.clone();
        latest[19] = json!(99);
        modal.baseline.as_mut().unwrap().native_result = Some(SemanticOperationResult::ReadOnly {
            value: baseline,
            rationale: "fixture".into(),
        });
        modal.result.as_mut().unwrap().native_result = Some(SemanticOperationResult::ReadOnly {
            value: latest,
            rationale: "fixture".into(),
        });
        modal.open_comparison();
        assert!(!modal.handle_key(
            KeyEvent::new(KeyCode::End, crossterm::event::KeyModifiers::NONE),
            &mut app,
        ));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| modal.render(frame, frame.area(), &app, false))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = (0..24)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        let screen = lines.join("\n");
        for expected in [
            "Compare Results",
            "1 inspect baseline • 2 inspect latest • Esc back • Enter is passive",
            "Comparison: changed",
            "Selected path: $/19",
            "↑/↓ select • n/p next/previous difference • Home/End • Esc back",
            "> changed $/19: 19 → 99",
        ] {
            assert!(screen.contains(expected), "missing {expected}:\n{screen}");
        }
        assert!(
            lines[21].contains("Bounded comparison: 1024 nodes, depth 32, 65536 comparison bytes;"),
            "bounds footer must remain inside the modal:\n{screen}",
        );
        assert_eq!(service.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn comparison_unsupported_completions_advance_without_parsing_feedback() {
        let root = crate::test_support::TempWorkspace::new("comparison-unsupported");
        root.write_file("sample.fixture", "source");
        let mut app = App::builder(root.workspace())
            .language(&FailureService("blocked"))
            .build()
            .unwrap();
        app.open_path("sample.fixture").unwrap();
        let mut modal = SemanticInspection::candidate(&app).unwrap();
        modal.run(&mut app);
        let old = modal
            .result()
            .unwrap()
            .document_binding()
            .unwrap()
            .text()
            .as_ptr();
        modal.pending = Some(InspectionBinding::capture_app(&app, "evaluate").unwrap());
        modal.run(&mut app);
        assert_eq!(
            modal
                .baseline
                .as_ref()
                .unwrap()
                .document_binding()
                .unwrap()
                .text()
                .as_ptr(),
            old
        );
        assert!(modal.comparison_disabled().unwrap().contains("native JSON"));
        modal.open_comparison();
        assert!(modal.comparison.is_none());
    }

    #[test]
    fn direct_run_comparison_catalog_is_passive_and_preserves_focus() {
        use super::super::{EditableScratchPane, View, WorkspaceAction, WorkspaceTui};
        let root = crate::test_support::TempWorkspace::new("direct-run-comparison");
        root.write_file("sample.fixture", "unsaved λ");
        let service = Box::leak(Box::new(Service(AtomicUsize::new(0))));
        let mut app = App::builder(root.workspace())
            .language(service)
            .build()
            .unwrap();
        let mut sources = crate::scratch::ScratchBuffers::default();
        let mut ui = WorkspaceTui::new();
        ui.register_editable("editor", EditableScratchPane::new())
            .unwrap();
        ui.layout("editor", View::leaf("editor")).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(180, 60)).unwrap();
        terminal
            .draw(|f| ui.render_with_sources(f, f.area(), &app, &sources))
            .unwrap();
        ui.open_app_document(&mut app, "sample.fixture").unwrap();
        terminal
            .draw(|f| ui.render_with_sources(f, f.area(), &app, &sources))
            .unwrap();
        let focus = ui.focused().map(str::to_owned);
        for count in 0..=2 {
            let info = ui
                .actions(&sources, false)
                .into_iter()
                .find(|i| i.action == WorkspaceAction::CompareResults)
                .unwrap();
            assert_eq!(info.id, "result.compare");
            assert!(info.description().contains("diff results"));
            assert_eq!(info.disabled_reason.is_none(), count == 2);
            if count < 2 {
                ui.invoke_action(
                    WorkspaceAction::RunCurrentDocument,
                    &mut app,
                    &mut sources,
                    None,
                )
                .unwrap();
            }
        }
        ui.invoke_action(
            WorkspaceAction::CompareResults,
            &mut app,
            &mut sources,
            None,
        )
        .unwrap();
        let key = |code| KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        for code in [
            KeyCode::Enter,
            KeyCode::Char('1'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Char('2'),
            KeyCode::Char('d'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Esc,
            KeyCode::Esc,
        ] {
            ui.handle_key(key(code), &mut app);
            assert_eq!(ui.focused(), focus.as_deref());
            assert_eq!(service.0.load(Ordering::SeqCst), 2);
        }
        assert_eq!(root.read_file("sample.fixture"), "unsaved λ");
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
            .draw(|frame| modal.render(frame, frame.area(), &app, false))
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
                .draw(|frame| modal.render(frame, frame.area(), app, false))
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
