//! Domain-specific continuation experiment; execution remains in the retained runner.
use super::*;
use scope_ide::{
    cli::{RetainedVerificationConfig, run_retained_scratch_agent_verification_observed},
    tool::verification::*,
};
use scope_language::{
    LanguageDocument, LanguagePath, LanguageService, SemanticOperationDescription,
    SemanticOperationRequest, SemanticOperationResult,
};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

fn safe_fields(v: Value) -> Option<ToolValue> {
    Some(match v {
        Value::Null => ToolValue::Null,
        Value::Bool(b) => ToolValue::Boolean(b),
        Value::String(s) => ToolValue::String(s),
        Value::Number(n) => ToolValue::Integer(n.as_i64()?),
        Value::Object(m) => ToolValue::Object(
            m.into_iter()
                .map(|(k, v)| Some((k, safe_fields(v)?)))
                .collect::<Option<_>>()?,
        ),
        _ => return None,
    })
}
const SOURCE: &str = "theorem trial (A : Type 0) (x : A) : Id A x x := ?\n";
const OP: &str = "evaluate_goal_candidate";
const TOOL: &str = "language.language.path.evaluate_goal_candidate";
fn classify(v: &Value) -> VerificationClassification {
    match v["classification"].as_str() {
        Some("Completed")
            if v.pointer("/candidate/outcome") == Some(&json!("Checked"))
                && v.pointer("/candidate/kernel/checked") == Some(&json!(true)) =>
        {
            VerificationClassification::Verified
        }
        Some("RejectedCandidate") => VerificationClassification::Rejected,
        _ => VerificationClassification::Inconclusive,
    }
}
fn native_value(native: Option<&SemanticOperationResult>) -> &Value {
    match native {
        Some(SemanticOperationResult::ReadOnly { value, .. }) => value,
        _ => &Value::Null,
    }
}
fn project_candidate(
    arguments: &Value,
    result: Option<&SemanticOperationResult>,
) -> VerificationSelection {
    let v = native_value(result);
    let classification = classify(v);
    let recognized = matches!(
        v["classification"].as_str(),
        Some(
            "Completed"
                | "RejectedCandidate"
                | "StillIncomplete"
                | "Inconclusive"
                | "InvalidRequest"
        )
    );
    VerificationSelection {
        classification,
        subject: VerificationSubject::HypotheticalCandidate,
        completeness: if recognized {
            VerificationCompleteness::SelectedFields
        } else {
            VerificationCompleteness::Unavailable
        },
        authority: "Path kernel/candidate evaluator".into(),
        fields: if recognized {
            safe_fields(json!({
                "classification": classification,
                "candidate_outcome": v.pointer("/candidate/outcome").and_then(Value::as_str),
                "kernel_checked": v.pointer("/candidate/kernel/checked").and_then(Value::as_bool),
                "goal_id": arguments["goal_id"].as_u64(),
            }))
            .expect("selected scalars are representable")
        } else {
            ToolValue::Null
        },
    }
}
#[derive(Default)]
struct Control {
    initial: String,
    h3: bool,
    candidate: String,
    inputs: Vec<usize>,
    feedback: BTreeSet<String>,
    observed: Vec<VerificationClassification>,
    conclusion: String,
}
impl DecisionModel for Control {
    fn decide(&mut self, messages: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.inputs.push(
            messages
                .iter()
                .map(|m| m.role.len() + m.content.len())
                .sum(),
        );
        if self.inputs.len() == 1 {
            self.candidate = self.initial.clone();
        } else {
            let message = messages.last().ok_or("missing feedback")?;
            let packet: Value =
                serde_json::from_str(&message.content).map_err(|e| e.to_string())?;
            self.feedback.insert(message.content.clone());
            let c = if self.h3 {
                match packet
                    .pointer("/value/observation/classification")
                    .and_then(Value::as_str)
                {
                    Some("Verified") => VerificationClassification::Verified,
                    Some("Rejected") => VerificationClassification::Rejected,
                    Some("Inconclusive") => VerificationClassification::Inconclusive,
                    _ => return Err("missing projected classification".into()),
                }
            } else {
                let raw = packet
                    .pointer("/value/value")
                    .ok_or("missing raw feedback")?;
                classify(raw)
            };
            self.observed.push(c);
            match c {
                VerificationClassification::Rejected => self.candidate = "refl x".into(),
                VerificationClassification::Verified | VerificationClassification::Inconclusive => {
                    self.conclusion = if c == VerificationClassification::Verified {
                        "verified"
                    } else {
                        "unresolved"
                    }
                    .into();
                    return Ok(json!({"type":"complete","content":self.conclusion}).to_string());
                }
            }
        }
        Ok(json!({"type":"tool","name":TOOL,"arguments":{"goal_id":0,"replacement":self.candidate}}).to_string())
    }
}
fn alternative_controls() -> Result<(), Box<dyn Error>> {
    for h3 in [false, true] {
        for c in [
            VerificationClassification::Rejected,
            VerificationClassification::Verified,
            VerificationClassification::Inconclusive,
        ] {
            let mut control = Control {
                initial: "x".into(),
                h3,
                ..Default::default()
            };
            control.decide(&[], 320)?;
            let raw = match c {
                VerificationClassification::Verified => {
                    json!({"classification":"Completed","candidate":{"outcome":"Checked","kernel":{"checked":true}}})
                }
                VerificationClassification::Rejected => {
                    json!({"classification":"RejectedCandidate"})
                }
                _ => json!({"classification":"StillIncomplete"}),
            };
            let packet = if h3 {
                json!({"value":{"observation":{"classification":c}}})
            } else {
                json!({"value":{"value":raw}})
            };
            let response: Value = serde_json::from_str(&control.decide(
                &[DecisionMessage {
                    role: "user".into(),
                    content: packet.to_string(),
                }],
                320,
            )?)?;
            assert_eq!(
                response["type"],
                if c == VerificationClassification::Rejected {
                    "tool"
                } else {
                    "complete"
                }
            );
            if c == VerificationClassification::Rejected {
                assert_eq!(response["arguments"]["replacement"], "refl x");
            }
        }
    }
    Ok(())
}
pub fn run() -> Result<(), Box<dyn Error>> {
    alternative_controls()?;
    let arguments = json!({"goal_id": 0});
    for result in [
        None,
        Some(SemanticOperationResult::ReadOnly {
            value: json!({"classification": "UnknownFutureResult"}),
            rationale: String::new(),
        }),
    ] {
        let selection = project_candidate(&arguments, result.as_ref());
        assert_eq!(
            selection.classification,
            VerificationClassification::Inconclusive
        );
        assert_eq!(
            selection.completeness,
            VerificationCompleteness::Unavailable
        );
        assert_eq!(selection.fields, ToolValue::Null);
    }
    let incomplete = SemanticOperationResult::ReadOnly {
        value: json!({"classification": "StillIncomplete"}),
        rationale: String::new(),
    };
    let selection = project_candidate(&arguments, Some(&incomplete));
    assert_eq!(
        selection.completeness,
        VerificationCompleteness::SelectedFields
    );
    assert_eq!(value(&selection.fields)["goal_id"], 0);
    for raw in [
        Value::Null,
        json!({"classification":"Completed"}),
        json!({"classification":"Completed","candidate":{"outcome":"Checked","kernel":{"checked":false}}}),
        json!({"classification":"StillIncomplete"}),
        json!({"classification":"Inconclusive"}),
        json!({"classification":"InvalidRequest"}),
    ] {
        assert_eq!(classify(&raw), VerificationClassification::Inconclusive);
    }
    let mut rows = Vec::new();
    for (scenario, initial) in [
        ("positive", "refl x"),
        ("refinement", "x"),
        ("incomplete", "?"),
    ] {
        for variant in ["baseline", "H2", "H3"] {
            let mut buffers = ScratchBuffers::default();
            let id = buffers.create("language.path", SOURCE);
            let source = buffers.get(id)?;
            let mut model = Control {
                initial: initial.into(),
                h3: variant == "H3",
                ..Default::default()
            };
            let allowed = vec![OP.to_string()];
            let mut authorized = 0;
            let mut authorize = |a: scope_ide::tool::semantic::SemanticAuthorization<'_>| {
                let replacement = a.arguments["replacement"].as_str();
                let ok = a.operation.id == OP
                    && a.source.id == source.snapshot().id
                    && a.source.version == source.version()
                    && a.source.language == "language.path"
                    && a.source.text == SOURCE
                    && a.arguments["goal_id"] == 0
                    && a.arguments.as_object().is_some_and(|m| m.len() == 2)
                    && matches!(replacement, Some("refl x" | "x" | "?"));
                if ok {
                    authorized += 1;
                }
                ok
            };
            let store = AgentTurnStore::new();
            let mut callbacks = 0;
            let mut last = None;
            let mut callback = |turn: &AgentTurn| {
                callbacks += 1;
                store.publish(turn).expect("append-only publication");
                last = Some(turn.id());
            };
            let native = Rc::new(RefCell::new(Vec::new()));
            let captured = native.clone();
            let projector =
                move |_: &SemanticOperationDescription,
                      _: scope_ide::source_edit::SourceSnapshot<'_>,
                      arguments: &Value,
                      result: Option<&SemanticOperationResult>| {
                    let selection = project_candidate(arguments, result);
                    captured.borrow_mut().push(selection.classification);
                    selection
                };
            let limits = AgentLimits {
                max_decisions: 4,
                max_tool_calls: 2,
                max_malformed: 1,
                max_tokens_per_decision: 320,
                max_message_bytes: 65536,
                max_conversation_bytes: 196608,
            };
            let config = RetainedScratchAgentConfig {
                trace: None,
                allowed_operation_ids: &allowed,
                authorize: &mut authorize,
            };
            let cancel = AgentCancellationToken::default();
            let request = format!(
                "Evaluate goal 0 with initial replacement {initial:?}. On rejection refine with refl x; on verified complete verified; on inconclusive complete unresolved. Read-only candidate evaluation, not a source proof. /no_think"
            );
            let run = match variant {
                "baseline" => run_retained_scratch_agent_observed(
                    source,
                    &path_scope::LANGUAGE,
                    &mut model,
                    &request,
                    limits,
                    config,
                    &cancel,
                    &mut callback,
                ),
                "H2" => run_retained_scratch_agent_semantic_observed(
                    source,
                    &path_scope::LANGUAGE,
                    &mut model,
                    &request,
                    limits,
                    config,
                    &cancel,
                    &mut callback,
                    SemanticContextOptions::default(),
                ),
                _ => run_retained_scratch_agent_verification_observed(
                    source,
                    &path_scope::LANGUAGE,
                    &mut model,
                    &request,
                    limits,
                    config,
                    &cancel,
                    &mut callback,
                    SemanticContextOptions::default(),
                    RetainedVerificationConfig {
                        allowed_operation_ids: allowed.clone(),
                        projector: Box::new(projector),
                        max_projection_bytes: 4096,
                    },
                ),
            }?;
            let mut trusted = Vec::new();
            let mut successes = 0;
            let mut verification = Vec::new();
            let mut projected_fields = Vec::new();
            for e in run.run.turn.evidence() {
                match e {
                    AgentEvidence::Tool {
                        outcome: AgentToolOutcome::Success,
                        output: Some(output),
                        ..
                    } => {
                        successes += 1;
                        let v = value(output);
                        if variant == "H3" {
                            assert_eq!(v.pointer("/fields/goal_id"), Some(&json!(0)));
                            if v.pointer("/observation/classification") == Some(&json!("Verified"))
                            {
                                assert_eq!(
                                    v.pointer("/fields/candidate_outcome"),
                                    Some(&json!("Checked"))
                                );
                            }
                            projected_fields.push(v["fields"].clone());
                        }
                        trusted.push(if variant == "H3" {
                            serde_json::from_value(
                                v.pointer("/observation/classification")
                                    .ok_or("missing trusted observation")?
                                    .clone(),
                            )?
                        } else {
                            classify(&v["value"])
                        });
                    }
                    AgentEvidence::Verification { observation, .. } => {
                        verification.push(observation.clone())
                    }
                    _ => {}
                }
            }
            if variant == "H3" {
                assert_eq!(*native.borrow(), trusted);
                assert_eq!(verification.len(), trusted.len());
            }
            assert_eq!(trusted, model.observed);
            let expected = if scenario == "refinement" {
                vec![
                    VerificationClassification::Rejected,
                    VerificationClassification::Verified,
                ]
            } else {
                vec![if scenario == "positive" {
                    VerificationClassification::Verified
                } else {
                    VerificationClassification::Inconclusive
                }]
            };
            assert_eq!(trusted, expected);
            // Independent public service invocation: outside the retained agent and its budgets.
            let final_result =
                path_scope::LANGUAGE.perform_semantic_operation(SemanticOperationRequest {
                    operation_id: OP,
                    document: LanguageDocument::new(
                        LanguagePath::new(""),
                        SOURCE,
                        source.version(),
                    ),
                    selection: None,
                    arguments: json!({"goal_id":0,"replacement":model.candidate}),
                })?;
            let final_classification = classify(native_value(Some(&final_result)));
            assert_eq!(Some(&final_classification), trusted.last());
            let turn_id = last.ok_or("missing callback")?;
            let snapshot = store.get(turn_id)?.ok_or("missing store turn")?;
            let mut reader = AgentSession::builder()
                .provider(AgentTurnToolProvider::new(store.clone()))
                .build()?;
            let retrieved = reader.invoke(
                ToolCall::new("scope.agent.get").with_argument("id", turn_id.to_string().into()),
            )?;
            assert_eq!(value(&retrieved.value), snapshot);
            let imported: AgentTurn = serde_json::from_value(snapshot)?;
            let retained: Vec<_> = imported
                .evidence()
                .iter()
                .filter_map(|e| {
                    if let AgentEvidence::Verification { observation, .. } = e {
                        Some(observation.clone())
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(verification, retained);
            assert!(imported.state().is_terminal());
            assert!(imported.evidence().iter().all(|e| !matches!(
                e,
                AgentEvidence::Tool {
                    output: Some(_),
                    ..
                }
            )));
            assert_eq!(format!("{:?}", run.run.stop), "Completed");
            assert!(run.source_is_fresh(source));
            assert_eq!(source.text(), SOURCE);
            assert_eq!(authorized, successes);
            assert_eq!(
                model.conclusion,
                if scenario == "incomplete" {
                    "unresolved"
                } else {
                    "verified"
                }
            );
            let contexts: Vec<_> = imported
                .evidence()
                .iter()
                .filter_map(|e| match e {
                    AgentEvidence::ContextSupplied {
                        packet_bytes,
                        initial_message_bytes,
                    } => Some(json!({
                        "packet_bytes": packet_bytes,
                        "initial_message_bytes": initial_message_bytes,
                    })),
                    _ => None,
                })
                .collect();
            rows.push(json!({
                "variant": variant,
                "scenario": scenario,
                "passed": true,
                "stop": "Completed",
                "conclusion": model.conclusion,
                "initial_context": contexts,
                "initial_input_bytes": model.inputs.first(),
                "decision_input_bytes": model.inputs,
                "cumulative_input_bytes": model.inputs.iter().sum::<usize>(),
                "distinct_feedback_content_bytes": model.feedback.iter().map(String::len).sum::<usize>(),
                "distinct_feedback_count": model.feedback.len(),
                "decisions": model.inputs.len(),
                "tools": successes,
                "rejections": trusted.iter().filter(|c| **c == VerificationClassification::Rejected).count(),
                "refinements": successes - 1,
                "unresolved": scenario == "incomplete",
                "budget_exhausted": false,
                "usage": imported.usage(),
                "fresh": true,
                "source_unchanged": true,
                "authorizations": authorized,
                "callbacks": callbacks,
                "store_get_import_valid": true,
                "retained_verification_count": retained.len(),
                "trusted_classifications": trusted,
                "projected_fields": projected_fields,
                "independent_final_validation": {
                    "invocations": 1,
                    "outside_agent_budget": true,
                    "classification": final_classification,
                },
            }));
        }
    }
    let summary = json!({
        "mode": "h3-control",
        "passed": true,
        "real_path_service_and_kernel": true,
        "local_model_runs": "not run",
        "alternative_feedback_controls_asserted": true,
        "limits": {
            "max_decisions": 4,
            "max_tool_calls": 2,
            "max_tokens_per_decision": 320,
            "max_message_bytes": 65536,
            "max_conversation_bytes": 196608,
        },
        "runs": rows,
    });
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/results");
    std::fs::create_dir_all(&output_dir)?;
    let output = output_dir.join("h3-control-summary.json");
    std::fs::write(output, serde_json::to_vec_pretty(&summary)?)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
