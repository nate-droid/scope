//! Domain-specific continuation experiment; execution remains in the retained runner.
use super::*;
use scope_ide::{
    cli::{run_retained_scratch_agent_verification_observed, RetainedVerificationConfig},
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

/// Measures the adapter boundary without retaining native feedback or transport payloads.
struct H4Model {
    inner: CellAgent,
    inputs: Vec<usize>,
    outputs: Vec<usize>,
    feedback: BTreeSet<String>,
    decisions: Vec<Value>,
    claim: Option<String>,
    proposed: Option<String>,
    adapter_errors: usize,
}
impl DecisionModel for H4Model {
    fn provider_identity(&self) -> Option<String> {
        self.inner.provider_identity()
    }
    fn model_identity(&self) -> Option<String> {
        self.inner.model_identity()
    }
    fn trace_settings(&self) -> Option<Value> {
        self.inner.trace_settings()
    }
    fn decide(&mut self, messages: &[DecisionMessage], max_tokens: u32) -> Result<String, String> {
        if !self.inputs.is_empty() {
            // Kept in memory only to measure distinct feedback bytes; never written.
            if let Some(message) = messages.last() {
                self.feedback.insert(message.content.clone());
            }
        }
        self.inputs.push(
            messages
                .iter()
                .map(|m| m.role.len() + m.content.len())
                .sum(),
        );
        let response = match self.inner.decide(messages, max_tokens) {
            Ok(response) => response,
            Err(error) => {
                self.adapter_errors += 1;
                return Err(error);
            }
        };
        self.outputs.push(response.len());
        // No raw response: a model could quote the native payload in arbitrary prose.
        let parsed = serde_json::from_str::<Value>(&response).ok();
        let mut safe = json!({"json_parsed": parsed.is_some(), "response_bytes": response.len()});
        if let Some(v) = parsed {
            match v["type"].as_str() {
                Some("tool") => {
                    safe["type"] = json!("tool");
                    safe["fixture_tool"] = json!(v["name"] == TOOL);
                    if v["name"] == TOOL {
                        safe["goal_id"] = v["arguments"]["goal_id"].clone();
                        safe["candidate"] = v["arguments"]["replacement"].clone();
                    }
                }
                Some("complete") => {
                    safe["type"] = json!("complete");
                    let content = v["content"].as_str().unwrap_or("");
                    if let Ok(final_value) = serde_json::from_str::<Value>(content) {
                        self.claim = final_value["claim"]
                            .as_str()
                            .filter(|c| matches!(*c, "verified" | "unresolved" | "rejected"))
                            .map(str::to_owned);
                        self.proposed = final_value["candidate"].as_str().map(str::to_owned);
                    }
                    safe["claim"] = json!(self.claim);
                    safe["candidate"] = json!(self.proposed);
                    safe["content_bytes"] = json!(content.len());
                }
                _ => {}
            }
        }
        self.decisions.push(safe);
        Ok(response)
    }
}

fn h4_claim_agreement(
    claim: Option<&str>,
    classification: Option<VerificationClassification>,
) -> Option<bool> {
    Some(matches!(
        (claim?, classification?),
        ("verified", VerificationClassification::Verified)
            | ("rejected", VerificationClassification::Rejected)
            | ("unresolved", VerificationClassification::Inconclusive)
    ))
}

fn h4_final_candidate<'a>(
    completion: Option<&'a str>,
    decisions: &'a [Value],
    authorized: &'a [String],
) -> (Option<&'a str>, &'static str) {
    if let Some(candidate) = completion {
        return (Some(candidate), "completion");
    }
    // A tool request blocked by a budget is still a model proposal, not an execution.
    if let Some(candidate) = decisions.iter().rev().find_map(|d| {
        (d["type"] == "tool" && d["fixture_tool"] == true && d["goal_id"] == 0)
            .then(|| d["candidate"].as_str())
            .flatten()
    }) {
        return (Some(candidate), "last_model_tool_proposal");
    }
    match authorized.last() {
        Some(candidate) => (Some(candidate.as_str()), "last_authorized_tool_candidate"),
        None => (None, "unavailable"),
    }
}

/// H4 local LM Studio evaluation. Wire the CLI directly to this function.
/// H4_REPEATS defaults to 2 and must be in 1..=10.
pub fn run_real() -> Result<(), Box<dyn Error>> {
    let repeats = match std::env::var("H4_REPEATS") {
        Ok(s) => s
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=10).contains(n))
            .ok_or("H4_REPEATS must be an integer in 1..=10")?,
        Err(std::env::VarError::NotPresent) => 2,
        Err(error) => return Err(error.into()),
    };
    let started = std::time::Instant::now();
    let mut rows = Vec::new();
    for repetition in 0..repeats {
        for (scenario, initial, tight) in [
            ("positive", "refl x", false),
            ("rejection_refinement", "x", false),
            ("incomplete", "?", false),
            ("tight_recovery", "x", true),
        ] {
            // Rotate order to reduce a fixed warmup/order advantage.
            for offset in 0..3 {
                let variant = ["baseline", "H2", "H3"][(offset + repetition) % 3];
                let run_started = std::time::Instant::now();
                let limits = AgentLimits {
                    max_decisions: if tight { 3 } else { 4 },
                    max_tool_calls: 2,
                    max_malformed: 1,
                    max_tokens_per_decision: 320,
                    max_message_bytes: 65536,
                    max_conversation_bytes: 196608,
                };
                let request = format!(
                    "Evaluate hypothetical replacement for goal 0 of this unchanged Path source: {SOURCE}\nFirst call {TOOL} with goal_id 0 and replacement {initial:?}. On rejection, infer a corrected candidate and evaluate it; do not claim success without a kernel-checked result. On incomplete/inconclusive stop unresolved, not verified. Complete with content containing a JSON string object with candidate (the final proposed replacement) and claim (verified, rejected, or unresolved). This is read-only candidate evaluation, not verification of the source containing a hole. /no_think"
                );
                let mut row = json!({
                    "repetition": repetition + 1, "scenario": scenario, "variant": variant,
                    "request": request, "initial_candidate": initial,
                    "limits": {"max_decisions":limits.max_decisions,"max_tool_calls":limits.max_tool_calls,
                        "max_malformed":limits.max_malformed,"max_tokens_per_decision":limits.max_tokens_per_decision,
                        "max_message_bytes":limits.max_message_bytes,"max_conversation_bytes":limits.max_conversation_bytes},
                    "passed":false,"input_tokens":null,"output_tokens":null,
                });
                // Isolate failures so one unavailable backend/run does not discard the experiment.
                let attempt = (|| -> Result<(), Box<dyn Error>> {
                    let mut model = H4Model {
                        inner: CellAgent::new(AgentConfig::local(
                            "qwen3-30b-a3b",
                            "http://127.0.0.1:1234/v1",
                        ))?,
                        inputs: Vec::new(),
                        outputs: Vec::new(),
                        feedback: BTreeSet::new(),
                        decisions: Vec::new(),
                        claim: None,
                        proposed: None,
                        adapter_errors: 0,
                    };
                    let mut buffers = ScratchBuffers::default();
                    let id = buffers.create("language.path", SOURCE);
                    let source = buffers.get(id)?;
                    let allowed = vec![OP.to_string()];
                    let mut candidates = Vec::<String>::new();
                    let mut authorization_rejections = 0;
                    let mut authorize =
                        |a: scope_ide::tool::semantic::SemanticAuthorization<'_>| {
                            let replacement = a.arguments["replacement"].as_str();
                            let ok = a.operation.id == OP
                                && a.source.id == source.snapshot().id
                                && a.source.version == source.version()
                                && a.source.language == "language.path"
                                && a.source.text == SOURCE
                                && a.arguments["goal_id"] == 0
                                && a.arguments.as_object().is_some_and(|m| m.len() == 2)
                                && replacement.is_some_and(|s| s.len() <= 4096);
                            if ok {
                                candidates.push(replacement.unwrap().to_owned());
                            } else {
                                authorization_rejections += 1;
                            }
                            ok
                        };
                    let store = AgentTurnStore::new();
                    let mut last = None;
                    let mut callbacks = 0;
                    let mut publish_errors = 0;
                    let mut lifecycle = Vec::new();
                    let mut callback = |turn: &AgentTurn| {
                        callbacks += 1;
                        if store.publish(turn).is_err() {
                            publish_errors += 1;
                        }
                        last = Some(turn.id());
                        lifecycle.push(format!("{:?}", turn.state()));
                    };
                    let config = RetainedScratchAgentConfig {
                        trace: None,
                        allowed_operation_ids: &allowed,
                        authorize: &mut authorize,
                    };
                    let cancel = AgentCancellationToken::default();
                    let result = match variant {
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
                                projector: Box::new(|_: &SemanticOperationDescription,
                                    _: scope_ide::source_edit::SourceSnapshot<'_>,
                                    arguments: &Value,
                                    result: Option<&SemanticOperationResult>| {
                                    project_candidate(arguments, result)
                                }),
                                max_projection_bytes: 4096,
                            },
                        ),
                    };
                    row["decision_input_bytes"] = json!(model.inputs);
                    row["initial_input_bytes"] = json!(model.inputs.first());
                    row["cumulative_input_bytes"] = json!(model.inputs.iter().sum::<usize>());
                    row["decision_output_bytes"] = json!(model.outputs);
                    row["cumulative_output_bytes"] = json!(model.outputs.iter().sum::<usize>());
                    row["distinct_feedback_content_bytes"] =
                        json!(model.feedback.iter().map(String::len).sum::<usize>());
                    row["distinct_feedback_count"] = json!(model.feedback.len());
                    row["safe_decisions"] = json!(model.decisions);
                    row["model_claim"] = json!(model.claim);
                    row["adapter_errors"] = json!(model.adapter_errors);
                    row["authorized_candidates"] = json!(candidates);
                    row["authorization_rejections"] = json!(authorization_rejections);
                    row["corrections"] =
                        json!(candidates.windows(2).filter(|p| p[0] != p[1]).count());
                    row["callbacks"] = json!(callbacks);
                    row["publish_errors"] = json!(publish_errors);
                    row["lifecycle"] = json!(lifecycle);
                    let mut trusted = Vec::new();
                    if let Ok(run) = &result {
                        row["stop"] = json!(format!("{:?}", run.run.stop));
                        row["budget_exhausted"] = json!(matches!(
                            run.run.stop,
                            AgentStop::DecisionBudget
                                | AgentStop::ToolBudget
                                | AgentStop::MalformedBudget
                                | AgentStop::MessageBudget
                                | AgentStop::ConversationBudget
                        ));
                        row["fresh"] = json!(run.source_is_fresh(source));
                        for e in run.run.turn.evidence() {
                            if let AgentEvidence::Tool {
                                outcome: AgentToolOutcome::Success,
                                output: Some(output),
                                ..
                            } = e
                            {
                                let v = value(output);
                                trusted.push(if variant == "H3" {
                                    serde_json::from_value::<VerificationClassification>(
                                        v["observation"]["classification"].clone(),
                                    )?
                                } else {
                                    classify(&v["value"])
                                });
                            }
                        }
                    } else {
                        // Adapter/runner errors may contain backend payloads: retain only category.
                        row["error"] = json!("retained runner failed; backend text omitted");
                    }
                    row["trusted_classifications"] = json!(trusted);
                    row["rejections"] = json!(trusted
                        .iter()
                        .filter(|c| **c == VerificationClassification::Rejected)
                        .count());
                    row["source_unchanged"] = json!(source.text() == SOURCE);
                    if let Some(turn_id) = last {
                        let snapshot = store.get(turn_id)?.ok_or("missing published turn")?;
                        let mut reader = AgentSession::builder()
                            .provider(AgentTurnToolProvider::new(store.clone()))
                            .build()?;
                        let retrieved = reader.invoke(
                            ToolCall::new("scope.agent.get")
                                .with_argument("id", turn_id.to_string().into()),
                        )?;
                        let imported: AgentTurn = serde_json::from_value(snapshot.clone())?;
                        row["store_get_import_valid"] = json!(
                            value(&retrieved.value) == snapshot && imported.state().is_terminal()
                        );
                        row["usage"] = json!(imported.usage());
                        row["calls"] = json!(imported.usage().tool_calls);
                        row["malformed"] = json!(imported.usage().malformed);
                        // Select only typed metadata; do not retain free-text failures or tool names.
                        row["safe_evidence"] = Value::Array(imported.evidence().iter().map(|e| match e {
                            AgentEvidence::ContextAssembly { success, bytes, bounded, .. } => json!({"kind":"context_assembly","success":success,"bytes":bytes,"bounded":bounded}),
                            AgentEvidence::ContextSupplied { packet_bytes, initial_message_bytes } => json!({"kind":"context_supplied","packet_bytes":packet_bytes,"initial_message_bytes":initial_message_bytes}),
                            AgentEvidence::Decision { ordinal } => json!({"kind":"decision","ordinal":ordinal}),
                            AgentEvidence::ModelResponse { ordinal, response_bytes, .. } => json!({"kind":"model_response","ordinal":ordinal,"response_bytes":response_bytes}),
                            AgentEvidence::Malformed { ordinal } => json!({"kind":"malformed","ordinal":ordinal}),
                            AgentEvidence::Verification { request_ordinal, call_ordinal, observation } => json!({"kind":"verification","request_ordinal":request_ordinal,"call_ordinal":call_ordinal,"observation":observation}),
                            AgentEvidence::Tool { request_ordinal, ordinal, outcome, .. } => json!({"kind":"tool","request_ordinal":request_ordinal,"ordinal":ordinal,"outcome":outcome}),
                        }).collect());
                    }
                    // Prefer the explicit final proposal; never replace it with a known-good proof.
                    let (candidate, origin) = h4_final_candidate(
                        model.proposed.as_deref(),
                        &model.decisions,
                        &candidates,
                    );
                    row["final_candidate_origin"] = json!(origin);
                    row["final_candidate"] = json!(candidate);
                    let mut final_classification = None;
                    if let Some(candidate) = candidate {
                        let checked = path_scope::LANGUAGE.perform_semantic_operation(
                            SemanticOperationRequest {
                                operation_id: OP,
                                document: LanguageDocument::new(
                                    LanguagePath::new(""),
                                    SOURCE,
                                    source.version(),
                                ),
                                selection: None,
                                arguments: json!({"goal_id":0,"replacement":candidate}),
                            },
                        );
                        match checked {
                            Ok(native) => {
                                final_classification = Some(classify(native_value(Some(&native))));
                            }
                            Err(_) => {
                                row["independent_error"] =
                                    json!("Path invocation failed; native error omitted");
                            }
                        }
                    }
                    row["independent_final_validation"] = json!({"invocations":usize::from(candidate.is_some()),
                        "outside_agent_budget":true,"classification":final_classification,
                        "subject":"hypothetical candidate, not source proof"});
                    let expected = if scenario == "incomplete" {
                        VerificationClassification::Inconclusive
                    } else {
                        VerificationClassification::Verified
                    };
                    let sequence_ok = match scenario {
                        "positive" => trusted == vec![VerificationClassification::Verified],
                        "incomplete" => trusted == vec![VerificationClassification::Inconclusive],
                        _ => {
                            trusted
                                == vec![
                                    VerificationClassification::Rejected,
                                    VerificationClassification::Verified,
                                ]
                        }
                    };
                    let claim_ok = model.claim.as_deref()
                        == Some(if scenario == "incomplete" {
                            "unresolved"
                        } else {
                            "verified"
                        });
                    row["sequence_ok"] = json!(sequence_ok);
                    row["task_claim_matches_expected"] = json!(claim_ok);
                    row["model_claim_matches_verification"] = json!(h4_claim_agreement(
                        model.claim.as_deref(),
                        final_classification
                    ));
                    row["passed"] = json!(
                        row["stop"] == "Completed"
                            && row["fresh"] == true
                            && row["store_get_import_valid"] == true
                            && publish_errors == 0
                            && candidates.first().is_some_and(|c| c == initial)
                            && sequence_ok
                            && claim_ok
                            && final_classification == Some(expected)
                    );
                    Ok(())
                })();
                if attempt.is_err() {
                    row["error"] =
                        json!("evaluation setup/collection failed; native error omitted");
                }
                row["elapsed_ms"] = json!(run_started.elapsed().as_secs_f64() * 1000.0);
                rows.push(row);
                // Checkpoint every attempt, including errors, rather than losing previous runs.
                write_h4_summary(&rows, repeats, started.elapsed().as_secs_f64() * 1000.0)?;
            }
        }
    }
    println!(
        "H4: {}/{} passed; target/results/h4-summary.json",
        rows.iter().filter(|r| r["passed"] == true).count(),
        rows.len()
    );
    Ok(())
}

fn write_h4_summary(rows: &[Value], repeats: usize, elapsed_ms: f64) -> Result<(), Box<dyn Error>> {
    let mut aggregates = Vec::new();
    for variant in ["baseline", "H2", "H3"] {
        for scenario in [
            "positive",
            "rejection_refinement",
            "incomplete",
            "tight_recovery",
        ] {
            let group: Vec<_> = rows
                .iter()
                .filter(|r| r["variant"] == variant && r["scenario"] == scenario)
                .collect();
            let mut aggregate = json!({"variant":variant,"scenario":scenario,"runs":group.len(),
                "passed":group.iter().filter(|r| r["passed"] == true).count(),
                "failed":group.iter().filter(|r| r["passed"] != true).count(),
                "input_tokens":null,"output_tokens":null});
            for field in [
                "cumulative_input_bytes",
                "cumulative_output_bytes",
                "distinct_feedback_content_bytes",
                "elapsed_ms",
                "calls",
                "malformed",
                "rejections",
                "corrections",
                "adapter_errors",
            ] {
                aggregate[field] = json!({"sum":group.iter().filter_map(|r| r[field].as_f64()).sum::<f64>(),
                    "available_runs":group.iter().filter(|r| r[field].is_number()).count()});
            }
            aggregate["budget_exhaustions"] = json!(group
                .iter()
                .filter(|r| r["budget_exhausted"] == true)
                .count());
            for field in [
                "fresh",
                "source_unchanged",
                "store_get_import_valid",
                "model_claim_matches_verification",
                "task_claim_matches_expected",
            ] {
                aggregate[field] = json!({"true_runs":group.iter().filter(|r| r[field] == true).count(),
                    "false_runs":group.iter().filter(|r| r[field] == false).count(),
                    "unavailable_runs":group.iter().filter(|r| !r[field].is_boolean()).count()});
            }
            aggregate["error_runs"] = json!(group.iter().filter(|r| !r["error"].is_null()).count());
            aggregates.push(aggregate);
        }
    }
    let summary = json!({"mode":"h4-real","metric_schema_version":2,"repeats":repeats,"expected_runs":repeats*12,
        "completed_attempts":rows.len(),"elapsed_ms":elapsed_ms,"clock":"std::time::Instant (monotonic)",
        "model":"qwen3-30b-a3b","endpoint":"http://127.0.0.1:1234/v1","adapter":"CellAgent / cell-capability-openai",
        "settings":{"temperature":0.2,"max_tokens_per_decision":320,"seed":null},
        "limitations":["Token usage not exposed by DecisionModel/CellAgent: null, not estimated.",
            "Adapter exposes no seed/top-p/reasoning control; LM Studio loaded model identity and server defaults are not attested.",
            "320 tokens is a requested cap; /no_think is a prompt hint, not an enforced server setting.",
            "Byte metrics count UTF-8 role+content, not HTTP framing or tokens; distinct feedback is measured in memory only.",
            "Sequential loopback calls; latency includes runner and independent verification, not isolated inference.",
            "No raw native payloads, arbitrary model prose, or backend error bodies are retained.",
            "Without an explicit completion candidate, independently check the last model tool proposal (even if budget-blocked); otherwise use the labeled last authorized candidate.",
                        "model_claim_matches_verification measures actual claim/classification agreement (null if unavailable); task_claim_matches_expected and passed measure task expectations separately.",
            "Tight recovery has three decisions: no spare decision for a malformed-response retry.",
            "Independent verification is outside agent budgets and verifies a hypothetical candidate, never the unchanged source."],
        "aggregates":aggregates,"runs":rows});
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/results");
    std::fs::create_dir_all(&out)?;
    std::fs::write(
        out.join("h4-summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod h4_tests {
    use super::*;

    #[test]
    fn claim_agreement_is_not_task_success() {
        assert_eq!(
            h4_claim_agreement(Some("rejected"), Some(VerificationClassification::Rejected)),
            Some(true)
        );
        assert_eq!(
            h4_claim_agreement(
                Some("unresolved"),
                Some(VerificationClassification::Inconclusive)
            ),
            Some(true)
        );
        assert_eq!(
            h4_claim_agreement(Some("verified"), Some(VerificationClassification::Rejected)),
            Some(false)
        );
        assert_eq!(
            h4_claim_agreement(None, Some(VerificationClassification::Rejected)),
            None
        );
        assert_eq!(h4_claim_agreement(Some("verified"), None), None);
    }

    #[test]
    fn final_candidate_includes_budget_blocked_proposal() {
        let authorized = vec!["x".to_owned(), "fun (p : Id A x x) => p".to_owned()];
        let decisions =
            vec![json!({"type":"tool","fixture_tool":true,"goal_id":0,"candidate":"Id A x x"})];
        assert_eq!(
            h4_final_candidate(None, &decisions, &authorized),
            (Some("Id A x x"), "last_model_tool_proposal")
        );
        assert_eq!(
            h4_final_candidate(Some("refl x"), &decisions, &authorized),
            (Some("refl x"), "completion")
        );
        assert_eq!(
            h4_final_candidate(None, &[], &authorized),
            (
                Some("fun (p : Id A x x) => p"),
                "last_authorized_tool_candidate"
            )
        );
        assert_eq!(h4_final_candidate(None, &[], &[]), (None, "unavailable"));
    }

    #[test]
    fn independently_check_historical_budget_blocked_candidate() {
        let result = path_scope::LANGUAGE
            .perform_semantic_operation(SemanticOperationRequest {
                operation_id: OP,
                document: LanguageDocument::new(LanguagePath::new(""), SOURCE, 0),
                selection: None,
                arguments: json!({"goal_id":0,"replacement":"Id A x x"}),
            })
            .expect("independent historical proposal check");
        let classification = classify(native_value(Some(&result)));
        println!("historical budget-blocked proposal classification: {classification:?}");
        assert_eq!(classification, VerificationClassification::Rejected);
    }

    #[test]
    fn offline_control() {
        super::run().expect("existing H3 controls and Path independent checks");
    }
}
