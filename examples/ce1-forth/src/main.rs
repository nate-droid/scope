mod script_composition;

use scope_forth::LANGUAGE;
use scope_ide::{
    agent::*,
    scratch::ScratchBuffers,
    source_edit::{AgentEditRequest, SourceEditController},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, error::Error, time::Instant};

const SOURCE: &str = ": square dup * ;\n: misleading-square dup ;\n";
const TASK: &str = "List document-local words and describe misleading-square and square with one symbolic input each. Distinguish stack observations from arithmetic proof. Do not infer behavior from names. Do not edit, propose, apply or save. Use only words and describe; do not repeat unchanged calls.";
fn names() -> Vec<String> {
    ["words", "describe"]
        .map(|s| format!("language.example.forth.{s}"))
        .to_vec()
}
fn value(v: &ToolValue) -> Value {
    match v {
        ToolValue::Null => Value::Null,
        ToolValue::Boolean(v) => json!(v),
        ToolValue::Integer(v) => json!(v),
        ToolValue::String(v) => json!(v),
        ToolValue::Array(v) => Value::Array(v.iter().map(value).collect()),
        ToolValue::Object(v) => {
            Value::Object(v.iter().map(|(k, v)| (k.clone(), value(v))).collect())
        }
    }
}
fn decisions() -> Vec<String> {
    [("language.example.forth.words", json!({"document_only":true})),
     ("language.example.forth.describe", json!({"name":"misleading-square","symbolic_inputs":1})),
     ("language.example.forth.describe", json!({"name":"square","symbolic_inputs":1}))]
        .into_iter().map(|(name,arguments)| json!({"type":"tool","name":name,"arguments":arguments}).to_string())
        .chain([json!({"type":"complete","content":"Provider observations retained; no universal arithmetic correctness claim."}).to_string()]).collect()
}
struct Recorded {
    live: Option<CellAgent>,
    fixed: Vec<String>,
    calls: Vec<Value>,
}
impl DecisionModel for Recorded {
    fn decide(&mut self, messages: &[DecisionMessage], max_tokens: u32) -> Result<String, String> {
        let started = Instant::now();
        let response = if let Some(live) = &mut self.live {
            live.decide(messages, max_tokens)
        } else {
            self.fixed
                .get(self.calls.len())
                .cloned()
                .ok_or_else(|| "fixed decisions exhausted".into())
        };
        self.calls.push(json!({"messages":messages.iter().map(|m|json!({"role":m.role,"content":m.content})).collect::<Vec<_>>(),
            "context_utf8_bytes":messages.iter().map(|m|m.content.len()).sum::<usize>(),
            "requested_max_tokens":max_tokens,"decision":response.as_ref().ok(),"error":response.as_ref().err(),"elapsed_ms":started.elapsed().as_millis()}));
        response
    }
}
// This certifies only the presence of these revision-bound provider observations.
// It neither grades conclusion prose nor upgrades conditional arithmetic to proof.
fn observations(evidence: &[InvestigationEvidence]) -> bool {
    let responses: Vec<_> = evidence.iter().map(|e| value(&e.response)).collect();
    let words = responses.iter().find(|v| v["operation_id"] == "words");
    let inventory_ok = words
        .and_then(|v| v["value"]["words"].as_array())
        .is_some_and(|ws| {
            ["square", "misleading-square"]
                .iter()
                .all(|name| ws.iter().any(|w| w["name"] == *name))
        });
    inventory_ok
        && ["square", "misleading-square"].iter().all(|name| {
            responses.iter().any(|v| {
                v["operation_id"] == "describe"
                    && v["value"]["status"] == "found"
                    && v["value"]["word"]["name"] == *name
                    && v["value"]["symbolic"]["stack_is_final"] == true
                    && v["value"]["symbolic"]["outcome"]["status"] == "constructed-conditional"
                    && v["value"]["symbolic"]["stack"]
                        == if *name == "misleading-square" {
                            json!([0, 0])
                        } else {
                            json!([1])
                        }
                    && v["value"]["symbolic"]["nodes"][0]["op"] == "input"
            })
        })
}
fn trial(
    compact: bool,
    denied: bool,
    config: Option<AgentConfig>,
) -> Result<Value, Box<dyn Error>> {
    let mut buffers = ScratchBuffers::default();
    let id = buffers.create("example.forth", SOURCE);
    let target = buffers.get(id).unwrap();
    let revision = target.version();
    let dirty = target.is_dirty();
    let mut controller = SourceEditController::default();
    let mut budgets = EditBudgets::default();
    budgets.inspections = 6;
    budgets.model_invocations = 10;
    budgets.transport.max_decisions = 10;
    budgets.transport.max_tool_calls = 8;
    budgets.transport.max_tokens_per_decision = 2000;
    let options = SemanticInvestigationOptions {
        operations: names(),
        budgets,
    };
    let discovery = SemanticDiscoveryOptions {
        initial_operations: names(),
        page_size: 2,
    };
    let mut model = Recorded {
        live: config.map(CellAgent::new).transpose()?,
        fixed: decisions(),
        calls: vec![],
    };
    let runner = if compact {
        run_semantic_investigation_with_compact_discovery
    } else {
        run_semantic_investigation_with_discovery
    };
    let mut assessed = false;
    let run = runner(
        &mut model,
        &mut controller,
        target,
        &LANGUAGE,
        AgentEditRequest::new(target, TASK),
        &options,
        &discovery,
        |a: scope_ide::tool::semantic::SemanticAuthorization<'_>| {
            !denied
                && a.source.language == "example.forth"
                && ["words", "describe"].contains(&a.operation.id.as_str())
        },
        |_: &str, evidence: &[InvestigationEvidence]| {
            assessed = observations(evidence);
            if assessed {
                InvestigationCompletion::Completed
            } else {
                InvestigationCompletion::Inconclusive
            }
        },
    );
    let unchanged =
        target.text() == SOURCE && target.version() == revision && target.is_dirty() == dirty;
    assert!(unchanged);
    assert_eq!(controller.list().count(), 0);
    let summary = run.summary();
    let mut seen = BTreeSet::new();
    let mut duplicates = 0;
    let mut tools = 0;
    let mut discoveries = 0;
    for call in &model.calls {
        if let Some(text) = call["decision"].as_str() {
            if let Ok(d) = serde_json::from_str::<Value>(text) {
                if d["type"] == "tool" {
                    tools += 1;
                    if d["name"] == "edit.discover_operations" {
                        discoveries += 1;
                    }
                    if !seen.insert(d.to_string()) {
                        duplicates += 1;
                    }
                }
            }
        }
    }
    Ok(
        json!({"arm":if compact {"compact"} else {"baseline-detailed"},"authority_denied":denied,
        "source":SOURCE,"revision":revision,"task":TASK,"grants":if denied {vec![]} else {names()},
        "budgets":format!("{budgets:?}"),"discovery":{"initial_operations":names(),"page_size":2},
        "source_unchanged":unchanged,"proposal_count":controller.list().count(),"host_observations_satisfied":assessed,
        "model_calls":run.model_invocations,"tool_decisions":tools,"discovery_decisions":discoveries,
        "duplicate_tool_decisions":duplicates,"duplicates_suppressed":summary.duplicate_calls_suppressed,
        "elapsed_ms":summary.elapsed_ms,"token_usage":null,"context_utf8_bytes_total":model.calls.iter().filter_map(|c|c["context_utf8_bytes"].as_u64()).sum::<u64>(),
        "conversation":model.calls,"outcome":format!("{:?}",run.outcome),"summary":format!("{summary:?}"),
        "edit_run":{"state":format!("{:?}",run.state),"proposal":format!("{:?}",run.proposal),
            "source_snapshot":run.source_snapshot.as_ref().map(value),"conclusion":run.conclusion,
            "discovered_operations":run.discovered_operations,"inspections":run.inspections,"evaluations":run.evaluations,
            "refinements":run.refinements,"proposal_attempts":run.proposal_attempts,"corrections":run.corrections,
            "last_result":run.last_result.as_ref().map(value),"trace":run.trace.iter().map(|e|format!("{e:?}")).collect::<Vec<_>>(),
            "evidence":run.evidence.iter().map(|e|json!({"operation":e.call.name,"arguments":value(&ToolValue::Object(e.call.arguments.clone())),"source":value(&e.source),"response":value(&e.response)})).collect::<Vec<_>>()}}),
    )
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let config = match args.as_slice() {
        [] => None,
        [mode, model, url] if mode == "--live" => {
            let mut c = AgentConfig::local(model, url);
            if let Ok(key) = std::env::var("OPENAI_API_KEY") {
                c = c.with_api_key(key);
            }
            Some(c)
        }
        _ => return Err("usage: ce1-forth [--live MODEL BASE_URL]".into()),
    };
    let runs = [false, true]
        .into_iter()
        .map(|compact| trial(compact, false, config.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"schema":"scope.ce1-forth/1","mode":if config.is_some() {"live"} else {"deterministic"},
        "assessment":"Presentation comparison, not LLM improvement. Host completion checks provider observations only; conditional symbolic evidence is not universal arithmetic proof. Tokens unavailable. UTF8 bytes sum message content only, not role/framing or tokenizer cost. Elapsed times are nondeterministic.","runs":runs,"script_composition":script_composition::demonstration(true)?})
        )?
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paired_provider_evidence() {
        let a = trial(false, false, None).unwrap();
        let b = trial(true, false, None).unwrap();
        assert_eq!(a["edit_run"]["evidence"], b["edit_run"]["evidence"]);
        assert_eq!(a["host_observations_satisfied"], true);
        assert_eq!(b["host_observations_satisfied"], true);
        assert_eq!(a["model_calls"], 4);
        assert_eq!(b["model_calls"], 4);
        assert!(
            b["context_utf8_bytes_total"].as_u64().unwrap()
                < a["context_utf8_bytes_total"].as_u64().unwrap()
        );
    }
    #[test]
    fn denied_authority_cannot_produce_evidence() {
        for compact in [false, true] {
            let r = trial(compact, true, None).unwrap();
            assert_eq!(r["source_unchanged"], true);
            assert_eq!(r["proposal_count"], 0);
            assert!(
                r["edit_run"]["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|e| e["response"]["status"] == "unauthorized")
            );
            assert_eq!(r["host_observations_satisfied"], false);
        }
    }
}
