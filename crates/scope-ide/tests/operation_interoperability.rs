#![cfg(feature = "tools")]
//! S2: a fixed two-call native consumer, not a composition framework.
use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
    tool::{
        AgentSession, ToolCall, ToolEffect, ToolResult, ToolValue, semantic::SemanticToolProvider,
    },
};
use scope_language::{
    LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, SemanticArgument,
    SemanticArgumentKind, SemanticOperationDescription, SemanticOperationEffect,
    SemanticOperationError, SemanticOperationRequest, SemanticOperationResult,
    SemanticTargetRequirement,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

const SOURCE: &str = "alpha beta alpha\n";

// No installed production language provides this deterministic dependent query pair.
// Both operations scan only the borrowed snapshot, with no I/O or editing authority.
#[derive(Default)]
struct Words {
    calls: AtomicUsize,
}
impl LanguageService for Words {
    fn language_id(&self) -> &'static str {
        "fixture.words"
    }
    fn language_name(&self) -> &'static str {
        "S2 words"
    }
    fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
        false
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        ["words", "occurrences"]
            .into_iter()
            .map(|id| SemanticOperationDescription {
                id: id.into(),
                description: match id {
                    "words" => "List document words in source order",
                    _ => "Count exact occurrences of a supplied word in this document",
                }
                .into(),
                arguments: if id == "occurrences" {
                    vec![SemanticArgument {
                        name: "word".into(),
                        kind: SemanticArgumentKind::String,
                        required: true,
                        description: "Exact word returned by words".into(),
                    }]
                } else {
                    vec![]
                },
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            })
            .collect()
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let words: Vec<_> = request.document.text().split_whitespace().collect();
        let value = match request.operation_id {
            "words" => json!({"words": words}),
            "occurrences" => {
                let word = request.arguments["word"].as_str().ok_or_else(|| {
                    SemanticOperationError::InvalidArguments("word must be a string".into())
                })?;
                json!({"word": word, "count": words.iter().filter(|w| **w == word).count()})
            }
            other => return Err(SemanticOperationError::UnsupportedOperation(other.into())),
        };
        Ok(SemanticOperationResult::ReadOnly {
            value,
            rationale: "Read supplied snapshot only".into(),
        })
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
fn call(name: &str, version: u64, source: &str, arguments: ToolValue) -> ToolCall {
    ToolCall::new(name)
        .with_argument("expected_version", version.to_string().into())
        .with_argument("expected_source", source.into())
        .with_argument("arguments", arguments)
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Success,
    Incompatible,
    Stale,
    WrongSource,
    Denied,
    WrongScope,
}

fn run(case: Case) {
    let service = Words::default();
    let mut buffers = ScratchBuffers::default();
    buffers.create(service.language_id(), SOURCE);
    // Setup only: an identical-text edit creates a genuinely older revision (0).
    // The read-only experiment starts after this, at revision 1.
    buffers
        .selected_mut()
        .unwrap()
        .apply_edit(scope_ide::text::TextEdit::replace(
            scope_ide::text::TextRange::new(
                scope_ide::text::Position::new(0, 0),
                scope_ide::text::Position::new(0, 5),
            ),
            "alpha",
        ))
        .unwrap();
    let target = buffers.selected().unwrap();
    let before = target.snapshot();
    let id = before.id;
    let version = before.version;
    let text = before.text.to_owned();
    let mut controller = SourceEditController::default();
    let policy_calls = AtomicUsize::new(0);
    {
        let scope = if matches!(case, Case::WrongScope) {
            // A scope for a different identity grants no authority over this target.
            PermittedEditScope::Document(scope_ide::source_edit::SourceId::Scratch(id.raw() + 1))
        } else {
            PermittedEditScope::Document(id)
        };
        let provider = SemanticToolProvider::new(
            &service,
            target,
            &mut controller,
            scope,
            None,
            |authorization| {
                policy_calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(authorization.source.id, id);
                assert_eq!(authorization.source.version, version);
                assert_eq!(authorization.source.text, text);
                authorization.operation.effect == SemanticOperationEffect::Read
                    && !(matches!(case, Case::Denied)
                        && authorization.operation.id == "occurrences")
            },
        )
        .unwrap();
        let mut session = AgentSession::builder().provider(provider).build().unwrap();
        let catalog = session.catalog();
        assert_eq!(catalog.tools.len(), 2);
        for entry in &catalog.tools {
            assert_eq!(entry.definition.effect, ToolEffect::Read);
            assert_eq!(
                field(entry.metadata.as_ref().unwrap(), "target").as_str(),
                Some("Document")
            );
        }
        let discover = |id: &str| {
            catalog
                .tools
                .iter()
                .find(|entry| field(entry.metadata.as_ref().unwrap(), "id").as_str() == Some(id))
                .unwrap()
                .definition
                .name
                .clone()
        };
        let first_name = discover("words");
        let second_name = discover("occurrences");
        // Capture both transport errors and structured semantic outcomes, without flattening them.
        let first = session.invoke(call(
            &first_name,
            version,
            &text,
            ToolValue::object([] as [(&str, ToolValue); 0]),
        ));
        eprintln!("{case:?} first: {first:?}");
        let first: ToolResult = first.unwrap();
        if matches!(case, Case::WrongScope) {
            assert_eq!(field(&first.value, "status").as_str(), Some("unauthorized"));
            assert_eq!(service.calls.load(Ordering::SeqCst), 0);
            assert_eq!(policy_calls.load(Ordering::SeqCst), 0);
        } else {
            assert_eq!(field(&first.value, "status").as_str(), Some("read_only"));
            let words = field(field(&first.value, "value"), "words");
            let ToolValue::Array(words) = words else {
                panic!("expected word array")
            };
            let word = if matches!(case, Case::Incompatible) {
                ToolValue::Array(words.clone())
            } else {
                words[0].clone()
            };
            let second = session.invoke(call(
                &second_name,
                if matches!(case, Case::Stale) {
                    version - 1
                } else {
                    version
                },
                if matches!(case, Case::WrongSource) {
                    "not this snapshot"
                } else {
                    &text
                },
                ToolValue::object([("word", word)]),
            ));
            eprintln!("{case:?} second: {second:?}");
            let second = second.unwrap();
            let expected = match case {
                Case::Success => "read_only",
                Case::Incompatible => "invalid_arguments",
                Case::Stale | Case::WrongSource => "revision_conflict",
                Case::Denied => "unauthorized",
                Case::WrongScope => unreachable!(),
            };
            assert_eq!(field(&second.value, "status").as_str(), Some(expected));
            if matches!(case, Case::Success) {
                assert_eq!(
                    field(field(&second.value, "value"), "word").as_str(),
                    Some("alpha")
                );
                assert_eq!(
                    field(field(&second.value, "value"), "count"),
                    &ToolValue::Integer(2)
                );
            }
            let dispatched = if matches!(case, Case::Success) { 2 } else { 1 };
            assert_eq!(service.calls.load(Ordering::SeqCst), dispatched);
            let authorized = if matches!(case, Case::Success | Case::Denied) {
                2
            } else {
                1
            };
            assert_eq!(policy_calls.load(Ordering::SeqCst), authorized);
        }
    }
    let after = target.snapshot();
    assert_eq!(after.id, id);
    assert_eq!(after.version, version);
    assert_eq!(after.text, text);
    assert_eq!(controller.list().count(), 0);
}

#[test]
fn two_discovered_reads_consume_structured_output() {
    run(Case::Success);
}
#[test]
fn incompatible_result_is_rejected_before_dispatch() {
    run(Case::Incompatible);
}
#[test]
fn stale_revision_is_rejected_before_dispatch() {
    run(Case::Stale);
}
#[test]
fn source_precondition_is_preserved() {
    run(Case::WrongSource);
}
#[test]
fn second_read_requires_authorization() {
    run(Case::Denied);
}
#[test]
fn discovery_does_not_grant_source_authority() {
    run(Case::WrongScope);
}
