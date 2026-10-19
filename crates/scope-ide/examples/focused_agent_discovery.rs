//! Run: cargo run -p scope-ide --features agent --example focused_agent_discovery
//! Test: cargo test -p scope-ide --features agent --example focused_agent_discovery
#[cfg(feature = "agent")]
mod selection_description_support;

#[cfg(feature = "agent")]
mod demo {
    use super::selection_description_support::{DeclarationLanguage, SOURCE};
    use scope_ide::{
        agent::{
            AgentLimits, AgentStop, DecisionMessage, DecisionModel, read_only_policy, run_session,
        },
        scratch::ScratchBuffers,
        source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
        tool::{AgentSession, semantic::SemanticToolProvider},
    };
    use scope_language::{
        LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, Position,
        SemanticArgument, SemanticArgumentKind, SemanticOperationDescription,
        SemanticOperationEffect, SemanticOperationError, SemanticOperationRequest,
        SemanticOperationResult, SemanticTargetRequirement, TextRange,
    };
    use serde_json::{Value, json};
    use std::sync::atomic::Ordering;

    #[derive(Default)]
    struct LookupLanguage(DeclarationLanguage);
    impl LanguageService for LookupLanguage {
        fn language_id(&self) -> &'static str {
            self.0.language_id()
        }
        fn language_name(&self) -> &'static str {
            self.0.language_name()
        }
        fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
            self.0.matches_path(path)
        }
        fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
            self.0.analyze(document)
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            vec![SemanticOperationDescription {
                id: "describe_binding".into(),
                description: "Find exactly one integer declaration by name and describe it without evaluation or edits. Returns {partial, kind, name, span, literal, references, inferred_type}; literal is a decimal string, span uses zero-based UTF-8 byte columns. References are unsupported and inferred_type is unknown. Missing or duplicate names are errors.".into(),
                arguments: vec![SemanticArgument {
                    name: "name".into(), kind: SemanticArgumentKind::String, required: true,
                    description: "Exact case-sensitive declaration identifier, not source text or an expression.".into(),
                }],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            }]
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            let name = request.arguments["name"].as_str().ok_or_else(|| {
                SemanticOperationError::InvalidArguments("name must be a string".into())
            })?;
            let selection = locate(request.document.text(), name)?;
            self.0.perform_semantic_operation(SemanticOperationRequest {
                operation_id: "describe_selection",
                document: request.document,
                selection: Some(selection),
                arguments: json!({}),
            })
        }
    }

    // This language-specific sequence stays downstream: locate, select, describe.
    fn locate(source: &str, name: &str) -> Result<TextRange, SemanticOperationError> {
        let matches: Vec<_> = source
            .lines()
            .enumerate()
            .filter(|(_, line)| {
                let words: Vec<_> = line.split_whitespace().collect();
                words.first() == Some(&"let") && words.get(1) == Some(&name)
            })
            .collect();
        match matches.as_slice() {
            [(line, text)] => Ok(TextRange::new(
                Position::new(*line, 0),
                Position::new(*line, text.len()),
            )),
            _ => Err(SemanticOperationError::InvalidArguments(
                "expected exactly one matching declaration".into(),
            )),
        }
    }

    #[derive(Default)]
    struct RecordingModel {
        snapshots: Vec<Value>,
    }
    impl DecisionModel for RecordingModel {
        fn decide(&mut self, messages: &[DecisionMessage], _: u32) -> Result<String, String> {
            self.snapshots.push(json!(
                messages
                    .iter()
                    .map(|m| json!({"role":m.role,"content":m.content}))
                    .collect::<Vec<_>>()
            ));
            if self.snapshots.len() > 1 {
                return Ok(json!({"type":"complete","content":"Fixture call finished; inspect structured feedback."}).to_string());
            }
            let request: Value = serde_json::from_str(&messages[1].content).unwrap();
            let tools = request["catalog"]["tools"].as_array().unwrap();
            assert_eq!(tools.len(), 1);
            let context: Value =
                serde_json::from_str(request["request"].as_str().unwrap()).unwrap();
            Ok(
                json!({"type":"tool", "name":tools[0]["definition"]["name"], "arguments": {
                    "expected_source":context["source"], "expected_version":context["version"],
                    "arguments":{"name":"answer"}
                }})
                .to_string(),
            )
        }
    }

    pub fn run(allow: bool) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let language = LookupLanguage::default();
        let mut buffers = ScratchBuffers::default();
        buffers.create("example.declarations", SOURCE);
        let target = buffers.selected()?;
        let mut controller = SourceEditController::default();
        let mut model = RecordingModel::default();
        {
            let provider = SemanticToolProvider::new(
                &language,
                target,
                &mut controller,
                PermittedEditScope::Document(target.source_id()),
                None,
                |authorization| {
                    allow && authorization.operation.effect == SemanticOperationEffect::Read
                },
            )?;
            let mut session = AgentSession::builder().provider(provider).build()?;
            let request = json!({"task":"Describe the answer binding; do not evaluate or edit.","source":target.text(),"version":target.version().to_string()}).to_string();
            let run = run_session(
                &mut model,
                &mut session,
                &request,
                AgentLimits::default(),
                read_only_policy,
            )?;
            assert_eq!(run.stop, AgentStop::Completed);
            assert_eq!(run.tool_calls, 1);
        }
        assert_eq!(language.0.calls.load(Ordering::SeqCst), usize::from(allow));
        assert_eq!(target.text(), SOURCE);
        assert_eq!(target.version(), 0);
        assert_eq!(controller.list().count(), 0);
        Ok(model.snapshots)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use scope_ide::tool::{ToolCall, ToolValue};
        #[test]
        fn exact_agent_context_and_read_feedback() {
            let snapshots = run(true).unwrap();
            let initial: Value =
                serde_json::from_str(snapshots[0][1]["content"].as_str().unwrap()).unwrap();
            let tool = &initial["catalog"]["tools"][0];
            assert_eq!(
                tool["definition"]["name"],
                "language.example.declarations.describe_binding"
            );
            assert_eq!(tool["definition"]["effect"], "read");
            assert_eq!(tool["metadata"]["target"], "Document");
            assert_eq!(tool["metadata"]["arguments"][0]["name"], "name");
            assert!(tool["metadata"]["invocation_schema"].is_object());
            assert!(
                tool["output_description"]
                    .as_str()
                    .unwrap()
                    .contains("read_only")
            );
            let transcript = snapshots[1].to_string();
            assert!(transcript.contains("read_only"));
            assert!(transcript.contains("integer_literal_binding"));
        }
        #[test]
        fn discovery_never_authorizes_dispatch() {
            let snapshots = run(false).unwrap();
            assert!(!snapshots[1].to_string().contains("integer_literal_binding"));
            assert!(snapshots[1].to_string().contains("unauthorized"));
        }
        #[test]
        fn equivalent_lower_level_sequence() {
            let lower = DeclarationLanguage::default();
            let focused = LookupLanguage::default();
            let document = LanguageDocument::new(LanguagePath::new("sample.decl"), SOURCE, 0);
            let raw = lower
                .perform_semantic_operation(SemanticOperationRequest {
                    operation_id: "describe_selection",
                    document: document.clone(),
                    selection: Some(locate(SOURCE, "answer").unwrap()),
                    arguments: json!({}),
                })
                .unwrap();
            let wrapped = focused
                .perform_semantic_operation(SemanticOperationRequest {
                    operation_id: "describe_binding",
                    document,
                    selection: None,
                    arguments: json!({"name":"answer"}),
                })
                .unwrap();
            assert_eq!(format!("{raw:?}"), format!("{wrapped:?}"));
            assert!(locate(SOURCE, "missing").is_err());
            assert!(locate("let answer = 1\nlet answer = 2\n", "answer").is_err());
        }
        #[test]
        fn invalid_argument_is_rejected_before_provider_dispatch() {
            let language = LookupLanguage::default();
            let mut buffers = ScratchBuffers::default();
            buffers.create("example.declarations", SOURCE);
            let target = buffers.selected().unwrap();
            let mut controller = SourceEditController::default();
            {
                let provider = SemanticToolProvider::new(
                    &language,
                    target,
                    &mut controller,
                    PermittedEditScope::Document(target.source_id()),
                    None,
                    |_| panic!("invalid arguments must not reach policy"),
                )
                .unwrap();
                let mut session = AgentSession::builder().provider(provider).build().unwrap();
                let name = session.catalog().tools[0].definition.name.clone();
                let result = session
                    .invoke(
                        ToolCall::new(name)
                            .with_argument("expected_source", SOURCE.into())
                            .with_argument("expected_version", "0".into())
                            .with_argument(
                                "arguments",
                                ToolValue::object([("name", ToolValue::Integer(42))]),
                            ),
                    )
                    .unwrap();
                assert!(format!("{:?}", result.value).contains("invalid_arguments"));
            }
            assert_eq!(language.0.calls.load(Ordering::SeqCst), 0);
            assert_eq!(target.text(), SOURCE);
            assert_eq!(controller.list().count(), 0);
        }
    }
}

#[cfg(feature = "agent")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let snapshots = demo::run(true)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"model":"deterministic fixture, not an LLM", "decision_messages":snapshots})
        )?
    );
    Ok(())
}
#[cfg(not(feature = "agent"))]
fn main() {
    eprintln!("Enable --features agent to run this example.");
}
