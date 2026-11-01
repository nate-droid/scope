#![cfg(feature = "tools")]

#[cfg(feature = "agent")]
use scope_ide::tool::ToolProvider;
use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceId, SourceTarget},
    tool::{
        AgentSession, ToolCall, ToolEffect, ToolError, ToolValue, semantic::SemanticToolProvider,
    },
};
use scope_language::{
    LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService,
    SemanticOperationDescription, SemanticOperationEffect, SemanticOperationError,
    SemanticOperationRequest, SemanticOperationResult, SemanticTargetRequirement,
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

const SOURCE: &str = "let answer = 40 + 2\n";
const GUIDANCE: &str = "language.example.calc.authoring_guidance";

struct Calc {
    guidance: bool,
    feedback_bytes: usize,
    calls: AtomicUsize,
}
impl Default for Calc {
    fn default() -> Self {
        Self {
            guidance: true,
            feedback_bytes: 0,
            calls: AtomicUsize::new(0),
        }
    }
}
impl Calc {
    fn guidance_value(&self) -> Value {
        json!({
            "orientation": "Calc uses let bindings and integer arithmetic; no evaluation is needed to author source.",
            "examples": [{"source": "let answer = 6 * 7", "explanation": "Bind an arithmetic expression."}],
            "local_replacement": {"operation_id": "replace_expression", "instruction": "Replace only the selected expression, preserving the binding."},
            "reference": "full native guidance detail; not needed in compact model feedback".repeat(128)
        })
    }
}
impl LanguageService for Calc {
    fn language_id(&self) -> &'static str {
        "example.calc"
    }
    fn language_name(&self) -> &'static str {
        "Calc"
    }
    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("calc")
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        let mut operations = vec![SemanticOperationDescription {
            id: "replace_expression".into(),
            description: "Propose a local expression replacement; not an authoring guide.".into(),
            arguments: vec![],
            effect: SemanticOperationEffect::ProposeChanges,
            target: SemanticTargetRequirement::Selection,
            constraints: None,
        }];
        if self.guidance {
            operations.push(SemanticOperationDescription {
                id: "authoring_guidance".into(),
                description:
                    "Read Calc orientation, examples, and local replacement operation references."
                        .into(),
                arguments: vec![],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            });
        }
        operations
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        assert_eq!(request.operation_id, "authoring_guidance");
        assert_eq!(request.document.text(), SOURCE);
        assert_eq!(request.document.version(), 0);
        assert!(request.selection.is_none());
        assert_eq!(request.arguments, json!({}));
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(SemanticOperationResult::ReadOnly {
            value: self.guidance_value(),
            rationale: "Language-owned authoring information, not permission to edit.".into(),
        })
    }
    fn semantic_feedback(
        &self,
        operation_id: &str,
        result: &SemanticOperationResult,
    ) -> Option<Value> {
        assert_eq!(operation_id, "authoring_guidance");
        let SemanticOperationResult::ReadOnly { value, .. } = result else {
            panic!("expected guidance")
        };
        Some(json!({
            "orientation": value["orientation"],
            "examples": value["examples"],
            "local_replacement": value["local_replacement"],
            "completion_established": false,
            "padding": "x".repeat(self.feedback_bytes)
        }))
    }
}
fn field<'a>(value: &'a ToolValue, key: &str) -> &'a ToolValue {
    let ToolValue::Object(fields) = value else {
        panic!("expected object: {value:?}")
    };
    fields
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {value:?}"))
}
fn call(version: &str, source: &str) -> ToolCall {
    ToolCall::new(GUIDANCE)
        .with_argument("expected_version", version.into())
        .with_argument("expected_source", source.into())
        .with_argument("arguments", ToolValue::object([] as [(&str, ToolValue); 0]))
}

#[test]
fn guidance_is_discovered_and_consumed_as_an_ordinary_source_bound_read() {
    let language = Calc::default();
    let mut buffers = ScratchBuffers::default();
    buffers.create(language.language_id(), SOURCE);
    let target = buffers.selected().unwrap();
    let before = (
        target.source_id(),
        target.version(),
        target.text().to_owned(),
        target.is_dirty(),
    );
    let mut controller = SourceEditController::default();
    {
        let provider = SemanticToolProvider::new(
            &language,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            None,
            |authorization| {
                assert_eq!(authorization.source.text, SOURCE);
                assert_eq!(authorization.source.version, 0);
                authorization.operation.effect == SemanticOperationEffect::Read
            },
        )
        .unwrap();
        let mut session = AgentSession::builder().provider(provider).build().unwrap();
        let catalog = session.catalog();
        let entry = catalog.lookup(GUIDANCE).unwrap();
        assert_eq!(entry.definition.effect, ToolEffect::Read);
        let metadata = entry.metadata.as_ref().unwrap();
        assert_eq!(field(metadata, "target").as_str(), Some("Document"));
        assert_eq!(field(metadata, "arguments"), &ToolValue::Array(vec![]));
        assert_eq!(field(metadata, "purpose").as_str(), Some("Exploration"));
        let result = session.invoke(call("0", SOURCE)).unwrap();
        assert_eq!(field(&result.value, "status").as_str(), Some("read_only"));
        assert_eq!(
            field(field(&result.value, "value"), "orientation").as_str(),
            language.guidance_value()["orientation"].as_str()
        );
        assert_eq!(
            field(
                field(field(&result.value, "value"), "local_replacement"),
                "operation_id"
            )
            .as_str(),
            Some("replace_expression")
        );
    }
    assert_eq!(language.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        (
            target.source_id(),
            target.version(),
            target.text().to_owned(),
            target.is_dirty()
        ),
        before
    );
    assert_eq!(controller.list().count(), 0);
}

#[test]
fn discovery_does_not_bypass_authorization_freshness_or_argument_validation() {
    for case in [
        "policy",
        "scope",
        "session_policy",
        "revision",
        "source",
        "arguments",
    ] {
        let language = Calc::default();
        let mut buffers = ScratchBuffers::default();
        buffers.create(language.language_id(), SOURCE);
        let target = buffers.selected().unwrap();
        let dirty = target.is_dirty();
        let mut controller = SourceEditController::default();
        {
            let scope = PermittedEditScope::Document(if case == "scope" {
                SourceId::Scratch(target.id() + 1)
            } else {
                target.source_id()
            });
            let provider =
                SemanticToolProvider::new(&language, target, &mut controller, scope, None, |_| {
                    case != "policy"
                })
                .unwrap();
            let mut session = AgentSession::builder().provider(provider).build().unwrap();
            assert!(session.catalog().lookup(GUIDANCE).is_some());
            let mut invocation = call(
                if case == "revision" { "1" } else { "0" },
                if case == "source" {
                    "let answer = 0\n"
                } else {
                    SOURCE
                },
            );
            if case == "arguments" {
                invocation = invocation
                    .with_argument("arguments", ToolValue::object([("source", "other".into())]));
            }
            let result = session.invoke_with_policy(invocation, |_| case != "session_policy");
            if case == "session_policy" {
                assert!(matches!(result, Err(ToolError::Denied { .. })));
            } else {
                let expected = match case {
                    "revision" | "source" => "revision_conflict",
                    "arguments" => "invalid_arguments",
                    _ => "unauthorized",
                };
                assert_eq!(
                    field(&result.unwrap().value, "status").as_str(),
                    Some(expected),
                    "{case}"
                );
            }
        }
        assert_eq!(language.calls.load(Ordering::SeqCst), 0, "{case}");
        assert_eq!(
            (target.text(), target.version(), target.is_dirty()),
            (SOURCE, 0, dirty)
        );
        assert_eq!(controller.list().count(), 0);
    }
}

#[test]
fn language_mismatch_is_denied_and_guidance_absence_remains_compatible() {
    for mismatch in [false, true] {
        let language = Calc {
            guidance: false,
            ..Calc::default()
        };
        let mut buffers = ScratchBuffers::default();
        buffers.create(
            if mismatch {
                "other.language"
            } else {
                language.language_id()
            },
            SOURCE,
        );
        let target = buffers.selected().unwrap();
        let mut controller = SourceEditController::default();
        {
            let provider = SemanticToolProvider::new(
                &language,
                target,
                &mut controller,
                PermittedEditScope::Document(target.source_id()),
                None,
                |_| true,
            );
            if mismatch {
                assert!(matches!(provider, Err(ToolError::Denied { .. })));
            } else {
                let mut session = AgentSession::builder()
                    .provider(provider.unwrap())
                    .build()
                    .unwrap();
                let catalog = session.catalog();
                assert_eq!(catalog.tools.len(), 1);
                assert!(catalog.lookup(GUIDANCE).is_none());
                assert!(
                    catalog
                        .lookup("language.example.calc.replace_expression")
                        .is_some()
                );
                assert!(session.invoke(call("0", SOURCE)).is_err());
            }
        }
        assert_eq!(language.calls.load(Ordering::SeqCst), 0);
        assert_eq!(target.text(), SOURCE);
        assert_eq!(controller.list().count(), 0);
    }
}

#[test]
fn human_inspection_discovers_captures_and_invokes_the_same_guidance() {
    use scope_ide::{
        document::{Document, DocumentId},
        semantic_inspection::{InspectionBinding, InspectionError, discover},
        text::{Span, TextEdit},
    };

    let language = Calc::default();
    let operations = discover(&language).unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].id, "authoring_guidance");
    assert_eq!(operations[0].effect, SemanticOperationEffect::Read);
    assert_eq!(operations[0].target, SemanticTargetRequirement::Document);
    assert!(operations[0].arguments.is_empty());
    let without_guidance = Calc {
        guidance: false,
        ..Calc::default()
    };
    assert!(discover(&without_guidance).unwrap().is_empty());

    let mut document = Document::new(DocumentId::new(7), "example.calc", SOURCE);
    let before = document.clone();
    assert!(matches!(
        InspectionBinding::capture(&document, &language, "replace_expression"),
        Err(InspectionError::NotInspectable(_))
    ));
    let binding = InspectionBinding::capture(&document, &language, &operations[0].id).unwrap();
    assert_eq!(binding.source_id(), SourceId::Document(document.id()));
    assert_eq!(binding.text(), SOURCE);
    assert_eq!(binding.version(), 0);
    assert_eq!(binding.language(), language.language_id());
    assert_eq!(binding.operation(), &operations[0]);
    assert!(binding.is_fresh(&document, &language));
    assert_eq!(language.calls.load(Ordering::SeqCst), 0);

    let denied = binding.invoke(&mut document, &language, |_| false).unwrap();
    assert_eq!(
        field(&denied.result.value, "status").as_str(),
        Some("unauthorized")
    );
    assert!(denied.native_result.is_none());
    assert_eq!(language.calls.load(Ordering::SeqCst), 0);
    assert_eq!(document, before);

    let result = binding
        .invoke(&mut document, &language, |request| {
            assert_eq!(request.source.id, binding.source_id());
            assert_eq!(request.source.text, SOURCE);
            assert_eq!(request.source.version, 0);
            assert_eq!(request.operation, binding.operation());
            assert_eq!(request.arguments, &json!({}));
            true
        })
        .unwrap();
    assert_eq!(result.binding, binding);
    assert_eq!(
        field(&result.result.value, "status").as_str(),
        Some("read_only")
    );
    let Some(SemanticOperationResult::ReadOnly { value, rationale }) = result.native_result else {
        panic!("human inspection must retain native guidance")
    };
    assert_eq!(value, language.guidance_value());
    assert!(rationale.contains("not permission"));
    assert_eq!(document, before);
    assert_eq!(language.calls.load(Ordering::SeqCst), 1);

    let range = document
        .buffer()
        .range_for_span(Span::new(0, SOURCE.len()))
        .unwrap();
    document
        .apply_edit(TextEdit::replace(range, "let answer = 0\n"))
        .unwrap();
    let edited = document.clone();
    assert!(!binding.is_fresh(&document, &language));
    assert!(matches!(
        binding.invoke(&mut document, &language, |_| panic!(
            "stale capture must not reach authorization"
        )),
        Err(InspectionError::SnapshotMismatch)
    ));
    assert_eq!(document, edited);
    assert_eq!(language.calls.load(Ordering::SeqCst), 1);
}

#[cfg(feature = "agent")]
#[test]
fn compact_feedback_is_bounded_without_discarding_native_guidance() {
    for padding in [0, 8192] {
        let language = Calc {
            feedback_bytes: padding,
            ..Calc::default()
        };
        let mut buffers = ScratchBuffers::default();
        buffers.create(language.language_id(), SOURCE);
        let target = buffers.selected().unwrap();
        let mut controller = SourceEditController::default();
        {
            let mut provider = SemanticToolProvider::new(
                &language,
                target,
                &mut controller,
                PermittedEditScope::Document(target.source_id()),
                None,
                |_| true,
            )
            .unwrap();
            let result = provider.invoke(call("0", SOURCE)).unwrap();
            let Some(SemanticOperationResult::ReadOnly { value, rationale }) =
                provider.native_result()
            else {
                panic!("native guidance missing")
            };
            assert_eq!(value, &language.guidance_value());
            assert!(rationale.contains("not permission"));
            assert_eq!(
                field(field(&result.value, "value"), "reference").as_str(),
                value["reference"].as_str()
            );
            let feedback = field(&result.value, "model_feedback");
            assert!(format!("{feedback:?}").len() < 8192);
            if padding == 0 {
                assert_eq!(
                    field(feedback, "orientation").as_str(),
                    value["orientation"].as_str()
                );
                assert_eq!(
                    field(feedback, "completion_established"),
                    &ToolValue::Boolean(false)
                );
            } else {
                assert_eq!(
                    field(feedback, "feedback_status").as_str(),
                    Some("inconclusive")
                );
            }
            let rejected = provider.invoke(call("1", SOURCE)).unwrap();
            assert_eq!(
                field(&rejected.value, "status").as_str(),
                Some("revision_conflict")
            );
            assert!(provider.native_result().is_none());
        }
        assert_eq!(language.calls.load(Ordering::SeqCst), 1);
        assert_eq!(controller.list().count(), 0);
    }
}

#[cfg(feature = "agent")]
#[test]
fn deterministic_model_receives_actual_guidance_through_existing_runner() {
    use scope_ide::agent::{
        AgentLimits, AgentStop, DecisionMessage, DecisionModel, read_only_policy, run_session,
    };
    #[derive(Default)]
    struct Model {
        decisions: usize,
    }
    impl DecisionModel for Model {
        fn decide(&mut self, messages: &[DecisionMessage], _: u32) -> Result<String, String> {
            self.decisions += 1;
            if self.decisions == 1 {
                let context: Value = serde_json::from_str(&messages[1].content).unwrap();
                let tools = context["catalog"]["tools"].as_array().unwrap();
                let guidance = tools
                    .iter()
                    .find(|tool| tool["definition"]["name"] == GUIDANCE)
                    .unwrap();
                assert_eq!(guidance["metadata"]["target"], "Document");
                return Ok(json!({"type":"tool", "name":guidance["definition"]["name"], "arguments":{"expected_source":SOURCE,"expected_version":"0","arguments":{}}}).to_string());
            }
            let transcript = messages
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(transcript.contains("Calc uses let bindings"));
            assert!(transcript.contains("let answer = 6 * 7"));
            assert!(transcript.contains("replace_expression"));
            assert!(transcript.contains("model_feedback"));
            // The ordinary runner preserves the full result alongside compact presentation.
            assert!(transcript.contains("full native guidance detail"));
            Ok(json!({"type":"complete","content":"Use the supplied Calc guidance; no edit was applied."}).to_string())
        }
    }
    for (tool_budget, message_budget) in [(0, 65_536), (1, 65_536), (1, 8192)] {
        let language = Calc::default();
        let mut buffers = ScratchBuffers::default();
        buffers.create(language.language_id(), SOURCE);
        let target = buffers.selected().unwrap();
        let mut controller = SourceEditController::default();
        let mut model = Model::default();
        {
            let provider = SemanticToolProvider::new(
                &language,
                target,
                &mut controller,
                PermittedEditScope::Document(target.source_id()),
                None,
                |a| a.operation.effect == SemanticOperationEffect::Read,
            )
            .unwrap();
            let mut session = AgentSession::builder().provider(provider).build().unwrap();
            let run = run_session(
                &mut model,
                &mut session,
                "Read authoring guidance before suggesting Calc source.",
                AgentLimits {
                    max_tool_calls: tool_budget,
                    max_message_bytes: message_budget,
                    ..AgentLimits::default()
                },
                read_only_policy,
            )
            .unwrap();
            assert_eq!(
                run.stop,
                if tool_budget == 0 {
                    AgentStop::ToolBudget
                } else if message_budget == 8192 {
                    AgentStop::MessageBudget
                } else {
                    AgentStop::Completed
                }
            );
            assert_eq!(run.tool_calls, tool_budget);
        }
        if message_budget == 8192 {
            // Compact presentation fits, but the full tool result does not. The
            // runner must stop rather than truncate it or deliver only the projection.
            let native = SemanticOperationResult::ReadOnly {
                value: language.guidance_value(),
                rationale: String::new(),
            };
            assert!(language.guidance_value().to_string().len() > message_budget);
            assert!(
                language
                    .semantic_feedback("authoring_guidance", &native)
                    .unwrap()
                    .to_string()
                    .len()
                    < message_budget
            );
            assert_eq!(
                model.decisions, 1,
                "oversized feedback must never reach another decision"
            );
        } else {
            assert_eq!(model.decisions, tool_budget + 1);
        }
        assert_eq!(language.calls.load(Ordering::SeqCst), tool_budget);
        assert_eq!((target.text(), target.version()), (SOURCE, 0));
        assert_eq!(controller.list().count(), 0);
    }
}
