use scope_ide::{
    agent::*,
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
    tool::{semantic::*, *},
};
use scope_script::{presentation::value_json, ErrorKind, Host, Interpreter};
use serde_json::{json, Value};

const WORKFLOW: &str = "host.ce4.compose";
const INNER: [&str; 3] = [
    "host.ce4.evaluate_candidate",
    "host.ce4.church",
    "host.ce4.check_source",
];
const ADAPTER_CONTRACT: &str = "host.ce4.church-adapter.v1";
const CHECK: &str = "fixed Church add two two equals encoded observed integer, by reflexivity";
const SCRIPT: &str = r#"
let produced = scope.invoke("forth", "evaluate_candidate", {});
let adapted = scope.invoke("adapter", "church", {evidence: produced});
let checked = scope.invoke("path", "check_source", {source: adapted});
checked
"#;
const PREFIX: &str = "def two (A : Type 0) (f : Pi (a : A). A) (x : A) : A := f (f x)\ndef add (m : Pi (A : Type 0). Pi (f : Pi (a : A). A). Pi (x : A). A) (n : Pi (B : Type 0). Pi (g : Pi (b : B). B). Pi (y : B). B) (C : Type 0) (h : Pi (c : C). C) (z : C) : C := m C h (n C h z)\n";
fn tv(v: Value) -> ToolValue {
    match v {
        Value::Null => ToolValue::Null,
        Value::Bool(x) => x.into(),
        Value::Number(x) => x.as_i64().unwrap().into(),
        Value::String(x) => x.into(),
        Value::Array(x) => ToolValue::Array(x.into_iter().map(tv).collect()),
        Value::Object(x) => ToolValue::Object(x.into_iter().map(|(k, v)| (k, tv(v))).collect()),
    }
}
fn encode(n: i64) -> Result<String, String> {
    if !(0..=8).contains(&n) {
        return Err("adapter requires one integer in 0..=8".into());
    }
    let mut term = "x".to_string();
    for _ in 0..n {
        term = format!("f ({term})");
    }
    Ok(format!(
        "{PREFIX}theorem result (A : Type 0) (f : Pi (a : A). A) (x : A) : Id A (add two two A f x) ({term}) := refl (add two two A f x)"
    ))
}
#[derive(Clone)]
struct Config {
    candidate: &'static str,
    expected: Vec<i64>,
    grants: [bool; 3],
    calls: usize,
    bytes: usize,
    simulated_authorization_denial: bool,
    simulated_incomplete: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            candidate: ": result 2 2 + ;",
            expected: vec![4],
            grants: [true; 3],
            calls: 3,
            bytes: 1_000_000,
            simulated_authorization_denial: false,
            simulated_incomplete: false,
        }
    }
}
struct ReportHost {
    config: Config,
    evidence: Vec<Value>,
    snapshots: Vec<Value>,
    calls: usize,
    bytes: usize,
}
impl ReportHost {
    fn semantic(&mut self, forth: bool, text: &str, args: Value) -> Result<Value, String> {
        let mut buffers = ScratchBuffers::default();
        let id = buffers.create(
            if forth {
                "example.forth"
            } else {
                "language.path"
            },
            text,
        );
        let target = buffers.get(id).unwrap();
        let s = target.snapshot();
        let revision = s.version;
        let source_index = self.snapshots.len();
        let attempt = self.evidence.len() - 1;
        self.evidence[attempt]["source_ref"] = json!(format!("source:{source_index}"));
        self.evidence[attempt]["source_binding"] = json!({"source_ref":format!("source:{source_index}"),"actual_revision":s.version.to_string(),"operation_bound":true,"authorization_policy":"exact operation, source identity, actual revision, and text; optional simulated denial"});
        self.evidence[attempt]["semantic_arguments"] = args.clone();
        self.snapshots.push(json!({"source_ref":format!("source:{source_index}"),"qualified_identity":format!("source:{source_index}/{:?}",s.id),"bound_attempt":format!("attempt:{attempt}"),"identity":format!("{:?}",s.id),"language":s.language,"revision":s.version.to_string(),"text":s.text}));
        let mut controller = SourceEditController::default();
        let mismatch = self.config.simulated_authorization_denial;
        let op = if forth {
            "evaluate_candidate"
        } else {
            "check_source"
        };
        let language: &dyn scope_language::LanguageService = if forth {
            &scope_forth::LANGUAGE
        } else {
            &path_scope::LANGUAGE
        };
        let provider = SemanticToolProvider::new(
            language,
            target,
            &mut controller,
            PermittedEditScope::Document(s.id),
            None,
            move |a: SemanticAuthorization<'_>| {
                a.operation.id == op
                    && a.source.id == s.id
                    && a.source.version == revision
                    && a.source.text == text
                    && !mismatch
            },
        )
        .map_err(|e| e.to_string())?;
        let mut provider = provider
            .with_projected_arguments()
            .map_err(|e| e.to_string())?;
        let ToolValue::Object(arguments) = tv(args) else {
            unreachable!()
        };
        let name = if forth {
            "language.example.forth.evaluate_candidate"
        } else {
            "language.language.path.check_source"
        };
        self.evidence[attempt]["full_operation"] = json!(name);
        provider
            .invoke(ToolCall {
                name: name.into(),
                arguments,
            })
            .map(|r| value_json(&r.value))
            .map_err(|e| e.to_string())
    }
    fn dispatch(&mut self, p: &str, a: &str, args: &Value) -> Result<Value, String> {
        match (p,a) {
            ("forth","evaluate_candidate")=>self.semantic(true,"",json!({"source":self.config.candidate,"tests":[{"input":[],"expected":self.config.expected}]})),
            ("adapter","church")=>{
                let e=&args["evidence"];
                if self.config.simulated_incomplete { return Err("simulated host fault: incomplete producer evidence".into()); }
                let producer = self.evidence.iter().rev().skip(1).next().ok_or("missing retained producer evidence")?;
                                if producer["provider"] != "forth" || producer["state"] != "completed" || &producer["returned"] != e {
                                    return Err("adapter evidence is not the completed preceding producer result".into());
                                }
                                let v=&e["value"];
                let tests=v["tests"].as_array().ok_or("producer missing tests")?;
                if tests.len()!=1 || tests[0]["outcome"]!="passed" || e["status"]!="read_only" {return Err("producer not a completed passing observation".into());}
                let stack=tests[0]["actual"].as_array().ok_or("missing actual stack")?;
                if stack.len()!=1 {return Err("adapter requires singleton actual stack".into());}
                let n = stack[0].as_i64().ok_or("adapter requires integer")?;
                let source = encode(n)?;
                let index = self.evidence.len()-1;
                self.evidence[index]["compatibility"] = json!({"contract":ADAPTER_CONTRACT,"compatible":true,"integer":n,"range":{"min":0,"max":8,"within":true}});
                Ok(Value::String(source))
            },
            ("path","check_source")=>self.semantic(false,args["source"].as_str().ok_or("missing source")?,json!({})),
            _=>Err("unknown operation".into())
        }
    }
}
impl Host for ReportHost {
    fn operations(&mut self) -> Result<ToolValue, ErrorKind> {
        Ok(ToolValue::Null)
    }
    fn invoke(&mut self, p: &str, a: &str, args: ToolValue) -> Result<ToolValue, ErrorKind> {
        let args = value_json(&args);
        let index = self.evidence.len();
        self.evidence
            .push(json!({"id":format!("attempt:{index}"),"predecessor":index.checked_sub(1).map(|i|format!("attempt:{i}")),"provider":p,"operation":a,"full_operation":if p=="adapter" { ADAPTER_CONTRACT } else if p=="forth" {"language.example.forth.evaluate_candidate"} else {"language.language.path.check_source"},"state":"attempted","arguments":args}));
        let slot = match (p, a) {
            ("forth", "evaluate_candidate") => 0,
            ("adapter", "church") => 1,
            ("path", "check_source") => 2,
            _ => 3,
        };
        let argument_bytes = serde_json::to_vec(&args).unwrap().len();
        let result = if slot == 3 || !self.config.grants[slot] {
            Err("operation not granted".into())
        } else if self.calls >= self.config.calls || self.bytes + argument_bytes > self.config.bytes
        {
            Err("shared budget exhausted before dispatch".into())
        } else {
            self.calls += 1;
            self.bytes += argument_bytes;
            let result = self.dispatch(p, a, &args);
            if let Ok(v) = &result {
                self.bytes += serde_json::to_vec(v).unwrap().len();
            }
            result
        };
        self.evidence[index]["transport_status"] = json!(if result.is_ok() {
            "completed"
        } else {
            "failed"
        });
        self.evidence[index]["semantic_classification"] = result.as_ref().ok().map(|v| {
            if p == "path" {v["value"]["outcome"].clone()}
            else if p == "forth" {json!({"envelope_status":v["status"],"tests":v["value"]["tests"].as_array().map(|ts|ts.iter().map(|t|t["outcome"].clone()).collect::<Vec<_>>())})}
            else {json!("adapter_compatible")}
        }).unwrap_or(Value::Null);
        if p == "adapter" && result.is_err() {
            self.evidence[index]["compatibility"] = json!({"contract":ADAPTER_CONTRACT,"compatible":false,"reason":result.as_ref().err(),"range":{"min":0,"max":8}});
        }
        // Retain complete evidence even if delivery exceeds the byte budget.
        self.evidence[index]["returned"] = result.as_ref().ok().cloned().unwrap_or(Value::Null);
        let result = if result.is_ok() && self.bytes > self.config.bytes {
            Err("shared return-byte budget exhausted after dispatch".into())
        } else {
            result
        };
        self.evidence[index]["state"] = json!(if result.is_ok() {
            "completed"
        } else {
            "failed"
        });
        self.evidence[index]["error"] = json!(result.as_ref().err());
        result.map(tv).map_err(ErrorKind::Runtime)
    }
}
// These host tools bind dynamic sources before invoking exactly one semantic operation.
// Session authorization sees the generated Path source before any provider is constructed.
fn execution_error(tool: impl Into<String>, message: impl Into<String>) -> ToolError {
    ToolError::Execution {
        tool: tool.into(),
        message: message.into(),
    }
}
struct BoundTools<'a> {
    host: &'a mut ReportHost,
    tools: Vec<ToolDefinition>,
}
impl ToolProvider for BoundTools<'_> {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let slot = INNER
            .iter()
            .position(|n| *n == call.name)
            .ok_or_else(|| execution_error(&call.name, "unknown bound operation"))?;
        let args = ToolValue::Object(call.arguments);
        let result = match slot {
            0 => {
                if value_json(&args)["source"] != self.host.config.candidate {
                    return Err(execution_error(call.name, "candidate binding mismatch"));
                }
                self.host
                    .invoke("forth", "evaluate_candidate", tv(json!({})))
            }
            1 => self.host.invoke("adapter", "church", args),
            _ => self.host.invoke("path", "check_source", args),
        };
        result
            .map(ToolResult::new)
            .map_err(|e| execution_error(call.name, format!("{e:?}")))
    }
}
struct Forward<'a, 'b, 'c> {
    context: &'a mut AgentWorkflowContext<'b, 'c>,
    candidate: &'static str,
    steps: &'a mut Vec<Value>,
    transferred: &'a mut Option<i64>,
}
impl Host for Forward<'_, '_, '_> {
    fn operations(&mut self) -> Result<ToolValue, ErrorKind> {
        Ok(ToolValue::Null)
    }
    fn invoke(&mut self, p: &str, a: &str, args: ToolValue) -> Result<ToolValue, ErrorKind> {
        let slot = match (p, a) {
            ("forth", "evaluate_candidate") => 0,
            ("adapter", "church") => 1,
            ("path", "check_source") => 2,
            _ => return Err(ErrorKind::Runtime("unknown operation".into())),
        };
        let args = if slot == 0 {
            tv(json!({"source":self.candidate}))
        } else {
            args
        };
        let ToolValue::Object(arguments) = args else {
            return Err(ErrorKind::Runtime("arguments must be object".into()));
        };
        let observed = if slot == 1 {
            value_json(&ToolValue::Object(arguments.clone()))["evidence"]["value"]["tests"][0]
                ["actual"][0]
                .as_i64()
        } else {
            None
        };
        let result = self
            .context
            .invoke(ToolCall {
                name: INNER[slot].into(),
                arguments,
            })
            .map(|r| r.value)
            .map_err(ErrorKind::Tool);
        if slot == 1 && result.is_ok() {
            *self.transferred = observed;
        }
        let response = result.as_ref().ok().map(value_json).unwrap_or(Value::Null);
        self.steps.push(json!({"attempt_ref":format!("attempt:{slot}"),"provider":p,"operation":a,"state":if result.is_ok() {"completed"} else {"failed"},"error":result.as_ref().err().map(|e|format!("{e:?}")),"outcome":response["value"]["outcome"],"kernel_checked":response["value"]["kernel"]["checked"]}));
        result
    }
}
struct Composition {
    candidate: &'static str,
    result: Option<Result<ToolValue, String>>,
    projection: Value,
}
impl AgentWorkflow for Composition {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            WORKFLOW,
            "Fixed read-only Forth to Church to Path composition; no arguments",
        )
    }
    fn execute(
        &mut self,
        call: ToolCall,
        context: &mut AgentWorkflowContext<'_, '_>,
    ) -> Result<ToolResult, ToolError> {
        if !call.arguments.is_empty() {
            return Err(execution_error(call.name, "workflow takes no arguments"));
        }
        let mut steps = Vec::new();
        let mut transferred = None;
        let result = Interpreter::new()
            .eval(
                SCRIPT,
                &mut Forward {
                    context,
                    candidate: self.candidate,
                    steps: &mut steps,
                    transferred: &mut transferred,
                },
            )
            .map_err(|e| e.to_string());
        self.result = Some(result.clone());
        let final_value = result.as_ref().ok().map(value_json).unwrap_or(Value::Null);
        let success = final_value["status"] == "read_only"
            && final_value["value"]["outcome"] == "Checked"
            && final_value["value"]["kernel"]["checked"] == true;
        self.projection = json!({"requested_check":CHECK,"adapter_contract":ADAPTER_CONTRACT,"transferred_integer":transferred,"steps":steps,"outcome":final_value["value"]["outcome"],"kernel_checked":final_value["value"]["kernel"]["checked"],"success":success,"error":result.as_ref().err(),"limitations":["Finite concrete Forth observation only", "Host adapter is not a proved compiler", "Attempt references are local to this report", "Missing, denied, or unknown outcomes are not success"]});
        Ok(ToolResult::new(tv(self.projection.clone())))
    }
}
struct Deterministic {
    decisions: usize,
    message_content_utf8_bytes: Vec<usize>,
}
impl DecisionModel for Deterministic {
    fn decide(&mut self, messages: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.message_content_utf8_bytes
            .push(messages.iter().map(|m| m.content.len()).sum());
        self.decisions += 1;
        match self.decisions {
            1 => Ok(json!({"type":"tool","name":WORKFLOW,"arguments":{}}).to_string()),
            2 => Ok(json!({"type":"complete","content":"Composition attempted; inspect retained evidence for semantic success."}).to_string()),
            _ => Err("deterministic protocol exhausted".into()),
        }
    }
}
fn agent_execute(h: &mut ReportHost) -> (Result<ToolValue, String>, Value) {
    let candidate = h.config.candidate;
    let grants = h.config.grants;
    let tools = INNER
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let (arg, kind) = if i == 1 {
                ("evidence", ToolInputKind::Object)
            } else {
                ("source", ToolInputKind::String)
            };
            ToolDefinition::new(
                *name,
                "Host source-binding wrapper; exactly one operation, no nested dispatch",
            )
            .input(ToolInputDefinition::required(arg, kind, "Bound input"))
        })
        .collect();
    let mut session = AgentSession::builder()
        .provider(BoundTools { host: h, tools })
        .build()
        .unwrap();
    let mut workflow = Composition {
        candidate,
        result: None,
        projection: Value::Null,
    };
    let mut model = Deterministic {
        decisions: 0,
        message_content_utf8_bytes: vec![],
    };
    let mut authorized_calls = Vec::new();
    let run = run_session_with_workflows(
        &mut model,
        &mut session,
        "Run the fixed CE4 composition without edits",
        AgentLimits {
            max_decisions: 2,
            max_tool_calls: 4,
            max_message_bytes: 1_000_000,
            max_conversation_bytes: 4_000_000,
            ..AgentLimits::default()
        },
        |d: &ToolDefinition, c: &ToolCall| {
            let allowed = d.effect == ToolEffect::Read
                && (c.name == WORKFLOW
                    || INNER
                        .iter()
                        .position(|n| *n == c.name)
                        .is_some_and(|i| grants[i]));
            authorized_calls.push(json!({"name":c.name,"arguments":value_json(&ToolValue::Object(c.arguments.clone())),"allowed":allowed}));
            allowed
        },
        &mut [&mut workflow],
    );
    let metrics = json!({"exercised":true,"decisions":model.decisions,"decision_message_content_utf8_bytes":model.message_content_utf8_bytes,"cumulative_message_content_utf8_bytes":model.message_content_utf8_bytes.iter().sum::<usize>(),"workflow_projection":workflow.projection,"authorized_calls":authorized_calls,"tokens":null,"run":run.as_ref().ok().map(|r|json!({"stop":format!("{:?}",r.stop),"tool_calls":r.tool_calls})),"error":run.as_ref().err().map(ToString::to_string)});
    drop(session);
    for denied in authorized_calls.iter().filter(|c| c["allowed"] == false) {
        let Some(slot) = INNER.iter().position(|n| denied["name"] == *n) else {
            continue;
        };
        let (provider, operation, full_operation) = match slot {
            0 => (
                "forth",
                "evaluate_candidate",
                "language.example.forth.evaluate_candidate",
            ),
            1 => ("adapter", "church", ADAPTER_CONTRACT),
            _ => (
                "path",
                "check_source",
                "language.language.path.check_source",
            ),
        };
        let index = h.evidence.len();
        h.evidence.push(json!({"id":format!("attempt:{index}"),"predecessor":index.checked_sub(1).map(|i|format!("attempt:{i}")),"provider":provider,"operation":operation,"full_operation":full_operation,"session_operation":denied["name"],"arguments":denied["arguments"],"state":"failed","transport_status":"failed","dispatch_status":"not_dispatched","failure_phase":"session_authorization","error":"AgentSession authorization denied inner call","returned":null}));
    }
    // Completion text is not evidence of success; use only the delivered consumer result.
    let result = run.map_err(|e| e.to_string()).and_then(|_| {
        workflow
            .result
            .unwrap_or_else(|| Err("workflow was not executed".into()))
    });
    (result, metrics)
}
fn run(config: Config) -> Value {
    run_arm(config, false)
}
fn run_direct(config: Config) -> Value {
    run_arm(config, true)
}
fn run_arm(config: Config, direct: bool) -> Value {
    run_mode(config, direct, false)
}
fn run_agent(config: Config) -> Value {
    run_mode(config, false, true)
}
fn run_mode(config: Config, direct: bool, agent: bool) -> Value {
    let mut h = ReportHost {
        config,
        evidence: vec![],
        snapshots: vec![],
        calls: 0,
        bytes: 0,
    };
    let start = std::time::Instant::now();
    let mut agent_metrics = Value::Null;
    let result = if agent {
        let (result, metrics) = agent_execute(&mut h);
        agent_metrics = metrics;
        result
    } else if direct {
        (|| {
            let produced = h.invoke("forth", "evaluate_candidate", tv(json!({})))?;
            let adapted = h.invoke(
                "adapter",
                "church",
                tv(json!({"evidence":value_json(&produced)})),
            )?;
            h.invoke(
                "path",
                "check_source",
                tv(json!({"source":value_json(&adapted)})),
            )
        })()
        .map_err(|e: ErrorKind| format!("{e:?}"))
    } else {
        Interpreter::new()
            .eval(SCRIPT, &mut h)
            .map_err(|e| e.to_string())
    };
    let elapsed_us = start.elapsed().as_micros();
    let success = result.as_ref().ok().map(value_json).is_some_and(|v| {
        v["status"] == "read_only"
            && v["value"]["kernel"]["checked"] == true
            && v["value"]["outcome"] == "Checked"
    });
    for (p, a) in [
        ("forth", "evaluate_candidate"),
        ("adapter", "church"),
        ("path", "check_source"),
    ] {
        if !h.evidence.iter().any(|e| e["provider"] == p) {
            h.evidence.push(
                json!({"id":format!("step:{}",h.evidence.len()),"predecessor":h.evidence.last().map(|e|e["id"].clone()),"provider":p,"operation":a,"full_operation":if p=="adapter" {ADAPTER_CONTRACT} else if p=="forth" {"language.example.forth.evaluate_candidate"} else {"language.language.path.check_source"},"state":"skipped","transport_status":"not_attempted","reason":"prior step failed"}),
            );
        }
    }
    let semantic_ops = h
        .evidence
        .iter()
        .filter(|e| e.get("semantic_arguments").is_some())
        .count();
    let checks = h
        .evidence
        .iter()
        .filter(|e| e["provider"] == "path" && e["transport_status"] == "completed")
        .count();
    json!({"success":success,"workflow_status":if success {"succeeded"} else {"not_succeeded"},"arm":if agent {"agent_workflow"} else if direct {"direct_sequential"} else {"script_composed"},"agent":agent_metrics,"adapter_contract":ADAPTER_CONTRACT,"requested_check":CHECK,"script":if direct {None} else {Some(SCRIPT)},"script_error":result.err(),"steps":h.evidence,"snapshots":h.snapshots,"grants":h.config.grants,"limits":{"calls":h.config.calls,"bytes":h.config.bytes},"metrics":{"elapsed_us":elapsed_us,"dispatched_calls":h.calls,"semantic_operations":semantic_ops,"checks":checks,"argument_and_return_json_bytes":h.bytes,"decisions":agent_metrics["decisions"],"tokens":null},"simulated_faults":{"incomplete":h.config.simulated_incomplete,"authorization_denial":h.config.simulated_authorization_denial}})
}
fn compact(r: &Value) -> Value {
    json!({"success":r["success"],"workflow_status":r["workflow_status"],"requested_check":r["requested_check"],"adapter_contract":ADAPTER_CONTRACT,"transferred_integer":r["steps"][1]["compatibility"]["integer"],"compatibility":r["steps"][1]["compatibility"],"limitations":["Finite concrete Forth observation only", "Host adapter contract is not a provider version or a proved compiler", "Missing or unknown provider outcomes are not success", "References are local to this arm's report", "Agent metrics, when exercised, are in the full report; token usage unavailable"],"metrics":r["metrics"],"steps":r["steps"].as_array().unwrap().iter().enumerate().map(|(i,e)|json!({"evidence_index":i,"attempt_ref":e["id"],"predecessor":e["predecessor"],"source_ref":e["source_ref"],"operation":e["full_operation"],"provider":e["provider"],"state":e["state"],"transport_status":e["transport_status"],"semantic_classification":e["semantic_classification"],"error":e["error"],"outcome":e["returned"]["value"]["outcome"],"kernel":e["returned"]["value"]["kernel"]})).collect::<Vec<_>>()})
}
fn measured(r: Value) -> Value {
    let projection = compact(&r);
    // Measure these payloads before adding the external size wrapper (no self-reference).
    json!({"report_bytes":serde_json::to_vec(&r).unwrap().len(),"projection_bytes":serde_json::to_vec(&projection).unwrap().len(),"report":r,"projection":projection})
}
fn main() {
    let composed = run(Config::default());
    let direct = run_direct(Config::default());
    let agent = run_agent(Config::default());
    let same_evidence =
        composed["steps"] == direct["steps"] && composed["snapshots"] == direct["snapshots"];
    let success = composed["success"] == true
        && direct["success"] == true
        && same_evidence
        && agent["success"] == true;
    println!("{}",serde_json::to_string_pretty(&json!({"composed":measured(composed),"independent_direct_baseline":measured(direct),"comparison":{"same_evidence_and_actual_outputs":same_evidence,"elapsed_note":"Observed wall time, nondeterministic; not a model efficiency claim"},"agent_workflow":measured(agent)})).unwrap());
    if !success {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agent_workflow_authorizes_every_bound_operation() {
        let r = run_agent(Config::default());
        assert_eq!(r["success"], true, "{r:#}");
        assert_eq!(r["agent"]["decisions"], 2);
        let projection = &r["agent"]["workflow_projection"];
        assert_eq!(projection["transferred_integer"], 4);
        assert_eq!(projection["requested_check"], CHECK);
        assert_eq!(projection["success"], true);
        assert_eq!(projection["kernel_checked"], true);
        assert!(projection.get("value").is_none());
        assert!(
            serde_json::to_vec(projection).unwrap().len()
                < serde_json::to_vec(&r["steps"][2]["returned"])
                    .unwrap()
                    .len()
        );
        let sizes = r["agent"]["decision_message_content_utf8_bytes"]
            .as_array()
            .unwrap();
        assert_eq!(sizes.len(), 2);
        assert_eq!(
            r["agent"]["cumulative_message_content_utf8_bytes"]
                .as_u64()
                .unwrap(),
            sizes.iter().map(|v| v.as_u64().unwrap()).sum::<u64>()
        );
        assert!(sizes[1].as_u64().unwrap() > sizes[0].as_u64().unwrap());
        let calls = r["agent"]["authorized_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 4);
        for i in 0..3 {
            assert_eq!(calls[i + 1]["name"], INNER[i]);
        }
        assert_eq!(calls[1]["arguments"]["source"], Config::default().candidate);
        assert_eq!(calls[3]["arguments"]["source"], r["snapshots"][1]["text"]);
        let direct = run_direct(Config::default());
        assert_eq!(r["steps"], direct["steps"]);
        assert_eq!(r["snapshots"], direct["snapshots"]);
    }
    #[test]
    fn agent_denial_prevents_actual_provider_dispatch() {
        for slot in 0..3 {
            let mut config = Config::default();
            config.grants[slot] = false;
            let r = run_agent(config);
            assert_eq!(r["success"], false, "{r:#}");
            assert_eq!(r["metrics"]["dispatched_calls"], slot);
            let calls = r["agent"]["authorized_calls"].as_array().unwrap();
            assert_eq!(calls.last().unwrap()["name"], INNER[slot]);
            assert_eq!(calls.last().unwrap()["allowed"], false);
            let denied = &r["steps"][slot];
            assert_eq!(denied["id"], format!("attempt:{slot}"));
            assert_eq!(denied["state"], "failed");
            assert_eq!(denied["transport_status"], "failed");
            assert_eq!(denied["dispatch_status"], "not_dispatched");
            assert_eq!(denied["failure_phase"], "session_authorization");
            assert!(denied["error"].as_str().unwrap().contains("denied"));
            assert!(denied.get("source_ref").is_none());
            for later in slot + 1..3 {
                assert_eq!(r["steps"][later]["state"], "skipped");
            }
            assert_eq!(
                r["agent"]["workflow_projection"]["steps"][slot]["state"],
                "failed"
            );
            assert_eq!(
                r["snapshots"].as_array().unwrap().len(),
                if slot == 0 { 0 } else { 1 }
            );
        }
    }
    #[test]
    fn agent_failures_are_not_model_completion_success() {
        for config in [
            Config {
                candidate: ": result 5 ;",
                expected: vec![5],
                ..Config::default()
            },
            Config {
                bytes: 1,
                ..Config::default()
            },
            Config {
                calls: 1,
                ..Config::default()
            },
        ] {
            assert_eq!(run_agent(config)["success"], false);
        }
    }
    #[test]
    fn forged_adapter_evidence_is_rejected() {
        let mut h = ReportHost {
            config: Config::default(),
            evidence: vec![],
            snapshots: vec![],
            calls: 0,
            bytes: 0,
        };
        assert!(h.invoke("adapter", "church", tv(json!({"evidence":{"status":"read_only","value":{"tests":[{"outcome":"passed","actual":[4]}]}}}))).is_err());
    }
    #[test]
    fn direct_baseline_and_provenance() {
        let composed = run(Config::default());
        let direct = run_direct(Config::default());
        assert_eq!(composed["steps"], direct["steps"]);
        assert_eq!(composed["snapshots"], direct["snapshots"]);
        assert_ne!(
            composed["snapshots"][0]["qualified_identity"],
            composed["snapshots"][1]["qualified_identity"]
        );
        assert_eq!(
            composed["steps"][0]["semantic_arguments"]["source"],
            ": result 2 2 + ;"
        );
        assert_eq!(composed["steps"][2]["source_ref"], "source:1");
        assert_eq!(composed["steps"][2]["predecessor"], "attempt:1");
        assert_eq!(direct["metrics"]["semantic_operations"], 2);
        assert_eq!(direct["metrics"]["checks"], 1);
        let measured = measured(direct);
        assert_eq!(
            measured["report_bytes"],
            serde_json::to_vec(&measured["report"]).unwrap().len()
        );
        assert_eq!(
            measured["projection_bytes"],
            serde_json::to_vec(&measured["projection"]).unwrap().len()
        );
    }
    #[test]
    fn adapter_runtime_type_and_range_contract() {
        for actual in [
            json!(["4"]),
            json!([true]),
            json!([-1]),
            json!([9]),
            json!([]),
            json!([4, 4]),
        ] {
            let mut h = ReportHost {
                config: Config::default(),
                evidence: vec![],
                snapshots: vec![],
                calls: 0,
                bytes: 0,
            };
            let evidence = json!({"status":"read_only","value":{"tests":[{"outcome":"passed","actual":actual}]}});
            h.evidence
                .push(json!({"provider":"forth","state":"completed","returned":evidence}));
            let result = h.invoke("adapter", "church", tv(json!({"evidence":evidence})));
            assert!(result.is_err(), "{actual}");
            assert_eq!(h.evidence[1]["compatibility"]["compatible"], false);
            assert_eq!(h.evidence[1]["compatibility"]["contract"], ADAPTER_CONTRACT);
        }
    }
    #[test]
    fn adapter_rejects_fractional_json_at_dispatch_boundary() {
        // ToolValue has no floating-point variant; exercise the JSON validator directly.
        let mut h = ReportHost {
            config: Config::default(),
            evidence: vec![
                json!({"provider":"forth","state":"completed","returned":{"status":"read_only","value":{"tests":[{"outcome":"passed","actual":[4.5]}]}}}),
                json!({}),
            ],
            snapshots: vec![],
            calls: 0,
            bytes: 0,
        };
        assert!(h.dispatch("adapter","church", &json!({"evidence":{"status":"read_only","value":{"tests":[{"outcome":"passed","actual":[4.5]}]}}})).is_err());
    }
    #[test]
    fn rejection_transport_is_not_semantic_success() {
        let r = run_direct(Config {
            candidate: ": result 5 ;",
            expected: vec![5],
            ..Config::default()
        });
        assert_eq!(r["steps"][2]["transport_status"], "completed");
        assert_eq!(
            r["steps"][2]["semantic_classification"],
            "MathematicalRejection"
        );
        assert_eq!(r["workflow_status"], "not_succeeded");
        let c = compact(&r);
        assert_eq!(c["transferred_integer"], 5);
        assert_eq!(c["compatibility"]["range"]["within"], true);
        assert_eq!(c["requested_check"], CHECK);
    }
    #[test]
    fn success() {
        let r = run(Config::default());
        assert_eq!(r["success"], true, "{r:#}");
        assert_eq!(
            r["steps"][0]["returned"]["value"]["tests"][0]["actual"],
            json!([4])
        );
    }
    #[test]
    fn incompatible() {
        let r = run(Config {
            candidate: ": result 2 2 ;",
            expected: vec![2, 2],
            ..Config::default()
        });
        assert_eq!(r["steps"][1]["state"], "failed");
        assert_eq!(r["steps"][2]["state"], "skipped");
    }
    #[test]
    fn deny_consumer() {
        let r = run(Config {
            grants: [true, true, false],
            ..Config::default()
        });
        assert_eq!(r["steps"][2]["error"], "operation not granted");
        assert_eq!(r["success"], false);
    }
    #[test]
    fn incomplete_simulated() {
        let r = run(Config {
            simulated_incomplete: true,
            ..Config::default()
        });
        assert_eq!(r["steps"][2]["state"], "skipped");
    }
    #[test]
    fn real_producer_failure() {
        let r = run(Config {
            candidate: ": result 1 0 / ;",
            ..Config::default()
        });
        assert_eq!(r["success"], false);
        assert_eq!(r["steps"][2]["state"], "skipped");
        assert!(!r["steps"][0]["returned"].is_null());
    }
    #[test]
    fn real_consumer_rejection() {
        let r = run(Config {
            candidate: ": result 5 ;",
            expected: vec![5],
            ..Config::default()
        });
        assert_eq!(r["steps"][2]["state"], "completed");
        assert_eq!(
            r["steps"][2]["returned"]["value"]["outcome"],
            "MathematicalRejection"
        );
        assert_eq!(r["success"], false);
        assert_eq!(
            r["steps"][2]["returned"]["value"]["kernel"]["checked"],
            false
        );
    }
    #[test]
    fn budgets() {
        for c in [
            Config {
                calls: 1,
                ..Config::default()
            },
            Config {
                bytes: 1,
                ..Config::default()
            },
        ] {
            let r = run(c);
            assert_eq!(r["success"], false);
            assert!(r["steps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["state"] == "failed"));
        }
    }
    #[test]
    fn stale_revision_prevents_semantic_provider_evaluation() {
        let mut buffers = ScratchBuffers::default();
        let id = buffers.create("example.forth", "");
        let target = buffers.get(id).unwrap();
        let snapshot = target.snapshot();
        let wrong_version = snapshot.version.checked_add(1).unwrap();
        let mut controller = SourceEditController::default();
        let mut provider = SemanticToolProvider::new(
            &scope_forth::LANGUAGE,
            target,
            &mut controller,
            PermittedEditScope::Document(snapshot.id),
            None,
            |_: SemanticAuthorization<'_>| {
                panic!("stale revision must be rejected before authorization and evaluation")
            },
        )
        .unwrap();
        let result = provider
            .invoke(
                ToolCall::new("language.example.forth.evaluate_candidate")
                    .with_argument("expected_source", snapshot.text.into())
                    .with_argument("expected_version", wrong_version.to_string().into())
                    .with_argument(
                        "arguments",
                        tv(json!({
                            "source": Config::default().candidate,
                            "tests": [{"input": [], "expected": [4]}]
                        })),
                    ),
            )
            .unwrap();
        let envelope = value_json(&result.value);
        assert_eq!(envelope["status"], "revision_conflict", "{envelope:#}");
        assert_eq!(envelope["message"], "target revision or source changed");
        assert_eq!(envelope["operation_id"], "evaluate_candidate");
        assert!(provider.native_result().is_none());
        let unchanged = target.snapshot();
        assert_eq!(unchanged.id, snapshot.id);
        assert_eq!(unchanged.version, snapshot.version);
        assert_eq!(unchanged.text, snapshot.text);
    }
    #[test]
    fn simulated_authorization_denial() {
        let r = run(Config {
            simulated_authorization_denial: true,
            ..Config::default()
        });
        assert_eq!(r["success"], false);
        assert_eq!(r["steps"][2]["state"], "skipped");
    }
    #[test]
    fn compact_projection() {
        let r = run(Config::default());
        let c = compact(&r);
        assert_eq!(c["success"], r["success"]);
        for i in 0..3 {
            assert_eq!(c["steps"][i]["state"], r["steps"][i]["state"]);
            assert_eq!(c["steps"][i]["evidence_index"], i);
        }
    }
}
