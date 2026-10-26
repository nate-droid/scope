use path_scope::LANGUAGE;
use scope_ide::{
    agent::*,
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
    tool::{semantic::*, *},
};
use scope_script::presentation::value_json;
use serde_json::{Value, json};
use std::{cell::RefCell, error::Error, rc::Rc, time::Instant};

const SOURCE: &str = include_str!("../application.path");
const ACTION: &str = "language.language.path.evaluate_goal_candidate";
const TASK: &str = "Find a kernel-checked replacement for goal 0 in the immutable source below. Evaluate candidates using evaluate_goal_candidate; never apply edits. Choose your own candidates, do not repeat unchanged evaluations. Report whether observed evidence supports completion. Feedback may be raw, compact, or withheld; withheld feedback is not evidence of rejection or success.";
fn tv(v: Value) -> ToolValue {
    match v {
        Value::Null => ToolValue::Null,
        Value::Bool(v) => v.into(),
        Value::Number(v) => v.as_i64().unwrap().into(),
        Value::String(v) => v.into(),
        Value::Array(v) => ToolValue::Array(v.into_iter().map(tv).collect()),
        Value::Object(v) => ToolValue::Object(v.into_iter().map(|(k, v)| (k, tv(v))).collect()),
    }
}
// Copy provider fields, without deriving a new classification or interpreting diagnostics.
fn compact(envelope: &Value) -> Value {
    if envelope["status"] != "read_only"
        || !envelope["value"].is_object()
        || !envelope["value"]["classification"].is_string()
    {
        return envelope.clone();
    }
    let mut out = serde_json::Map::new();
    for key in ["status", "operation_id", "rationale"] {
        if let Some(v) = envelope.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    let v = &envelope["value"];
    let mut p = serde_json::Map::new();
    for key in [
        "schema_version",
        "classification",
        "replacement",
        "goal_id",
        "selected_goal",
        "invalid_request",
        "feedback",
        "explanation",
        "application_authorized",
    ] {
        if let Some(v) = v.get(key) {
            p.insert(key.into(), v.clone());
        }
    }
    for key in ["original", "candidate"] {
        let observation = &v[key];
        if observation.is_null() {
            p.insert(key.into(), Value::Null);
            continue;
        }
        let mut fields = serde_json::Map::new();
        for field in [
            "input",
            "outcome",
            "diagnostics",
            "goals",
            "kernel",
            "limitations",
        ] {
            if let Some(v) = observation.get(field) {
                fields.insert(field.into(), v.clone());
            }
        }
        p.insert(key.into(), Value::Object(fields));
    }
    out.insert("value".into(), Value::Object(p));
    Value::Object(out)
}
type Evidence = Rc<RefCell<Vec<Value>>>;
struct Feedback<P> {
    inner: P,
    tools: Vec<ToolDefinition>,
    arm: &'static str,
    evidence: Evidence,
}
impl<P: ToolProvider> ToolProvider for Feedback<P> {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
    fn metadata(&self, name: &str) -> Option<ToolValue> {
        self.inner.metadata(name)
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let start = Instant::now();
        let result = self.inner.invoke(call.clone());
        let raw = result.as_ref().ok().map(|r| value_json(&r.value));
        let delivered = raw.as_ref().map(|r| match self.arm {
            "compact" => compact(r),
            "withheld" => json!({"feedback_withheld":true}),
            _ => r.clone(),
        });
        self.evidence.borrow_mut().push(json!({"arguments":value_json(&ToolValue::Object(call.arguments)), "name":call.name,"envelope":raw,"delivered":delivered,"error":result.as_ref().err().map(ToString::to_string),"elapsed_us":start.elapsed().as_micros()}));
        result.map(|mut r| {
            r.value = tv(delivered.unwrap());
            r
        })
    }
}
struct Recorded {
    live: Option<CellAgent>,
    fixed: Vec<String>,
    requests: Vec<Value>,
}
impl DecisionModel for Recorded {
    fn decide(&mut self, messages: &[DecisionMessage], tokens: u32) -> Result<String, String> {
        let start = Instant::now();
        let result = match &mut self.live {
            Some(m) => m.decide(messages, tokens),
            None => self
                .fixed
                .get(self.requests.len())
                .cloned()
                .ok_or("prescribed sequence exhausted".into()),
        };
        self.requests.push(json!({"messages":messages.iter().map(|m|json!({"role":m.role,"content":m.content})).collect::<Vec<_>>(),"message_content_utf8_bytes":messages.iter().map(|m|m.content.len()).sum::<usize>(),"decision":result.as_ref().ok(),"error":result.as_ref().err(),"elapsed_us":start.elapsed().as_micros()}));
        result
    }
}
fn decision(goal: u64, replacement: &str) -> String {
    json!({"type":"tool","name":ACTION,"arguments":{"goal_id":goal,"replacement":replacement}})
        .to_string()
}
fn trial(
    arm: &'static str,
    config: Option<AgentConfig>,
    prescribed: Vec<String>,
) -> Result<Value, Box<dyn Error>> {
    trial_with_limits(arm, config, prescribed, limits(), true)
}
fn limits() -> AgentLimits {
    AgentLimits {
        max_decisions: 10,
        max_tool_calls: 8,
        max_message_bytes: 200_000,
        max_conversation_bytes: 2_000_000,
        max_tokens_per_decision: 2000,
        ..AgentLimits::default()
    }
}
fn trial_with_limits(
    arm: &'static str,
    config: Option<AgentConfig>,
    prescribed: Vec<String>,
    limits: AgentLimits,
    authorize: bool,
) -> Result<Value, Box<dyn Error>> {
    let mut buffers = ScratchBuffers::default();
    let id = buffers.create("language.path", SOURCE);
    let target = buffers.get(id).unwrap();
    let snapshot = target.snapshot();
    let dirty = target.is_dirty();
    let binding = json!({"identity":format!("{:?}",snapshot.id),"language":snapshot.language,"revision":snapshot.version.to_string(),"text":snapshot.text,"goal_id":0});
    let evidence = Evidence::default();
    let mut controller = SourceEditController::default();
    let semantic_authorizations = std::cell::Cell::new(0usize);
    let provider = SemanticToolProvider::new(
        &LANGUAGE,
        target,
        &mut controller,
        PermittedEditScope::Document(snapshot.id),
        None,
        |a: SemanticAuthorization<'_>| {
            semantic_authorizations.set(semantic_authorizations.get() + 1);
            a.operation.id == "evaluate_goal_candidate"
                && a.source.id == snapshot.id
                && a.source.version == snapshot.version
                && a.source.text == SOURCE
        },
    )?
    .with_projected_arguments()?;
    let tools = provider
        .tools()
        .iter()
        .filter(|t| t.name == ACTION)
        .cloned()
        .collect();
    let mut session = AgentSession::builder()
        .provider(Feedback {
            inner: provider,
            tools,
            arm,
            evidence: evidence.clone(),
        })
        .build()?;
    let catalog = value_json(&session.catalog().to_value());
    let mut model = Recorded {
        live: config.map(CellAgent::new).transpose()?,
        fixed: prescribed,
        requests: vec![],
    };

    let task = format!("{TASK}\nSource binding: {binding}");
    let start = Instant::now();
    let run = run_session(&mut model, &mut session, &task, limits, |d, _| {
        authorize && d.effect == ToolEffect::Read
    });
    let session_result = match &run {
        Ok(r) => {
            json!({"ok":true,"decisions":r.decisions,"tool_calls":r.tool_calls,"stop":format!("{:?}",r.stop),"content":r.content})
        }
        Err(e) => json!({"ok":false,"error":e.to_string()}),
    };
    let elapsed = start.elapsed().as_micros();
    drop(session);
    let e = evidence.borrow().clone();
    let mut seen = std::collections::BTreeSet::new();
    let repeats = e
        .iter()
        .filter(|e| !seen.insert(e["arguments"].to_string()))
        .count();
    let classifications: Vec<_> = e
        .iter()
        .filter_map(|e| e["envelope"]["value"].get("classification").cloned())
        .collect();
    let completed = classifications.iter().any(|v| v == "Completed");
    let unchanged = target.text() == SOURCE
        && target.version() == snapshot.version
        && target.is_dirty() == dirty
        && controller.list().count() == 0;
    Ok(
        json!({"arm":arm,"task":task,"source_binding":binding,"catalog":catalog,"budgets":format!("{limits:?}"),"evidence":e,"evaluations":classifications.len(),"classifications":classifications,"repeated_arguments":repeats,"candidate_revisions":e.windows(2).filter(|w|w[0]["arguments"]["replacement"] != w[1]["arguments"]["replacement"]).count(),"source_revision_before":snapshot.version,"source_revision_after":target.version(),"source_unchanged":unchanged,"observed_completed":completed,"semantic_authorizations":semantic_authorizations.get(),"session_result":session_result,"failed":run.is_err(),"decisions":model.requests.len(),"tool_calls":run.as_ref().ok().map(|r|r.tool_calls),"stop":run.as_ref().ok().map(|r|format!("{:?}",r.stop)),"conclusion":run.as_ref().ok().map(|r|r.content.clone()),"elapsed_us":elapsed,"message_content_utf8_bytes_total":model.requests.iter().filter_map(|r|r["message_content_utf8_bytes"].as_u64()).sum::<u64>(),"requests":model.requests,"tokens":null}),
    )
}
fn prescribed() -> Vec<String> {
    vec![decision(0,"x"),decision(0,"f x"),json!({"type":"complete","content":"Prescribed replay ended. Host report separately assesses provider evidence; this fixed decision does not establish model reasoning."}).to_string()]
}
fn retain_trial(arm: &str, result: Result<Value, Box<dyn Error>>) -> Value {
    result.unwrap_or_else(
        |e| json!({"arm":arm,"failed":true,"setup_error":e.to_string(),"observed_completed":false}),
    )
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let config = match args.as_slice() {
        [] => None,
        [flag, model, url] if flag == "--live" => {
            let mut c = AgentConfig::local(model, url);
            if let Ok(key) = std::env::var("OPENAI_API_KEY") {
                c = c.with_api_key(key);
            }
            Some(c)
        }
        _ => return Err("usage: ce3-path [--live MODEL BASE_URL]".into()),
    };
    let mut runs = vec![];
    for (pair, order) in [
        ["baseline", "compact", "withheld"],
        ["compact", "withheld", "baseline"],
        ["withheld", "baseline", "compact"],
    ]
    .into_iter()
    .enumerate()
    {
        for arm in order {
            let mut r = retain_trial(arm, trial(arm, config.clone(), prescribed()));
            r["trial"] = json!(pair);
            runs.push(r);
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"schema":"scope.ce3-path/1","mode":if config.is_some(){"live-own-candidates-no-seed"}else{"deterministic-prescribed-x-then-f-x"},"runs":runs,"limitations":"Ordinary run_session in every arm; no cache, workflow, or new loop. Replay is prescribed, not evidence of model improvement; withheld replay also uses the same fixed candidates. Local evidence retains full envelopes, not durable citations. Requests capture model decision inputs, not terminal feedback on stops. Message-content UTF8 bytes exclude framing and are not tokens or wire bytes. Token usage unavailable. Elapsed includes local overhead; no causal latency claim. Source never applied. Live sends source/catalog/results to configured endpoint and may incur costs; no seeded candidate or prescribed live decisions. Control withholds successful provider envelopes, not transport errors. Compact omits source copies/stages but retains provider classification, identity, diagnostics, limitations and feedback."})
        )?
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn predispatch_rejections_do_not_execute_provider() {
        for bad in [
            json!({"type":"tool","name":"unknown.operation","arguments":{}}).to_string(),
            json!({"type":"tool","name":ACTION,"arguments":{"goal_id":0,"replacement":"x","unknown":true}}).to_string(),
            json!({"type":"tool","name":ACTION,"arguments":{"goal_id":"zero","replacement":"x"}}).to_string(),
        ] {
            let r = trial("compact", None, vec![bad, prescribed().pop().unwrap()]).unwrap();
            assert_eq!(r["evaluations"], 0);
            assert_eq!(r["semantic_authorizations"], 0);
            for e in r["evidence"].as_array().unwrap() {
                assert_eq!(e["envelope"]["status"], "invalid_arguments");
                assert_eq!(e["delivered"], e["envelope"]);
            }
            assert!(r["requests"][1]["messages"].as_array().unwrap().iter().any(|m| {
                serde_json::from_str::<Value>(m["content"].as_str().unwrap())
                    .is_ok_and(|feedback| feedback["error"].is_string() || feedback["value"]["status"] == "invalid_arguments")
            }));
            assert_eq!(r["source_unchanged"], true);
        }
        let r = trial_with_limits("compact", None, prescribed(), limits(), false).unwrap();
        assert_eq!(r["evidence"], json!([]));
        assert_eq!(r["evaluations"], 0);
        assert_eq!(r["semantic_authorizations"], 0);
        assert_eq!(r["source_unchanged"], true);
    }
    #[test]
    fn shared_budget_keeps_partial_evidence() {
        for arm in ["baseline", "compact", "withheld"] {
            let mut budget = limits();
            budget.max_tool_calls = 1;
            let r = trial_with_limits(arm, None, prescribed(), budget, true).unwrap();
            assert_eq!(r["stop"], "ToolBudget");
            assert_eq!(r["tool_calls"], 1);
            assert_eq!(r["evaluations"], 1);
            assert_eq!(r["classifications"], json!(["RejectedCandidate"]));
            assert_eq!(r["observed_completed"], false);
            assert_eq!(r["source_unchanged"], true);
        }
    }
    #[test]
    fn failed_model_and_setup_are_retained() {
        // Exhausting this local model produces the same DecisionModel error path as a live error.
        let r = trial("compact", None, vec![decision(0, "x")]).unwrap();
        assert_eq!(r["failed"], true);
        assert_eq!(r["session_result"]["ok"], false);
        assert!(r["session_result"]["error"].is_string());
        assert_eq!(r["evaluations"], 1);
        assert_eq!(r["decisions"], 2);
        assert_eq!(r["observed_completed"], false);
        assert_eq!(r["source_unchanged"], true);
        assert!(r["requests"][1]["error"].is_string());
        assert!(r["tool_calls"].is_null()); // No fabricated AgentRun counters on Err.
        let r = retain_trial("compact", Err("synthetic setup failure".into()));
        assert_eq!(r["failed"], true);
        assert_eq!(r["setup_error"], "synthetic setup failure");
    }
    #[test]
    fn synthetic_projection_edge_cases_not_real_provider_outcomes() {
        // Synthetic transport fixtures only: no claim Path produced Inconclusive here.
        for envelope in [
            json!({"status":"blocked","blocker":{"diagnostics":["partial"],"reason":"limit"}}),
            json!({"status":"failed","error":"synthetic failure"}),
            json!({"status":"read_only","value":null}),
            json!({"status":"read_only","value":{"partial":true,"diagnostics":["synthetic truncated report"]}}),
        ] {
            assert_eq!(compact(&envelope), envelope);
        }
        let envelope = json!({"status":"read_only","operation_id":"evaluate_goal_candidate","value":{"classification":"Inconclusive","goal_id":0,"candidate":null,"original":{"diagnostics":[{"message":"synthetic partial"}],"limitations":{"closed_proof_checked":false}},"feedback":"synthetic unavailable result","application_authorized":false}});
        let p = compact(&envelope);
        assert_eq!(p["value"]["classification"], "Inconclusive");
        assert_eq!(p["value"]["candidate"], Value::Null);
        assert_eq!(p["value"]["original"], envelope["value"]["original"]);
        assert_eq!(p["value"]["feedback"], envelope["value"]["feedback"]);
        assert_eq!(p["value"]["application_authorized"], false);
    }
    #[test]
    fn real_classifications_and_faithful_projection() {
        let decisions = vec![
            decision(999, "f x"),
            decision(0, "?"),
            decision(0, "x"),
            decision(0, "f x"),
            json!({"type":"complete","content":"done"}).to_string(),
        ];
        let r = trial("compact", None, decisions).unwrap();
        assert_eq!(
            r["classifications"],
            json!([
                "InvalidRequest",
                "StillIncomplete",
                "RejectedCandidate",
                "Completed"
            ])
        );
        assert_eq!(r["source_unchanged"], true);
        for e in r["evidence"].as_array().unwrap() {
            assert_eq!(e["delivered"], compact(&e["envelope"]));
            for key in [
                "classification",
                "selected_goal",
                "goal_id",
                "invalid_request",
                "feedback",
                "explanation",
                "application_authorized",
            ] {
                assert_eq!(e["delivered"]["value"][key], e["envelope"]["value"][key]);
            }
            for key in ["original", "candidate"] {
                for field in [
                    "input",
                    "outcome",
                    "diagnostics",
                    "goals",
                    "kernel",
                    "limitations",
                ] {
                    assert_eq!(
                        e["delivered"]["value"][key][field],
                        e["envelope"]["value"][key][field]
                    );
                }
            }
        }
        assert!(
            r["evidence"][2]["envelope"]["value"]["candidate"]["diagnostics"]
                .as_array()
                .unwrap()
                .len()
                > 0
        );
        assert_eq!(
            r["evidence"][3]["envelope"]["value"]["candidate"]["kernel"]["checked"],
            true
        );
    }
    #[test]
    fn paired_arms_and_repeat_accounting() {
        let mut runs = vec![];
        for arm in ["baseline", "compact", "withheld"] {
            runs.push(trial(arm, None, prescribed()).unwrap());
        }
        for r in &runs {
            assert_eq!(
                r["classifications"],
                json!(["RejectedCandidate", "Completed"])
            );
            assert_eq!(r["source_unchanged"], true);
            assert_eq!(r["evaluations"], 2);
            assert_eq!(r["repeated_arguments"], 0);
            assert_eq!(r["candidate_revisions"], 1);
            assert_eq!(r["catalog"], runs[0]["catalog"]);
        }
        for e in runs[0]["evidence"].as_array().unwrap() {
            assert_eq!(e["delivered"], e["envelope"]);
        }
        for e in runs[2]["evidence"].as_array().unwrap() {
            assert_eq!(e["delivered"], json!({"feedback_withheld":true}));
        }
        assert!(
            runs[1]["message_content_utf8_bytes_total"]
                .as_u64()
                .unwrap()
                < runs[0]["message_content_utf8_bytes_total"]
                    .as_u64()
                    .unwrap()
        );
        let r = trial(
            "compact",
            None,
            vec![
                decision(0, "x"),
                decision(0, "x"),
                prescribed().pop().unwrap(),
            ],
        )
        .unwrap();
        assert_eq!(r["repeated_arguments"], 1);
        assert_eq!(r["evaluations"], 2);
    }
}
