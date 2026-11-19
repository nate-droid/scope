//! Registered closure presentation. Capture is inspection, never authorization.
use crate::{
    app::AppState,
    closure_inspection::ClosureInspectionBinding,
    registered_document_closure::{
        RegisteredCaptureRejection, RegisteredClosureBinding, RegisteredClosureResult,
        TrustedClosureProvider,
    },
    tool::semantic::{HostAuthorizationRequest, HostSemanticPolicy},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

/// Host-formatted output. Scope retains the original typed result independently.
/// Formatting is presentation only; it must not execute or resolve replacement sources.
pub struct RegisteredOutput {
    pub summary: String,
    pub details: String,
}

type ReviewRequirement = Box<dyn Fn(&ClosureInspectionBinding) -> Option<String>>;
type Formatter<O> = Box<dyn Fn(&RegisteredClosureResult<O>) -> RegisteredOutput>;
type NativeFeedback<O> = Box<
    dyn Fn(&RegisteredClosureResult<O>) -> Result<scope_language::SemanticOperationResult, String>,
>;

/// Install one registered operation behind the ordinary Run and Check actions.
/// Installation grants no authority. Run requires explicit review by default;
/// Check only discovers/captures the closure and never consults the policy.
pub struct RegisteredExecution<O> {
    operation: String,
    format: Formatter<O>,
    native_feedback: Option<NativeFeedback<O>>,
    invocation_contract: Option<crate::registered_document_closure::InvocationContract>,
    policy: Option<Box<dyn HostSemanticPolicy>>,
    review_requirement: Option<ReviewRequirement>,
}
impl<O> RegisteredExecution<O> {
    /// The formatter runs once after invocation returns a typed result, never on render.
    /// Scope bounds displayed summary/details to 4,096/65,536 characters and retains
    /// the original result independently, including nested provider failure evidence.
    pub fn new(
        operation: impl Into<String>,
        format: impl Fn(&RegisteredClosureResult<O>) -> RegisteredOutput + 'static,
    ) -> Self {
        Self {
            operation: operation.into(),
            format: Box::new(format),
            native_feedback: None,
            invocation_contract: None,
            policy: None,
            review_requirement: None,
        }
    }

    /// Project native feedback once from the authentic completion, never from
    /// separately supplied values/evidence or during passive navigation.
    pub fn with_native_feedback(
        mut self,
        project: impl Fn(
            &RegisteredClosureResult<O>,
        ) -> Result<scope_language::SemanticOperationResult, String>
        + 'static,
    ) -> Self {
        self.native_feedback = Some(Box::new(project));
        self
    }

    /// Bind the producer's explicit zero-argument invocation contract to Run/Check.
    /// The complete contract is reviewed and authorized with the captured sources.
    pub fn with_invocation_contract(
        mut self,
        contract: crate::registered_document_closure::InvocationContract,
    ) -> Self {
        self.invocation_contract = Some(contract);
        self
    }

    /// Additional host restrictions, conjunctive with exact single-use UI confirmation.
    pub fn with_policy(mut self, policy: impl HostSemanticPolicy + 'static) -> Self {
        self.policy = Some(Box::new(policy));
        self.review_requirement = None;
        self
    }

    /// Opt in to running within existing authority without confirmation.
    /// `review_requirement` returns a human-readable reason when the sealed call
    /// needs human review, or `None` when policy alone may decide. It grants no
    /// authority: the supplied policy must authorize the exact call in either
    /// case. A policy denial never falls back to confirmation.
    pub fn with_policy_authorized_run(
        mut self,
        policy: impl HostSemanticPolicy + 'static,
        review_requirement: impl Fn(&ClosureInspectionBinding) -> Option<String> + 'static,
    ) -> Self {
        self.policy = Some(Box::new(policy));
        self.review_requirement = Some(Box::new(review_requirement));
        self
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.operation.is_empty()
            || self.operation.len() > 256
            || self
                .operation
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace())
        {
            return Err("invalid registered execution operation".into());
        }
        Ok(())
    }
}

struct SourceIdentity {
    module: String,
    source: scope_language::SourceId,
}
struct Captured {
    selection: RegisteredClosureBinding,
    identities: Vec<SourceIdentity>,
}
struct Retained<O> {
    result: RegisteredClosureResult<O>,
    identities: Vec<SourceIdentity>,
    output: RegisteredOutput,
}
#[derive(Default)]
enum Latest {
    #[default]
    Empty,
    Review(String),
    Check,
    Result,
    Message(String),
}

struct DiagnosticLocation {
    module: String,
    source: scope_language::SourceId,
    version: u64,
    cursor: usize,
}

pub(super) struct RegisteredInspection<O> {
    config: RegisteredExecution<O>,
    pending: Option<Captured>,
    checked: Option<Captured>,
    retained: Option<Retained<O>>,
    latest: Latest,
    open: bool,
    scroll: u16,
    review_details: bool,
    completed_observation: Option<crate::semantic_inspection::InspectionResult>,
    diagnostic: Option<DiagnosticLocation>,
}
impl<O> RegisteredInspection<O> {
    pub(super) fn new(config: RegisteredExecution<O>) -> Self {
        Self {
            config,
            pending: None,
            checked: None,
            retained: None,
            latest: Latest::Empty,
            open: false,
            scroll: 0,
            review_details: false,
            completed_observation: None,
            diagnostic: None,
        }
    }

    pub(super) fn take_completed_observation(
        &mut self,
    ) -> Option<crate::semantic_inspection::InspectionResult> {
        self.completed_observation.take()
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }
    pub(super) fn result(&self) -> Option<&RegisteredClosureResult<O>> {
        self.retained.as_ref().map(|retained| &retained.result)
    }

    pub(super) fn has_diagnostic(&self) -> bool {
        self.diagnostic.is_some()
    }

    pub(super) fn diagnostic_target<P: TrustedClosureProvider<Output = O>>(
        &self,
        app: &AppState<P>,
    ) -> Result<(String, usize), String> {
        let target = self
            .diagnostic
            .as_ref()
            .ok_or("no registered result location")?;
        let document = app
            .registered_documents()
            .and_then(|documents| documents.document(&target.module))
            .filter(|document| {
                document.evaluation_source_id() == &target.source
                    && document.version() == target.version
            })
            .ok_or("registered diagnostic is historical; Check again")?;
        Ok((document.path().display().to_string(), target.cursor))
    }

    fn capture<P: TrustedClosureProvider<Output = O>>(
        &mut self,
        app: &mut AppState<P>,
    ) -> Result<Captured, String> {
        let root = app
            .selected_registered_module()
            .ok_or("select a registered App document")?
            .to_owned();
        let selection = app
            .prepare_registered_call(
                &root,
                &self.config.operation,
                serde_json::json!({}),
                self.config.invocation_contract.clone(),
            )
            .ok_or("registered session is disabled")?;
        let documents = app
            .registered_documents()
            .ok_or("registered session is disabled")?;
        let selection = selection.map_err(|error| {
            if let RegisteredCaptureRejection::Diagnostics { diagnostics, .. } = &error {
                self.diagnostic = diagnostics.iter().take(8).find_map(|diagnostic| {
                    let document = documents.document(&diagnostic.module)?;
                    document.text().get(diagnostic.start..diagnostic.end)?;
                    Some(DiagnosticLocation {
                        module: diagnostic.module.clone(),
                        source: document.evaluation_source_id().clone(),
                        version: document.version(),
                        cursor: diagnostic.start,
                    })
                });
            }
            capture_error(app, error)
        })?;
        let identities = selection
            .discovered()
            .sources()
            .iter()
            .map(|source| {
                let document = documents
                    .document(&source.module)
                    .ok_or("captured document is unavailable")?;
                Ok(SourceIdentity {
                    module: source.module.clone(),
                    source: document.evaluation_source_id().clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Captured {
            selection,
            identities,
        })
    }

    /// Called only after the existing App editor publication and global draft checks.
    pub(super) fn start<P: TrustedClosureProvider<Output = O>>(
        &mut self,
        app: &mut AppState<P>,
        run: bool,
    ) -> bool {
        self.pending = None;
        self.diagnostic = None;
        self.scroll = 0;
        self.review_details = false;
        self.open = false;
        match self.capture(app) {
            Ok(captured) if run => {
                // Review construction is bounded and validated before opening confirmation.
                match captured.selection.binding().review() {
                    Ok(_) => {
                        let reason = match &self.config.review_requirement {
                            Some(requirement) => requirement(captured.selection.binding()),
                            None => Some("This launcher requires approval for every Run.".into()),
                        };
                        let Some(reason) = reason else {
                            return self.dispatch(app, captured, None);
                        };
                        self.pending = Some(captured);
                        self.latest = Latest::Review(diagnostic_line(&reason, 1024));
                        self.open = true;
                    }
                    Err(error) => {
                        self.latest = Latest::Message(format!("Closure review rejected: {error:?}"))
                    }
                }
            }
            Ok(captured) => {
                self.checked = Some(captured);
                self.latest = Latest::Check;
            }
            Err(error) => self.latest = Latest::Message(error),
        }
        false
    }

    pub(super) fn inspect(&mut self) {
        self.open = true;
        self.scroll = 0;
    }

    pub(super) fn handle_key<P: TrustedClosureProvider<Output = O>>(
        &mut self,
        key: KeyEvent,
        app: &mut AppState<P>,
        draft_blocker: Option<String>,
    ) -> bool {
        match key.code {
            KeyCode::Esc => {
                if self.pending.take().is_some() {
                    self.latest =
                        Latest::Message("Run cancelled; no authorization or dispatch".into());
                }
                self.open = false;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.pending.take().is_some() {
                    self.latest =
                        Latest::Message("Run cancelled; no authorization or dispatch".into());
                }
                self.open = false;
            }
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::Home => self.scroll = 0,
            KeyCode::Char('d') if self.pending.is_some() => {
                self.review_details = !self.review_details;
                self.scroll = 0;
            }
            KeyCode::Enter if self.pending.is_some() => {
                let captured = self.pending.take().expect("pending selection");
                return self.dispatch(app, captured, draft_blocker);
            }
            // Enter after consumption is deliberately passive: no recapture, approval or replay.
            _ => {}
        }
        false
    }

    fn dispatch<P: TrustedClosureProvider<Output = O>>(
        &mut self,
        app: &mut AppState<P>,
        captured: Captured,
        draft_blocker: Option<String>,
    ) -> bool {
        if let Some(reason) = draft_blocker {
            self.latest = Latest::Message(format!("Run rejected: {reason}; review again"));
            return false;
        }
        // Selection changes must never authorize the formerly selected root.
        if sources_changed(app, captured.selection.binding(), &captured.identities) {
            self.latest =
                Latest::Message("Run rejected: STALE closure sources; Run again to review".into());
            return false;
        }
        let expected = captured.selection.binding().clone();
        let review = match expected.review() {
            Ok(review) => review,
            Err(error) => {
                self.latest = Latest::Message(format!("Run rejected: {error:?}"));
                return false;
            }
        };
        let mut policy = ConfirmedReview {
            expected: &expected,
            review: review.as_str(),
            used: false,
            host: self.config.policy.as_deref_mut(),
        };
        // Consume exactly the displayed selection. No recapture or automatic retry.
        let completed = match app.invoke_registered_execution(captured.selection, &mut policy) {
            Some(Ok(result)) => {
                let output = (self.config.format)(&result);
                self.completed_observation = self.config.native_feedback.as_ref().map(|project| {
                    crate::semantic_inspection::InspectionResult::from_registered_completion(
                        &result, project,
                    )
                });
                self.retained = Some(Retained {
                    result,
                    identities: captured.identities,
                    output: RegisteredOutput {
                        summary: bounded(&output.summary, 4096),
                        details: bounded(&output.details, 65536),
                    },
                });
                self.latest = Latest::Result;
                true
            }
            Some(Err(error)) => {
                self.latest = Latest::Message(format!("Run rejected: {error:?}"));
                false
            }
            None => {
                self.latest =
                    Latest::Message("Run rejected: registered session is disabled".into());
                false
            }
        };
        self.scroll = 0;
        if completed {
            self.open = false;
        }
        completed
    }

    pub(super) fn concise<P: TrustedClosureProvider<Output = O>>(
        &self,
        app: &AppState<P>,
        drafts: bool,
    ) -> Option<String> {
        let (text, binding) = match &self.latest {
            Latest::Empty => return None,
            Latest::Review(reason) => (
                format!("Approval required: {reason}"),
                self.pending
                    .as_ref()
                    .map(|p| (p.selection.binding(), p.identities.as_slice())),
            ),
            Latest::Check => {
                let checked = self.checked.as_ref()?;
                (
                    format!(
                        "Closure valid (discovery): {} registered source(s); not authorized or executed",
                        checked.selection.discovered().sources().len()
                    ),
                    Some((checked.selection.binding(), checked.identities.as_slice())),
                )
            }
            Latest::Result => {
                let retained = self.retained.as_ref()?;
                (
                    format!("{}\nd: Details / Inspect", retained.output.summary),
                    Some((retained.result.binding(), retained.identities.as_slice())),
                )
            }
            Latest::Message(message) => (message.clone(), None),
        };
        Some(
            if binding.is_some_and(|(binding, identities)| {
                drafts || sources_changed(app, binding, identities)
            }) {
                format!("STALE (closure sources or unpublished draft changed)\n{text}")
            } else {
                text
            },
        )
    }

    pub(super) fn render<P: TrustedClosureProvider<Output = O>>(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        app: &AppState<P>,
        drafts: bool,
    ) {
        if !self.open {
            return;
        }
        let modal = Rect::new(
            area.x + area.width / 20,
            area.y + area.height / 20,
            area.width.saturating_sub(area.width / 10),
            area.height.saturating_sub(area.height / 10),
        );
        frame.render_widget(Clear, modal);
        let mut text = self
            .concise(app, drafts)
            .unwrap_or_else(|| "No registered evidence. Use Check or Run.".into());
        if let Some(pending) = &self.pending {
            let snapshot = pending.selection.binding().snapshot();
            text.push_str(&format!("\n\nOperation: {}", snapshot.operation.id));
            if let Some(entry) =
                snapshot.compilation_options["invocation_contract"]["entry"].as_str()
            {
                text.push_str(&format!(" · Entry: {}", diagnostic_line(entry, 256)));
            }
            text.push_str("\n\nEnter: approve once · Esc: cancel · d: toggle invocation details\n");
            if self.review_details {
                append_review(&mut text, pending.selection.binding());
            }
        } else {
            text.push_str("\n\nHistorical evidence only. Source currentness is not authorization; policy/artifact freshness is rechecked on Run.\nEsc: close · Up/Down/PageUp/PageDown: scroll\n");
            if matches!(self.latest, Latest::Check) {
                if let Some(checked) = &self.checked {
                    append_review(&mut text, checked.selection.binding());
                }
            }
            if let Some(retained) = &self.retained {
                if !matches!(self.latest, Latest::Result) {
                    text.push_str("\nEarlier retained result (not the outcome of this attempt):\n");
                }
                text.push_str(&retained.output.details);
                text.push_str("\n\nOriginal whole-closure review:\n");
                append_review(&mut text, retained.result.binding());
            }
        }
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(if self.pending.is_some() {
                            "Run registered closure?"
                        } else {
                            "Registered closure Details"
                        }),
                ),
            modal,
        );
    }
}

/// Diagnostics carry one-based line/column coordinates from the trusted discovery
/// facade. Resolve missing display paths from the authoritative registered owner.
fn capture_error<P: TrustedClosureProvider>(
    app: &AppState<P>,
    rejection: RegisteredCaptureRejection,
) -> String {
    use crate::closure_inspection::ClosureInspectionError;
    use crate::registered_document_closure::DiscoveryCategory;
    let discovery_help = |category| match category {
        DiscoveryCategory::Bounds => {
            "closure exceeds discovery limits; reduce its size or adjust host limits"
        }
        DiscoveryCategory::DuplicateOrCapacity => {
            "check duplicate module names and registration capacity"
        }
        DiscoveryCategory::MissingSource => {
            "required module unavailable; check the module root and workspace access"
        }
        DiscoveryCategory::Language => "register dependencies with the same supported language",
        DiscoveryCategory::SourceAdmission => {
            "fix source text or module declarations and Check again"
        }
        DiscoveryCategory::GraphOrMetadata => "check imports and registered module identities",
        DiscoveryCategory::Compilation => "fix source diagnostics and Check again",
    };
    match rejection {
        RegisteredCaptureRejection::Diagnostics {
            error, diagnostics, ..
        } => {
            let mut output = format!(
                "Closure capture rejected: {}",
                discovery_help(error.category)
            );
            for diagnostic in diagnostics.iter().take(8) {
                let path = diagnostic
                    .path
                    .clone()
                    .or_else(|| {
                        app.registered_documents()?
                            .document(&diagnostic.module)
                            .map(|document| document.path().display().to_string())
                    })
                    .unwrap_or_else(|| diagnostic.module.clone());
                output.push_str(&format!(
                    "\n{}:{}:{}: {}",
                    diagnostic_line(&path, 1024),
                    diagnostic.line,
                    diagnostic.column,
                    diagnostic_line(&diagnostic.message, 1024)
                ));
            }
            if diagnostics.len() > 8 {
                output.push_str(
                    "\nAdditional diagnostics omitted; fix the first errors and Check again.",
                );
            }
            output
        }
        RegisteredCaptureRejection::Discovery { error, .. } => format!(
            "Closure capture rejected: {}",
            discovery_help(error.category)
        ),
        RegisteredCaptureRejection::Inspection(error) => {
            let message = match error {
                ClosureInspectionError::InvalidCapture => "operation or closure unavailable; check the configured operation and enrolled modules".into(),
                ClosureInspectionError::LimitExceeded => "capture exceeds host limits; reduce the closure size".into(),
                ClosureInspectionError::SnapshotMismatch => "registered sources changed; Check again".into(),
                ClosureInspectionError::Denied => "host rejected capture".into(),
                ClosureInspectionError::Host(message) => diagnostic_line(&message, 1024),
            };
            format!("Closure capture rejected: {message}")
        }
    }
}
fn diagnostic_line(text: &str, max: usize) -> String {
    bounded(
        &text
            .chars()
            .take(max.saturating_add(1))
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect::<String>(),
        max,
    )
}

fn bounded(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let mut output: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        output.push_str("\n[truncated]");
    }
    output
}
fn append_review(text: &mut String, binding: &ClosureInspectionBinding) {
    match binding.review() {
        Ok(review) => text.push_str(review.as_str()),
        Err(error) => text.push_str(&format!("Review unavailable: {error:?}")),
    }
}

/// Passive source-only currentness. It neither discovers nor asserts policy/artifact freshness.
fn sources_changed<P: TrustedClosureProvider>(
    app: &AppState<P>,
    binding: &ClosureInspectionBinding,
    identities: &[SourceIdentity],
) -> bool {
    let Some(documents) = app.registered_documents() else {
        return true;
    };
    let snapshot = binding.snapshot();
    let Some(root) = snapshot
        .sources
        .iter()
        .find(|source| source.id == snapshot.root)
    else {
        return true;
    };
    if app.selected_registered_module() != Some(root.module.as_str()) {
        return true;
    }
    snapshot.sources.iter().any(|source| {
        let Some(document) = documents.document(&source.module) else {
            return true;
        };
        let Some(identity) = identities
            .iter()
            .find(|identity| identity.module == source.module)
        else {
            return true;
        };
        document.evaluation_source_id() != &identity.source
            || documents.binding_source_id(&source.module, &identity.source) != Some(source.id)
            || document.version() != source.version
            || document.text() != source.text
    })
}

struct ConfirmedReview<'a> {
    expected: &'a ClosureInspectionBinding,
    review: &'a str,
    used: bool,
    host: Option<&'a mut (dyn HostSemanticPolicy + 'static)>,
}
impl HostSemanticPolicy for ConfirmedReview<'_> {
    fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
        let HostAuthorizationRequest::Closure { binding, review } = &request else {
            return false;
        };
        if self.used || *binding != self.expected || review.as_str() != self.review {
            return false;
        }
        self.used = true;
        self.host
            .as_mut()
            .is_none_or(|host| host.authorize(request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        closure_inspection::{ClosureCapture, ClosureOwner, ClosureSource},
        document::DocumentId,
        source_edit::SourceId,
    };
    use scope_language::{
        SemanticOperationDescription, SemanticOperationEffect, SemanticTargetRequirement,
    };

    #[test]
    fn confirmation_matches_the_complete_binding_not_only_its_source_free_review() {
        let id = SourceId::Document(DocumentId::new(1));
        let capture = ClosureCapture {
            owner: ClosureOwner::default(),
            sources: vec![ClosureSource {
                id,
                text: "original".into(),
                version: 0,
                language: "fixture".into(),
                module: "root".into(),
                origin: "test".into(),
            }],
            root: id,
            graph: vec![],
            compilation_identity: "compiler".into(),
            compilation_options: serde_json::json!({}),
            compilation_limits: serde_json::json!({}),
            operation: SemanticOperationDescription {
                id: "inspect".into(),
                description: "test".into(),
                arguments: vec![],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            },
            execution_policy: serde_json::json!({}),
            artifact_identity: "prepared".into(),
        };
        let expected = ClosureInspectionBinding::capture(capture.clone()).unwrap();
        let mut substituted = capture;
        substituted.sources[0].text = "replaced".into();
        let substituted = ClosureInspectionBinding::capture(substituted).unwrap();
        let review = expected.review().unwrap();
        // Reviews deliberately project source length/metadata, not raw text.
        assert_eq!(review.as_str(), substituted.review().unwrap().as_str());
        let mut policy = ConfirmedReview {
            expected: &expected,
            review: review.as_str(),
            used: false,
            host: None,
        };
        assert!(!policy.authorize(HostAuthorizationRequest::Closure {
            binding: &substituted,
            review: &review
        }));
        assert!(policy.authorize(HostAuthorizationRequest::Closure {
            binding: &expected,
            review: &review
        }));
        assert!(!policy.authorize(HostAuthorizationRequest::Closure {
            binding: &expected,
            review: &review
        }));
    }
    #[test]
    fn capture_diagnostics_use_one_based_locations_and_no_debug_dump() {
        use crate::registered_document_closure::{
            DiscoveryCategory, DiscoveryDiagnostic, DiscoveryError, DiscoveryPhase,
        };
        let root = crate::test_support::TempWorkspace::new("closure-ui-diagnostics");
        let app = crate::app::App::new(root.workspace()).unwrap();
        let message = capture_error(
            &app,
            RegisteredCaptureRejection::Diagnostics {
                error: DiscoveryError {
                    phase: DiscoveryPhase::Compilation,
                    category: DiscoveryCategory::Compilation,
                },
                root: None,
                diagnostics: vec![DiscoveryDiagnostic {
                    module: "domain".into(),
                    path: Some("workflow/domain.sprout".into()),
                    start: 4,
                    end: 9,
                    line: 3,
                    column: 7,
                    message: "expected Bool\nfound Int32\u{1b}".into(),
                }],
            },
        );
        assert!(message.contains("workflow/domain.sprout:3:7: expected Bool found Int32 "));
        assert!(message.contains("Check again"));
        assert!(!message.contains("Diagnostics {"));
        assert!(!message.contains("SourceId"));
        assert!(!message.contains('\u{1b}'));
    }
}
