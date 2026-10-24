//! Mock-language integration coverage; no model, filesystem, or evaluation required.
use super::semantic::SemanticToolProvider;
use super::*;
use crate::proposal::{ProposalError, ProposalId, ProposalStatus};
use crate::source_edit::{PermittedEditScope, SourceEditController, SourceId, SourceTarget};
use crate::text::{Position, TextBuffer, TextEdit, TextRange};
use scope_language::{
    LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, SemanticArgument,
    SemanticArgumentKind, SemanticEdit, SemanticOperationDescription, SemanticOperationEffect,
    SemanticOperationError, SemanticOperationRequest, SemanticOperationResult,
    SemanticTargetRequirement,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Target {
    text: TextBuffer,
    version: u64,
}
impl Default for Target {
    fn default() -> Self {
        Self {
            text: TextBuffer::new("old"),
            version: 7,
        }
    }
}
impl SourceTarget for Target {
    fn source_id(&self) -> SourceId {
        SourceId::Scratch(42)
    }
    fn text(&self) -> &str {
        self.text.as_str()
    }
    fn version(&self) -> u64 {
        self.version
    }
    fn language_id(&self) -> &str {
        "mock"
    }
    fn apply_source_edit(&mut self, edit: TextEdit) -> Result<(), ProposalError> {
        self.text.apply_edit(edit).map_err(ProposalError::Text)?;
        self.version += 1;
        Ok(())
    }
}
#[test]
fn projected_arguments_preserve_authority_schema_and_strict_validation() {
    for allow in [false, true] {
        let service = Service::default();
        let target = Target::default();
        let mut controller = SourceEditController::default();
        {
            let provider = SemanticToolProvider::new(
                &service,
                &target,
                &mut controller,
                scope(),
                Some(selection()),
                |authorization| {
                    assert_eq!(authorization.source.text, "old");
                    assert_eq!(authorization.source.version, 7);
                    assert_eq!(authorization.source.id, SourceId::Scratch(42));
                    assert_eq!(authorization.source.language, "mock");
                    assert_eq!(authorization.arguments, &json!({"name": "new"}));
                    allow
                },
            )
            .unwrap();
            assert_eq!(provider.tools()[0].inputs.len(), 3);
            let mut provider = provider.with_projected_arguments().unwrap();
            assert_eq!(provider.tools()[0].inputs.len(), 1);
            assert_eq!(provider.tools()[0].inputs[0].name, "name");
            let metadata = provider.metadata("language.mock.rename").unwrap();
            let schema = field(&metadata, "invocation_schema");
            assert_eq!(
                field(schema, "additionalProperties"),
                &ToolValue::Boolean(false)
            );
            let domain =
                || ToolCall::new("language.mock.rename").with_argument("name", "new".into());
            for key in [
                "expected_source",
                "expected_version",
                "arguments",
                "source_id",
                "language",
                "target",
                "selection",
            ] {
                let result = provider
                    .invoke(domain().with_argument(key, "override".into()))
                    .unwrap();
                assert_eq!(
                    field(&result.value, "status").as_str(),
                    Some("invalid_arguments")
                );
                assert_eq!(field(&result.value, "invocation_schema"), schema);
            }
            assert_status(
                provider.invoke(domain()).unwrap(),
                if allow { "proposal" } else { "unauthorized" },
            );
        }
        assert_eq!(service.calls.load(Ordering::SeqCst), usize::from(allow));
        assert_eq!(controller.list().count(), usize::from(allow));
    }
}

#[test]
fn projected_baseline_is_exact_and_never_recaptured() {
    use std::cell::Cell;
    struct ChangingTarget {
        text: Cell<&'static str>,
        version: Cell<u64>,
        id: Cell<u64>,
        language: Cell<&'static str>,
    }
    impl SourceTarget for ChangingTarget {
        fn source_id(&self) -> SourceId {
            SourceId::Scratch(self.id.get())
        }
        fn text(&self) -> &str {
            self.text.get()
        }
        fn version(&self) -> u64 {
            self.version.get()
        }
        fn language_id(&self) -> &str {
            self.language.get()
        }
        fn apply_source_edit(&mut self, _: TextEdit) -> Result<(), ProposalError> {
            unreachable!()
        }
    }
    for original in ["old\n", "old\r\n", "old\n\n", "old ", "o\u{301}ld"] {
        let target = ChangingTarget {
            text: Cell::new(original),
            version: Cell::new(7),
            id: Cell::new(42),
            language: Cell::new("mock"),
        };
        let service = Service::default();
        let mut controller = SourceEditController::default();
        let authorizations = Cell::new(0);
        let mut provider = SemanticToolProvider::new(
            &service,
            &target,
            &mut controller,
            scope(),
            Some(selection()),
            |authorization| {
                assert_eq!(authorization.source.text, original);
                authorizations.set(authorizations.get() + 1);
                false
            },
        )
        .unwrap()
        .with_projected_arguments()
        .unwrap();
        let domain = || ToolCall::new("language.mock.rename").with_argument("name", "new".into());
        assert_status(provider.invoke(domain()).unwrap(), "unauthorized");
        for changed in ["old", "old\n ", "óld"] {
            target.text.set(changed);
            assert_status(provider.invoke(domain()).unwrap(), "revision_conflict");
        }
        target.text.set(original);
        target.version.set(8);
        assert_status(provider.invoke(domain()).unwrap(), "revision_conflict");
        target.version.set(7);
        target.id.set(43);
        assert_status(provider.invoke(domain()).unwrap(), "revision_conflict");
        target.id.set(42);
        target.language.set("other");
        assert_status(provider.invoke(domain()).unwrap(), "revision_conflict");
        target.language.set("mock");
        assert_status(provider.invoke(domain()).unwrap(), "unauthorized");
        assert_eq!(authorizations.get(), 2);
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    }
}

fn selection() -> TextRange {
    TextRange::new(Position::new(0, 0), Position::new(0, 3))
}
fn description(effect: SemanticOperationEffect) -> SemanticOperationDescription {
    SemanticOperationDescription {
        id: "rename".into(),
        description: "Rename the selected mock symbol".into(),
        arguments: vec![SemanticArgument {
            name: "name".into(),
            kind: SemanticArgumentKind::String,
            required: true,
            description: "Replacement identifier".into(),
        }],
        effect,
        target: SemanticTargetRequirement::Selection,
        constraints: Some(json!({"identifier": true})),
    }
}
struct Service {
    calls: AtomicUsize,
    effect: SemanticOperationEffect,
    result: Result<SemanticOperationResult, SemanticOperationError>,
}
impl Default for Service {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            effect: SemanticOperationEffect::ProposeChanges,
            result: Ok(SemanticOperationResult::Changes {
                edits: vec![SemanticEdit {
                    range: selection(),
                    replacement: "new".into(),
                }],
                rationale: "Rename resolved symbol".into(),
            }),
        }
    }
}
impl LanguageService for Service {
    fn language_id(&self) -> &'static str {
        "mock"
    }
    fn language_name(&self) -> &'static str {
        "Mock"
    }
    fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
        true
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        vec![description(self.effect)]
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.operation_id, "rename");
        assert_eq!(request.document.text(), "old");
        assert_eq!(request.document.version(), 7);
        assert_eq!(request.selection, Some(selection()));
        assert_eq!(request.arguments, json!({"name": "new"}));
        self.result.clone()
    }
}
fn session<'a>(
    service: &'a dyn LanguageService,
    target: &'a Target,
    controller: &'a mut SourceEditController,
    scope: PermittedEditScope,
    selection: Option<TextRange>,
    allow: bool,
) -> AgentSession<'a> {
    let provider = SemanticToolProvider::new(
        service,
        target,
        controller,
        scope,
        selection,
        move |authorization| {
            assert_eq!(authorization.operation.id, "rename");
            assert_eq!(authorization.source.id, SourceId::Scratch(42));
            assert_eq!(authorization.arguments, &json!({"name": "new"}));
            allow
        },
    )
    .unwrap();
    AgentSession::builder().provider(provider).build().unwrap()
}
fn scope() -> PermittedEditScope {
    PermittedEditScope::Document(SourceId::Scratch(42))
}
fn call() -> ToolCall {
    ToolCall::new("language.mock.rename")
        .with_argument("expected_version", "7".into())
        .with_argument("expected_source", "old".into())
        .with_argument("arguments", ToolValue::object([("name", "new".into())]))
}
fn field<'a>(value: &'a ToolValue, name: &str) -> &'a ToolValue {
    match value {
        ToolValue::Object(fields) => fields.get(name).unwrap(),
        other => panic!("expected object, got {other:?}"),
    }
}
fn assert_status(result: ToolResult, expected: &str) {
    assert_eq!(field(&result.value, "status").as_str(), Some(expected));
}
fn assert_unchanged(target: &Target, controller: &SourceEditController) {
    assert_eq!(target.text(), "old");
    assert_eq!(target.version(), 7);
    assert_eq!(controller.list().count(), 0);
}

#[test]
fn malformed_nesting_feedback_matches_discovery_and_retry_preserves_preconditions() {
    let service = Service::default();
    let target = Target::default();
    let mut controller = SourceEditController::default();
    {
        let mut session = session(
            &service,
            &target,
            &mut controller,
            scope(),
            Some(selection()),
            true,
        );
        let catalog = session.catalog();
        let metadata = catalog.tools[0].metadata.as_ref().unwrap();
        let schema = field(metadata, "invocation_schema").clone();
        let properties = field(&schema, "properties");
        let language_properties = field(field(properties, "arguments"), "properties");
        assert_eq!(
            field(field(language_properties, "name"), "type").as_str(),
            Some("string")
        );
        assert!(matches!(properties, ToolValue::Object(fields) if !fields.contains_key("name")));
        let malformed = call().with_argument("name", "new".into());
        let rejected = session.invoke(malformed).unwrap();
        assert_eq!(
            field(&rejected.value, "status").as_str(),
            Some("invalid_arguments")
        );
        assert_eq!(field(&rejected.value, "invocation_schema"), &schema);
        assert!(
            field(&rejected.value, "message")
                .as_str()
                .unwrap()
                .contains("under arguments")
        );
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
        assert_status(
            session
                .invoke(call().with_argument("expected_version", "6".into()))
                .unwrap(),
            "revision_conflict",
        );
        assert_status(
            session
                .invoke(call().with_argument("expected_source", "different".into()))
                .unwrap(),
            "revision_conflict",
        );
        assert_status(session.invoke(call()).unwrap(), "proposal");
    }
    assert_eq!(service.calls.load(Ordering::SeqCst), 1);
    assert_eq!(controller.list().count(), 1);
    assert_eq!(target.text(), "old");
}

#[test]
fn discovery_exports_schema_and_dispatch_requires_review_before_acceptance() {
    let service = Service::default();
    let mut target = Target::default();
    let mut controller = SourceEditController::default();
    let id;
    {
        let mut session = session(
            &service,
            &target,
            &mut controller,
            scope(),
            Some(selection()),
            true,
        );
        let catalog = session.catalog();
        assert_eq!(catalog.tools.len(), 1);
        let entry = catalog.lookup("language.mock.rename").unwrap();
        assert_eq!(entry.provider.as_deref(), Some("mock"));
        assert!(
            entry
                .output_description
                .as_deref()
                .unwrap()
                .contains("proposal_id")
        );
        let metadata = entry.metadata.as_ref().unwrap();
        assert_eq!(field(metadata, "id").as_str(), Some("rename"));
        assert_eq!(field(metadata, "target").as_str(), Some("Selection"));
        assert!(matches!(field(metadata, "arguments"), ToolValue::Array(_)));
        assert_eq!(
            field(metadata, "constraints_json").as_str(),
            Some("{\"identifier\":true}")
        );
        let definition = &entry.definition;
        assert_eq!(definition.effect, ToolEffect::Write);
        assert_eq!(
            definition.description,
            description(service.effect).description
        );
        assert_eq!(definition.inputs.len(), 3);
        for (input, name, kind) in [
            (
                &definition.inputs[0],
                "expected_version",
                ToolInputKind::String,
            ),
            (
                &definition.inputs[1],
                "expected_source",
                ToolInputKind::String,
            ),
            (&definition.inputs[2], "arguments", ToolInputKind::Object),
        ] {
            assert_eq!(input.name, name);
            assert_eq!(input.kind, kind);
            assert!(input.required);
        }
        let arguments_schema: serde_json::Value =
            serde_json::from_str(&definition.inputs[2].description).unwrap();
        assert_eq!(arguments_schema["properties"]["name"]["type"], "string");
        assert_eq!(arguments_schema["additionalProperties"], false);
        assert!(matches!(catalog.to_value(), ToolValue::Object(_)));
        let result = session.invoke(call()).unwrap();
        assert_eq!(field(&result.value, "status").as_str(), Some("proposal"));
        id = ProposalId(
            field(&result.value, "proposal_id")
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
        );
    }
    assert_eq!(service.calls.load(Ordering::SeqCst), 1);
    assert_eq!(target.text(), "old");
    assert_eq!(target.version(), 7);
    let proposal = controller.get(id).unwrap();
    assert_eq!(proposal.status(), ProposalStatus::Pending);
    assert_eq!(proposal.target(), target.source_id());
    assert_eq!(proposal.expected_source(), "old");
    assert_eq!(proposal.expected_version(), 7);
    assert_eq!(proposal.rationale(), Some("Rename resolved symbol"));
    assert_eq!(controller.preview(id, &target).unwrap(), "new");
    assert_eq!(
        controller.diff(id, &target).unwrap(),
        "--- before\n+++ after\n@@ -1,1 +1,1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n"
    );
    assert_eq!(target.text(), "old");
    controller.accept(id, &mut target).unwrap();
    assert_eq!(target.text(), "new");
    assert_eq!(target.version(), 8);
    assert_eq!(
        controller.get(id).unwrap().status(),
        ProposalStatus::Applied
    );
    assert!(controller.accept(id, &mut target).is_err());
    assert_eq!(target.version(), 8);
}

#[test]
fn malformed_arguments_and_unknown_tools_never_dispatch() {
    let service = Service::default();
    let target = Target::default();
    let mut controller = SourceEditController::default();
    {
        let mut session = session(
            &service,
            &target,
            &mut controller,
            scope(),
            Some(selection()),
            true,
        );
        for arguments in [
            ToolValue::Null,
            ToolValue::array([]),
            ToolValue::object([] as [(&str, ToolValue); 0]),
            ToolValue::object([("name", 1_i64.into())]),
            ToolValue::object([("name", ToolValue::Null)]),
            ToolValue::object([("name", "new".into()), ("extra", true.into())]),
        ] {
            assert_status(
                session
                    .invoke(call().with_argument("arguments", arguments))
                    .unwrap(),
                "invalid_arguments",
            );
        }
        for key in ["expected_version", "expected_source", "arguments"] {
            let mut missing = call();
            missing.arguments.remove(key);
            assert_status(session.invoke(missing).unwrap(), "invalid_arguments");
        }
        for invalid in ["bad", "-1", "18446744073709551616"] {
            assert_status(
                session
                    .invoke(call().with_argument("expected_version", invalid.into()))
                    .unwrap(),
                "invalid_arguments",
            );
        }
        assert_status(
            session
                .invoke(call().with_argument("expected_source", true.into()))
                .unwrap(),
            "invalid_arguments",
        );
        assert_status(
            session
                .invoke(call().with_argument("extra", true.into()))
                .unwrap(),
            "invalid_arguments",
        );
        assert!(matches!(
            session.invoke(ToolCall::new("language.mock.unknown")),
            Err(ToolError::UnknownTool { .. })
        ));
    }
    assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    assert_unchanged(&target, &controller);
}

#[test]
fn scope_policy_and_stale_preconditions_reject_before_dispatch() {
    let service = Service::default();
    let target = Target::default();
    let mut controller = SourceEditController::default();
    for (permitted, allow, invocation, status) in [
        (
            PermittedEditScope::Document(SourceId::Scratch(99)),
            true,
            call(),
            "unauthorized",
        ),
        (scope(), false, call(), "unauthorized"),
        (
            scope(),
            true,
            call().with_argument("expected_version", "6".into()),
            "revision_conflict",
        ),
        (
            scope(),
            true,
            call().with_argument("expected_source", "different".into()),
            "revision_conflict",
        ),
    ] {
        let mut session = session(
            &service,
            &target,
            &mut controller,
            permitted,
            Some(selection()),
            allow,
        );
        assert_status(session.invoke(invocation).unwrap(), status);
    }
    {
        let mut session = session(
            &service,
            &target,
            &mut controller,
            scope(),
            Some(selection()),
            true,
        );
        assert!(matches!(
            session.invoke_with_policy(call(), |_| false),
            Err(ToolError::Denied { .. })
        ));
    }
    assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    assert_unchanged(&target, &controller);
}

#[test]
fn missing_and_invalid_selection_do_not_dispatch() {
    let service = Service::default();
    let target = Target::default();
    let mut controller = SourceEditController::default();
    for (selection, status) in [
        (None, "ambiguous_target"),
        (
            Some(TextRange::new(Position::new(0, 0), Position::new(0, 99))),
            "invalid_arguments",
        ),
    ] {
        let mut session = session(&service, &target, &mut controller, scope(), selection, true);
        assert_status(session.invoke(call()).unwrap(), status);
    }
    assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    assert_unchanged(&target, &controller);
}

#[test]
fn language_errors_are_structured_without_proposals() {
    for (error, status) in [
        (
            SemanticOperationError::InvalidArguments("identifier rejected".into()),
            "invalid_arguments",
        ),
        (
            SemanticOperationError::UnsupportedOperation("rename".into()),
            "unsupported_operation",
        ),
        (
            SemanticOperationError::InvalidSource(vec![]),
            "invalid_source",
        ),
        (
            SemanticOperationError::AmbiguousTarget("multiple symbols".into()),
            "ambiguous_target",
        ),
        (
            SemanticOperationError::InternalFailure("mock failure".into()),
            "internal_failure",
        ),
    ] {
        let service = Service {
            result: Err(error),
            ..Service::default()
        };
        let target = Target::default();
        let mut controller = SourceEditController::default();
        {
            let mut session = session(
                &service,
                &target,
                &mut controller,
                scope(),
                Some(selection()),
                true,
            );
            let result = session.invoke(call()).unwrap();
            if status == "invalid_source" {
                assert_eq!(
                    field(&result.value, "diagnostics"),
                    &ToolValue::Array(vec![])
                );
            }
            assert_status(result, status);
        }
        assert_eq!(service.calls.load(Ordering::SeqCst), 1);
        assert_unchanged(&target, &controller);
    }
}

#[test]
fn read_effect_returns_values_but_cannot_create_proposals() {
    for (result, status) in [
        (
            Ok(SemanticOperationResult::ReadOnly {
                value: json!({"symbol": "old"}),
                rationale: "Found symbol".into(),
            }),
            "read_only",
        ),
        (Service::default().result, "internal_failure"),
        (
            Ok(SemanticOperationResult::ReadOnly {
                value: json!(1.5),
                rationale: "Fractional result".into(),
            }),
            "internal_failure",
        ),
    ] {
        let service = Service {
            effect: SemanticOperationEffect::Read,
            result,
            ..Service::default()
        };
        let target = Target::default();
        let mut controller = SourceEditController::default();
        {
            let mut session = session(
                &service,
                &target,
                &mut controller,
                scope(),
                Some(selection()),
                true,
            );
            assert_eq!(
                session
                    .catalog()
                    .lookup("language.mock.rename")
                    .unwrap()
                    .definition
                    .effect,
                ToolEffect::Read
            );
            let result = session.invoke(call()).unwrap();
            if status == "read_only" {
                assert_eq!(
                    field(field(&result.value, "value"), "symbol").as_str(),
                    Some("old")
                );
                assert_eq!(
                    field(&result.value, "rationale").as_str(),
                    Some("Found symbol")
                );
            }
            assert_status(result, status);
        }
        assert_eq!(service.calls.load(Ordering::SeqCst), 1);
        assert_unchanged(&target, &controller);
    }
}

#[test]
fn legacy_language_with_default_hooks_exposes_no_tools() {
    struct Legacy;
    impl LanguageService for Legacy {
        fn language_id(&self) -> &'static str {
            "mock"
        }
        fn language_name(&self) -> &'static str {
            "Legacy"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            true
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            LanguageAnalysis::default()
        }
    }
    let target = Target::default();
    let mut controller = SourceEditController::default();
    {
        let mut session = session(&Legacy, &target, &mut controller, scope(), None, false);
        assert!(session.catalog().tools.is_empty());
        assert!(session.tools().is_empty());
        assert!(matches!(
            session.invoke(call()),
            Err(ToolError::UnknownTool { .. })
        ));
    }
    assert_unchanged(&target, &controller);
}
