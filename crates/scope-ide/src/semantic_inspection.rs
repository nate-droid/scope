//! Revision-bound, explicitly authorized semantic inspection for interactive hosts.
//! Discovery is not authorization. Bindings/results are owned display state, not
//! editable documents. Identity is local to one App/document owner (as in SourceId).
use crate::{
    app::App,
    document::Document,
    source_edit::{DocumentSource, PermittedEditScope, SourceEditController, SourceId},
    tool::{
        ToolCall, ToolError, ToolProvider, ToolResult, ToolValue,
        semantic::{SemanticAuthorization, SemanticToolProvider},
    },
};
use scope_language::{
    LanguageService, SemanticOperationDescription, SemanticOperationEffect,
    SemanticTargetRequirement,
};
use std::collections::BTreeSet;

#[derive(Debug)]
pub enum InspectionError {
    NoDocument,
    InvalidOperationId(String),
    UnknownOperation(String),
    NotInspectable(String),
    SnapshotMismatch,
    Tool(ToolError),
}
impl std::fmt::Display for InspectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "semantic inspection failed: {self:?}")
    }
}
impl std::error::Error for InspectionError {}
impl From<ToolError> for InspectionError {
    fn from(error: ToolError) -> Self {
        Self::Tool(error)
    }
}

fn inspectable(operation: &SemanticOperationDescription) -> bool {
    operation.effect == SemanticOperationEffect::Read
        && operation.target == SemanticTargetRequirement::Document
        && operation.arguments.is_empty()
}
fn catalog(
    service: &dyn LanguageService,
) -> Result<Vec<SemanticOperationDescription>, InspectionError> {
    let operations = service.semantic_operations();
    let mut ids = BTreeSet::new();
    for operation in &operations {
        if operation.id.is_empty() || !ids.insert(operation.id.clone()) {
            return Err(InspectionError::InvalidOperationId(operation.id.clone()));
        }
    }
    Ok(operations)
}

/// Discover only operations this bridge can invoke. Optional arguments also
/// exclude an operation: this UI deliberately has no argument editor.
pub fn discover(
    service: &dyn LanguageService,
) -> Result<Vec<SemanticOperationDescription>, InspectionError> {
    Ok(catalog(service)?.into_iter().filter(inspectable).collect())
}
pub fn discover_app(app: &App) -> Result<Vec<SemanticOperationDescription>, InspectionError> {
    let document = app.open_document().ok_or(InspectionError::NoDocument)?;
    discover(app.semantic_service_for_document(document))
}

/// An operation selection bound to exact in-memory source, not saved disk text.
/// Fields are private so changing the operation requires a fresh capture.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectionBinding {
    source_id: SourceId,
    text: String,
    version: u64,
    language: String,
    operation: SemanticOperationDescription,
}
impl InspectionBinding {
    pub fn capture(
        document: &Document,
        service: &dyn LanguageService,
        operation_id: &str,
    ) -> Result<Self, InspectionError> {
        let operation = catalog(service)?
            .into_iter()
            .find(|op| op.id == operation_id)
            .ok_or_else(|| InspectionError::UnknownOperation(operation_id.into()))?;
        if !inspectable(&operation) {
            return Err(InspectionError::NotInspectable(operation_id.into()));
        }
        Ok(Self {
            source_id: SourceId::Document(document.id()),
            text: document.text().into(),
            version: document.version(),
            language: service.language_id().into(),
            operation,
        })
    }
    pub fn capture_app(app: &App, operation_id: &str) -> Result<Self, InspectionError> {
        let document = app.open_document().ok_or(InspectionError::NoDocument)?;
        Self::capture(
            document,
            app.semantic_service_for_document(document),
            operation_id,
        )
    }
    pub fn source_id(&self) -> SourceId {
        self.source_id
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn language(&self) -> &str {
        &self.language
    }
    pub fn operation(&self) -> &SemanticOperationDescription {
        &self.operation
    }
    /// Freshness is source/language freshness; it does not imply authorization.
    pub fn is_fresh(&self, document: &Document, service: &dyn LanguageService) -> bool {
        self.source_id == SourceId::Document(document.id())
            && self.text == document.text()
            && self.version == document.version()
            && self.language == service.language_id()
    }
    pub fn is_fresh_app(&self, app: &App) -> bool {
        app.open_document().is_some_and(|document| {
            self.is_fresh(document, app.semantic_service_for_document(document))
        })
    }
    /// The policy sees the provider's actual operation, source and empty arguments.
    /// It must explicitly grant this invocation; no ambient session grant is used.
    pub fn invoke(
        &self,
        document: &mut Document,
        service: &dyn LanguageService,
        mut authorize: impl for<'a> FnMut(SemanticAuthorization<'a>) -> bool,
    ) -> Result<InspectionResult, InspectionError> {
        if !self.is_fresh(document, service) {
            return Err(InspectionError::SnapshotMismatch);
        }
        let current = Self::capture(document, service, &self.operation.id)?;
        if current != *self {
            return Err(InspectionError::SnapshotMismatch);
        }
        let target = DocumentSource::new(document, service.language_id());
        let mut controller = SourceEditController::default();
        let mut provider = SemanticToolProvider::new(
            service,
            &target,
            &mut controller,
            PermittedEditScope::Document(self.source_id),
            None,
            |request: SemanticAuthorization<'_>| {
                request.operation == &self.operation
                    && inspectable(request.operation)
                    && request.source.id == self.source_id
                    && request.source.version == self.version
                    && request.source.text == self.text
                    && request.source.language == self.language
                    && request
                        .arguments
                        .as_object()
                        .is_some_and(|args| args.is_empty())
                    && authorize(request)
            },
        )?;
        let result = provider.invoke(
            ToolCall::new(format!("language.{}.{}", self.language, self.operation.id))
                .with_argument("expected_version", self.version.to_string().into())
                .with_argument("expected_source", self.text.clone().into())
                .with_argument("arguments", ToolValue::Object(Default::default())),
        )?;
        Ok(InspectionResult {
            binding: self.clone(),
            native_result: provider.native_result().cloned(),
            result,
        })
    }
    pub fn invoke_app(
        &self,
        app: &mut App,
        authorize: impl for<'a> FnMut(SemanticAuthorization<'a>) -> bool,
    ) -> Result<InspectionResult, InspectionError> {
        let document = app.open_document().ok_or(InspectionError::NoDocument)?;
        let service = app.semantic_service_for_document(document);
        self.invoke(
            app.open_document_mut().ok_or(InspectionError::NoDocument)?,
            service,
            authorize,
        )
    }
}

/// Retain structured provider feedback (including denied/blocked/error statuses)
/// alongside its binding. Displaying an old result never makes it fresh again.
#[derive(Debug, Clone)]
pub struct InspectionResult {
    pub binding: InspectionBinding,
    pub result: ToolResult,
    /// Lossless validated language observation, independent of ToolValue limits.
    /// The enum is not Serialize; read variants contain serializable JSON `value`
    /// fields (and Evaluated has a serializable evaluation). Match the variant
    /// when rendering rather than converting its payload through ToolValue.
    pub native_result: Option<scope_language::SemanticOperationResult>,
}
impl InspectionResult {
    pub fn is_fresh_app(&self, app: &App) -> bool {
        self.binding.is_fresh_app(app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::DocumentId, text::TextEdit};
    use scope_language::{
        LanguageAnalysis, LanguageDocument, LanguagePath, SemanticOperationError,
        SemanticOperationRequest, SemanticOperationResult,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Service {
        language: &'static str,
        operations: Vec<SemanticOperationDescription>,
        outcome: Result<SemanticOperationResult, SemanticOperationError>,
        calls: AtomicUsize,
    }
    impl LanguageService for Service {
        fn language_id(&self) -> &'static str {
            self.language
        }
        fn language_name(&self) -> &'static str {
            self.language
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            true
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            panic!("no analysis")
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            self.operations.clone()
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(request.selection.is_none());
            assert_eq!(request.arguments, serde_json::json!({}));
            assert_eq!(request.document.text(), "unsaved λ");
            assert_eq!(request.document.version(), 1);
            assert_eq!(request.document.path().as_str(), "sample.fixture");
            self.outcome.clone()
        }
    }
    fn service() -> Service {
        Service {
            language: "fixture",
            operations: vec![SemanticOperationDescription {
                id: "inspect".into(),
                description: "inspect source".into(),
                arguments: vec![],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            }],
            outcome: Ok(SemanticOperationResult::ReadOnly {
                value: serde_json::json!({"nested": [1, true]}),
                rationale: "evidence".into(),
            }),
            calls: AtomicUsize::new(0),
        }
    }
    fn replacement(document: &Document, text: &str) -> TextEdit {
        TextEdit::replace(
            document
                .buffer()
                .range_for_span(crate::text::Span::new(0, document.text().len()))
                .unwrap(),
            text,
        )
    }
    fn document() -> Document {
        let mut document = Document::new(DocumentId::new(7), "sample.fixture", "saved");
        document
            .apply_edit(replacement(&document, "unsaved λ"))
            .unwrap();
        document
    }
    fn status(result: &InspectionResult) -> &str {
        let ToolValue::Object(fields) = &result.result.value else {
            panic!("object")
        };
        fields["status"].as_str().unwrap()
    }
    #[test]
    fn unsaved_source_structured_result_and_no_mutation() {
        let service = service();
        let mut document = document();
        let before = document.clone();
        let binding = InspectionBinding::capture(&document, &service, "inspect").unwrap();
        let result = binding
            .invoke(&mut document, &service, |request| {
                assert_eq!(request.source.text, "unsaved λ");
                assert_eq!(request.operation.id, "inspect");
                true
            })
            .unwrap();
        assert_eq!(status(&result), "read_only");
        assert_eq!(document, before);
        assert!(document.is_dirty());
        assert_eq!(service.calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn native_json_preserves_floats_and_large_unsigned_values() {
        let mut service = service();
        let value = serde_json::json!({"float": 1.25, "large": u64::MAX, "nested": [0.125, {"max": u64::MAX}]});
        service.outcome = Ok(SemanticOperationResult::ReadOnly {
            value: value.clone(),
            rationale: "lossless".into(),
        });
        let mut document = document();
        let binding = InspectionBinding::capture(&document, &service, "inspect").unwrap();
        let result = binding.invoke(&mut document, &service, |_| true).unwrap();
        // Existing agent protocol behavior is intentionally unchanged.
        assert_eq!(status(&result), "internal_failure");
        let Some(SemanticOperationResult::ReadOnly { value: native, .. }) = result.native_result
        else {
            panic!("native read")
        };
        assert_eq!(native, value);
        let roundtrip: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&native).unwrap()).unwrap();
        assert_eq!(roundtrip, value);
    }
    #[test]
    fn denial_never_dispatches() {
        let service = service();
        let mut document = document();
        let binding = InspectionBinding::capture(&document, &service, "inspect").unwrap();
        assert_eq!(
            status(&binding.invoke(&mut document, &service, |_| false).unwrap()),
            "unauthorized"
        );
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn stale_identity_text_revision_and_language_are_rejected() {
        let service = service();
        let document = document();
        let binding = InspectionBinding::capture(&document, &service, "inspect").unwrap();
        let mut variants = vec![
            document.clone(),
            document.clone(),
            document.clone(),
            document.clone(),
        ];
        variants[0] = Document::new(DocumentId::new(8), "sample.fixture", "unsaved λ");
        variants[1]
            .apply_edit(replacement(&document, "changed"))
            .unwrap();
        variants[2]
            .apply_edit(replacement(&document, "unsaved λ"))
            .unwrap();
        let mut other = Service {
            language: "other",
            ..self::service()
        };
        other.language = "other";
        for (index, mut document) in variants.into_iter().enumerate() {
            let selected = if index == 3 { &other } else { &service };
            assert!(!binding.is_fresh(&document, selected));
            assert!(matches!(
                binding.invoke(&mut document, selected, |_| panic!("stale")),
                Err(InspectionError::SnapshotMismatch)
            ));
        }
    }
    #[test]
    fn discovery_filters_and_capture_rejects_unsupported_operations() {
        let mut service = service();
        let read = service.operations[0].clone();
        let mut write = read.clone();
        write.id = "write".into();
        write.effect = SemanticOperationEffect::ProposeChanges;
        let mut selection = read.clone();
        selection.id = "selection".into();
        selection.target = SemanticTargetRequirement::Selection;
        let mut arguments = read.clone();
        arguments.id = "arguments".into();
        arguments.arguments.push(scope_language::SemanticArgument {
            name: "optional".into(),
            kind: scope_language::SemanticArgumentKind::String,
            required: false,
            description: String::new(),
        });
        service.operations.extend([write, selection, arguments]);
        assert_eq!(discover(&service).unwrap(), vec![read]);
        for id in ["write", "selection", "arguments"] {
            assert!(matches!(
                InspectionBinding::capture(&document(), &service, id),
                Err(InspectionError::NotInspectable(_))
            ));
        }
        assert!(matches!(
            InspectionBinding::capture(&document(), &service, "unknown"),
            Err(InspectionError::UnknownOperation(_))
        ));
    }
    #[test]
    fn empty_catalog_and_removed_or_changed_effect_fail_closed() {
        let mut service = service();
        let mut document = document();
        let binding = InspectionBinding::capture(&document, &service, "inspect").unwrap();
        service.operations[0].effect = SemanticOperationEffect::ProposeChanges;
        assert!(discover(&service).unwrap().is_empty());
        assert!(matches!(
            binding.invoke(&mut document, &service, |_| panic!("wrong effect")),
            Err(InspectionError::NotInspectable(_))
        ));
        service.operations.clear();
        assert!(discover(&service).unwrap().is_empty());
        assert!(matches!(
            binding.invoke(&mut document, &service, |_| panic!("removed")),
            Err(InspectionError::UnknownOperation(_))
        ));
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn duplicate_and_empty_ids_fail_closed_even_when_filtered() {
        for id in ["inspect", ""] {
            let mut service = service();
            let mut duplicate = service.operations[0].clone();
            duplicate.id = id.into();
            duplicate.effect = SemanticOperationEffect::ProposeChanges;
            service.operations.push(duplicate);
            assert!(matches!(
                discover(&service),
                Err(InspectionError::InvalidOperationId(_))
            ));
            assert!(InspectionBinding::capture(&document(), &service, "inspect").is_err());
        }
    }
    #[test]
    fn changed_catalog_cannot_expand_authority() {
        let mut service = service();
        let mut document = document();
        let binding = InspectionBinding::capture(&document, &service, "inspect").unwrap();
        service.operations[0].description = "changed contract".into();
        assert!(matches!(
            binding.invoke(&mut document, &service, |_| panic!("changed")),
            Err(InspectionError::SnapshotMismatch)
        ));
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn blocked_feedback_is_preserved() {
        let mut service = service();
        service.outcome = Ok(SemanticOperationResult::Blocked {
            blocker: scope_language::SemanticBlocker {
                operation_id: "inspect".into(),
                category: scope_language::BlockerCategory::MissingDependency,
                explanation: "dependency unavailable".into(),
                recoverable: true,
                missing_dependencies: vec!["fixture dependency".into()],
                diagnostics: vec![],
                suggested_next_action: Some("supply dependency".into()),
            },
        });
        let mut document = document();
        let before = document.clone();
        let binding = InspectionBinding::capture(&document, &service, "inspect").unwrap();
        let result = binding.invoke(&mut document, &service, |_| true).unwrap();
        assert_eq!(status(&result), "blocked");
        assert_eq!(result.native_result.as_ref(), service.outcome.as_ref().ok());
        let ToolValue::Object(fields) = &result.result.value else {
            panic!("object")
        };
        assert_eq!(fields["message"].as_str(), Some("dependency unavailable"));
        assert_eq!(
            fields["suggested_next_action"].as_str(),
            Some("supply dependency")
        );
        assert_eq!(document, before);
    }
    #[test]
    fn invalid_contract_and_blocker_identity_never_retain_native_result() {
        for outcome in [
            SemanticOperationResult::Evaluated {
                value: serde_json::json!({"float": 1.5}),
                rationale: "unbound".into(),
                evaluation: scope_language::SemanticEvaluation {
                    outcome: scope_language::SemanticOutcome::Satisfied,
                    summary: "untrusted contract".into(),
                    diagnostics: None,
                    failed_criteria: vec![],
                    contract_id: Some("unknown".into()),
                },
            },
            SemanticOperationResult::Blocked {
                blocker: scope_language::SemanticBlocker {
                    operation_id: "wrong".into(),
                    category: scope_language::BlockerCategory::InsufficientContext,
                    explanation: "wrong identity".into(),
                    recoverable: false,
                    missing_dependencies: vec![],
                    diagnostics: vec![],
                    suggested_next_action: None,
                },
            },
        ] {
            let mut service = service();
            service.outcome = Ok(outcome);
            let mut document = document();
            let binding = InspectionBinding::capture(&document, &service, "inspect").unwrap();
            let result = binding.invoke(&mut document, &service, |_| true).unwrap();
            assert_eq!(status(&result), "internal_failure");
            assert!(result.native_result.is_none());
        }
    }
    #[test]
    fn wrong_effect_result_and_language_errors_are_structured_without_mutation() {
        for (outcome, expected) in [
            (
                Ok(SemanticOperationResult::Changes {
                    edits: vec![],
                    rationale: "not permitted".into(),
                }),
                "internal_failure",
            ),
            (
                Err(SemanticOperationError::InvalidArguments("bad".into())),
                "invalid_arguments",
            ),
            (
                Err(SemanticOperationError::InternalFailure("failure".into())),
                "internal_failure",
            ),
            (
                Err(SemanticOperationError::UnsupportedOperation(
                    "inspect".into(),
                )),
                "unsupported_operation",
            ),
        ] {
            let mut service = service();
            service.outcome = outcome;
            let mut document = document();
            let before = document.clone();
            let binding = InspectionBinding::capture(&document, &service, "inspect").unwrap();
            let result = binding.invoke(&mut document, &service, |_| true).unwrap();
            assert_eq!(status(&result), expected);
            assert!(result.native_result.is_none());
            assert_eq!(document, before);
        }
    }
}
