use scope_forth::LANGUAGE;
use scope_ide::{
    agent::*,
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
    tool::{semantic::*, *},
};
use scope_script::{ErrorKind, Host, Interpreter, presentation::value_json};
use serde_json::{Value, json};
use std::{cell::RefCell, error::Error, rc::Rc, time::Instant};

const SOURCE: &str =
    ": square dup * ;\n: fourth square square ;\n: misleading-fourth square dup ;\n";
const WORKFLOW: &str = "host.investigate_fourth";
const TASK: &str = "List document-local words; investigate fourth, related square, and misleading-fourth with one symbolic input each. The host explicitly selected square as a dependency from the source; no graph operation exists here. Report symbolic statuses and stacks, not arithmetic proof. Use words and describe directly or the fixed host.investigate_fourth workflow (no arguments). No edits. Do not repeat unchanged calls.";
const LIMITATIONS: &str = "Host-selected square dependency, not graph discovery. Conditional symbolic construction is not execution or universal arithmetic proof. References are indexes into this run's retained evidence array only, not durable IDs. Missing or denied observations are not success.";
const SCRIPT: &str = r#"
let words = scope.invoke("semantic", "language.example.forth.words", {document_only: true});
let fourth = scope.invoke("semantic", "language.example.forth.describe", {name: "fourth", symbolic_inputs: 1});
let square = scope.invoke("semantic", "language.example.forth.describe", {name: "square", symbolic_inputs: 1});
let misleading = scope.invoke("semantic", "language.example.forth.describe", {name: "misleading-fourth", symbolic_inputs: 1});
{total: words.value.total,
 fourth: {name: fourth.value.word.name, status: fourth.value.status, symbolic_status: fourth.value.symbolic.outcome.status, stack: fourth.value.symbolic.stack, stack_is_final: fourth.value.symbolic.stack_is_final},
 square: {name: square.value.word.name, status: square.value.status, symbolic_status: square.value.symbolic.outcome.status, stack: square.value.symbolic.stack, stack_is_final: square.value.symbolic.stack_is_final},
 misleading: {name: misleading.value.word.name, status: misleading.value.status, symbolic_status: misleading.value.symbolic.outcome.status, stack: misleading.value.symbolic.stack, stack_is_final: misleading.value.symbolic.stack_is_final}}
"#;
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
fn calls() -> Vec<ToolCall> {
    [
        ("words", json!({"document_only":true})),
        ("describe", json!({"name":"fourth","symbolic_inputs":1})),
        ("describe", json!({"name":"square","symbolic_inputs":1})),
        (
            "describe",
            json!({"name":"misleading-fourth","symbolic_inputs":1}),
        ),
    ]
    .into_iter()
    .map(|(id, args)| {
        let ToolValue::Object(arguments) = tv(args) else {
            unreachable!()
        };
        ToolCall {
            name: format!("language.example.forth.{id}"),
            arguments,
        }
    })
    .collect()
}
fn entry(call: &ToolCall, result: &Result<ToolResult, ToolError>) -> Value {
    json!({"name":call.name,"arguments":value_json(&ToolValue::Object(call.arguments.clone())),"response":result.as_ref().ok().map(|r|value_json(&r.value)),"error":result.as_ref().err().map(ToString::to_string)})
}
type Retained = Rc<RefCell<Vec<Value>>>;
// Payload retention and catalog filtering only; production owns argument projection.
struct Projected<P> {
    inner: P,
    tools: Vec<ToolDefinition>,
    retained: Retained,
}
impl<P: ToolProvider> Projected<P> {
    fn new(inner: P, retained: Retained) -> Self {
        let tools = inner
            .tools()
            .iter()
            .filter(|t| calls().iter().any(|c| c.name == t.name))
            .cloned()
            .collect();
        Self {
            inner,
            tools,
            retained,
        }
    }
}
impl<P: ToolProvider> ToolProvider for Projected<P> {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
    fn metadata(&self, name: &str) -> Option<ToolValue> {
        self.tools
            .iter()
            .any(|t| t.name == name)
            .then(|| self.inner.metadata(name))
            .flatten()
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let result = self.inner.invoke(call.clone());
        self.retained.borrow_mut().push(entry(&call, &result));
        result
    }
}
struct Forward<'a, 'b, 'c> {
    context: &'a mut AgentWorkflowContext<'b, 'c>,
    evidence: &'a Retained,
}
impl Host for Forward<'_, '_, '_> {
    fn operations(&mut self) -> Result<ToolValue, ErrorKind> {
        Ok(ToolValue::Null)
    }
    fn invoke(
        &mut self,
        provider: &str,
        action: &str,
        arguments: ToolValue,
    ) -> Result<ToolValue, ErrorKind> {
        if provider != "semantic" {
            return Err(ErrorKind::Runtime(
                "only semantic provider is installed".into(),
            ));
        }
        let ToolValue::Object(arguments) = arguments else {
            return Err(ErrorKind::Runtime("arguments must be object".into()));
        };
        let call = ToolCall {
            name: action.into(),
            arguments,
        };
        let index = self.evidence.borrow().len();
        let result = self.context.invoke(call.clone());
        let mut retained = self.evidence.borrow_mut();
        if retained.len() == index {
            retained.push(entry(&call, &result));
        } else if let Err(error) = &result {
            retained[index]["context_error"] = json!(error.to_string());
        }
        drop(retained);
        result.map(|r| r.value).map_err(ErrorKind::Tool)
    }
}
#[derive(Default)]
struct Workflow {
    source_binding: Value,
    evidence: Retained,
    projections: Vec<Value>,
}
fn projection(
    evidence: &[Value],
    start: usize,
    error: Option<String>,
    source_binding: &Value,
) -> Value {
    let observations:Vec<_>=evidence.iter().enumerate().skip(start).filter_map(|(i,e)| {
        let r=&e["response"];
        if r.is_null() {return None;}
        let v=&r["value"];
        Some(json!({"evidence_index":i,"operation":r["operation_id"],"context_error":e["context_error"],"provider_status":r["status"],"source":v["source"],"name":v["word"]["name"],"status":v["status"],"words":v["words"],"symbolic_status":v["symbolic"]["outcome"]["status"],"stack":v["symbolic"]["stack"],"stack_is_final":v["symbolic"]["stack_is_final"]}))
    }).collect();
    json!({"source":source_binding,"related_definition_selection":"square explicitly selected by host from fourth's source; no dependency graph operation", "observations":observations,"attempt_indexes":(start..evidence.len()).collect::<Vec<_>>(),"partial":!satisfied(&evidence[start..]),"script_error":error,"limitations":LIMITATIONS})
}
impl AgentWorkflow for Workflow {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            WORKFLOW,
            "Fixed read-only Script: words, describe fourth, square, misleading-fourth. No arguments. Returns local evidence indexes and conditional symbolic observations.",
        )
    }
    fn execute(
        &mut self,
        call: ToolCall,
        context: &mut AgentWorkflowContext<'_, '_>,
    ) -> Result<ToolResult, ToolError> {
        if !call.arguments.is_empty() {
            return Err(ToolError::Execution {
                tool: call.name,
                message: "fixed workflow accepts no arguments".into(),
            });
        }
        let start = self.evidence.borrow().len();
        let result = Interpreter::new().eval(
            SCRIPT,
            &mut Forward {
                context,
                evidence: &self.evidence,
            },
        );
        let mut p = projection(
            &self.evidence.borrow(),
            start,
            result.as_ref().err().map(ToString::to_string),
            &self.source_binding,
        );
        p["script_projection"] = result.as_ref().ok().map(value_json).unwrap_or(Value::Null);
        self.projections.push(p.clone());
        Ok(ToolResult::new(tv(p)))
    }
}
fn satisfied(e: &[Value]) -> bool {
    let inventory = e.iter().any(|e| {
        e["response"]["operation_id"] == "words"
            && e["context_error"].is_null()
            && e["response"]["status"] == "read_only"
            && ["square", "fourth", "misleading-fourth"].iter().all(|n| {
                e["response"]["value"]["words"]
                    .as_array()
                    .is_some_and(|ws| ws.iter().any(|w| w["name"] == *n))
            })
    });
    inventory
        && ["fourth", "square", "misleading-fourth"]
            .iter()
            .all(|name| {
                e.iter().any(|e| {
                    let v = &e["response"]["value"];
                    e["error"].is_null()
                        && e["context_error"].is_null()
                        && e["response"]["status"] == "read_only"
                        && v["symbolic"]["stack"]
                            == match *name {
                                "fourth" => json!([2]),
                                "square" => json!([1]),
                                _ => json!([1, 1]),
                            }
                        && v["word"]["name"] == *name
                        && v["status"] == "found"
                        && v["source"]["revision"] == "0"
                        && v["symbolic"]["stack_is_final"] == true
                        && v["symbolic"]["outcome"]["status"] == "constructed-conditional"
                })
            })
}
struct Recorded {
    live: Option<CellAgent>,
    fixed: Vec<String>,
    conversation: Vec<Value>,
}
impl DecisionModel for Recorded {
    fn decide(&mut self, messages: &[DecisionMessage], tokens: u32) -> Result<String, String> {
        let start = Instant::now();
        let r = if let Some(live) = &mut self.live {
            live.decide(messages, tokens)
        } else {
            self.fixed
                .get(self.conversation.len())
                .cloned()
                .ok_or("replay exhausted".into())
        };
        self.conversation.push(json!({"messages":messages.iter().map(|m|json!({"role":m.role,"content":m.content})).collect::<Vec<_>>(),"context_utf8_bytes":messages.iter().map(|m|m.content.len()).sum::<usize>(),"decision":r.as_ref().ok(),"error":r.as_ref().err(),"elapsed_us":start.elapsed().as_micros()}));
        r
    }
}
fn limits() -> AgentLimits {
    AgentLimits {
        max_decisions: 10,
        max_tool_calls: 8,
        max_message_bytes: 100_000,
        max_conversation_bytes: 1_000_000,
        max_tokens_per_decision: 2000,
        ..AgentLimits::default()
    }
}
fn trial(
    workflow_arm: bool,
    config: Option<AgentConfig>,
    deny: Option<&str>,
    limits: AgentLimits,
    override_decisions: Option<Vec<String>>,
) -> Result<Value, Box<dyn Error>> {
    let mut buffers = ScratchBuffers::default();
    let id = buffers.create("example.forth", SOURCE);
    let target = buffers.get(id).unwrap();
    let snapshot = target.snapshot();
    let revision = snapshot.version;
    let source_binding = json!({
        "identity": format!("{:?}", snapshot.id),
        "language": snapshot.language,
        "revision": snapshot.version.to_string(),
        "text": snapshot.text,
    });
    let dirty = target.is_dirty();
    let retained = Rc::new(RefCell::new(Vec::new()));
    let mut controller = SourceEditController::default();
    let provider = SemanticToolProvider::new(
        &LANGUAGE,
        target,
        &mut controller,
        PermittedEditScope::Document(snapshot.id),
        None,
        |a: SemanticAuthorization<'_>| {
            ["words", "describe"].contains(&a.operation.id.as_str())
                && a.source.version == revision
                && a.source.text == SOURCE
        },
    )?;
    let mut session = AgentSession::builder()
        .provider(Projected::new(
            provider.with_projected_arguments()?,
            retained.clone(),
        ))
        .build()?;
    let catalog = value_json(&session.catalog().to_value());
    let fixed=override_decisions.unwrap_or_else(|| {
        let cs=if workflow_arm {vec![ToolCall::new(WORKFLOW)]} else {calls()};
        cs.into_iter().map(|c|json!({"type":"tool","name":c.name,"arguments":value_json(&ToolValue::Object(c.arguments))}).to_string()).chain([json!({"type":"complete","content":"Observations retained; symbolic construction is conditional, not arithmetic proof."}).to_string()]).collect()
    });
    let mut model = Recorded {
        live: config.map(CellAgent::new).transpose()?,
        fixed,
        conversation: vec![],
    };
    let mut workflow = Workflow {
        source_binding: source_binding.clone(),
        evidence: retained.clone(),
        ..Workflow::default()
    };
    let started = Instant::now();
    let policy = |d: &ToolDefinition, c: &ToolCall| {
        d.effect == ToolEffect::Read && deny != Some(c.name.as_str())
    };
    let run = if workflow_arm {
        run_session_with_workflows(
            &mut model,
            &mut session,
            TASK,
            limits,
            policy,
            &mut [&mut workflow],
        )
    } else {
        run_session(&mut model, &mut session, TASK, limits, policy)
    }?;
    let elapsed = started.elapsed().as_micros();
    drop(session);
    let evidence = retained.borrow().clone();
    let unchanged =
        target.text() == SOURCE && target.version() == revision && target.is_dirty() == dirty;
    let p = if workflow_arm {
        workflow.projections.last().cloned().unwrap_or(Value::Null)
    } else {
        projection(&evidence, 0, None, &source_binding)
    };
    Ok(
        json!({"arm":if workflow_arm {"workflow"} else {"baseline"},"task":TASK,"source":snapshot.text,"source_binding":source_binding,"revision":revision,"grants":["language.example.forth.words","language.example.forth.describe"],"denied_tool":deny,"base_catalog":catalog,"budgets":format!("{limits:?}"),"source_unchanged":unchanged,"evidence":evidence,"projection":p,"workflow_projections":workflow.projections,"observations_satisfied":satisfied(&evidence),"decisions":run.decisions,"tool_calls":run.tool_calls,"stop":format!("{:?}",run.stop),"conclusion":run.content,"elapsed_us":elapsed,"context_utf8_bytes_total":model.conversation.iter().filter_map(|c|c["context_utf8_bytes"].as_u64()).sum::<u64>(),"conversation":model.conversation,"tokens":null}),
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
        _ => return Err("usage: ce2-forth [--live MODEL BASE_URL]".into()),
    };
    let mut runs = Vec::new();
    for pair in 0..3 {
        for arm in if pair % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let mut r = trial(arm, config.clone(), None, limits(), None)?;
            r["pair"] = json!(pair);
            r["order"] = json!(runs.len());
            runs.push(r);
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"schema":"scope.ce2-forth/1","mode":if config.is_some(){"live"}else{"deterministic-replay"},"script":SCRIPT,"runs":runs,"caveats":"Three paired trials, alternating order (odd pair count cannot perfectly balance). Identical semantic catalog, grants, task and budgets; workflow arm adds only the host workflow definition. Replay decisions are prescribed, not evidence of LLM improvement. UTF8 message-content bytes are not tokens or wire bytes. Timing includes local overhead; no causal latency claim. Tool calls include workflow outer call. No token usage supplied by this adapter. Live mode sends source/catalog/results to the configured endpoint and may incur cost. Full calls/results/errors are report-local; context.invoke exposes no ToolExecutionEvent observer, so no invented event stream. Conversations capture requests at model decisions, not terminal feedback after budget stops; baseline pre-dispatch failures appear only if included in a subsequent decision request. Projected call arguments exclude injected host preconditions; the actual snapshot binding is retained separately in source_binding and projection.source. Byte limits are checked after provider return, not allocation or retention guarantees: oversized results may already be retained. Workflow inner attempts are retained even on denial/budget errors.","limitations":LIMITATIONS})
        )?
    );
    Ok(())
}
#[cfg(test)]
mod tests;
