//! Sequential, read-only Q3 fixture. Live requests require explicit opt-in.
//! cargo test -p scope-ide --features agent --example q3_live
//! cargo run -p scope-ide --features agent --example q3_live -- --run-live OUTPUT MODEL ENDPOINT
#[cfg(feature = "agent")]
mod experiment {
    use scope_ide::agent::{
        AgentConfig, AgentLimits, AgentStop, CellAgent, DecisionMessage, DecisionModel,
        ProtocolTrace, read_only_policy, run_session_traced,
    };
    use scope_ide::tool::{
        AgentSession, ToolCall, ToolDefinition, ToolEffect, ToolError, ToolInputDefinition,
        ToolInputKind, ToolProvider, ToolResult, ToolValue,
    };
    use serde_json::{Value, json};
    use std::{
        fs::{File, OpenOptions},
        io::Write,
        path::Path,
        sync::{Arc, Mutex},
        time::Instant,
    };

    const FIRST: &str = "sealed-index";
    const SECOND: &str = "hidden-card-482";
    const ANSWER: &str = "amber-heron-916";
    const TASK: &str = "Inspect resource sealed-index with fixture.inspect. Its feedback identifies the next resource. Inspect that resource next, then complete with exactly its answer field. Perform exactly these two sequential read-only inspections, one decision per response. Do not guess, batch actions, or perform other operations.";

    fn limits() -> AgentLimits {
        AgentLimits {
            max_decisions: 3,
            max_tool_calls: 2,
            max_malformed: 0,
            max_tokens_per_decision: 1500,
            ..AgentLimits::default()
        }
    }
    fn value_json(value: &ToolValue) -> Value {
        match value {
            ToolValue::Null => Value::Null,
            ToolValue::Boolean(v) => json!(v),
            ToolValue::Integer(v) => json!(v),
            ToolValue::String(v) => json!(v),
            ToolValue::Array(v) => Value::Array(v.iter().map(value_json).collect()),
            ToolValue::Object(v) => {
                Value::Object(v.iter().map(|(k, v)| (k.clone(), value_json(v))).collect())
            }
        }
    }
    fn call_record(call: &ToolCall) -> Value {
        // Keep every argument, not merely the expected resource field.
        json!({"name":call.name,"arguments":value_json(&ToolValue::Object(call.arguments.clone()))})
    }
    fn exact(call: &ToolCall, resource: &str) -> bool {
        call.name == "fixture.inspect"
            && call.arguments.len() == 1
            && call.arguments.get("resource").and_then(ToolValue::as_str) == Some(resource)
    }
    #[derive(Default)]
    struct State {
        calls: Vec<Value>,
        accepted: usize,
    }
    struct Fixture {
        tools: Vec<ToolDefinition>,
        state: Arc<Mutex<State>>,
    }
    impl Fixture {
        fn new(state: Arc<Mutex<State>>) -> Self {
            Self { state, tools: vec![ToolDefinition::new("fixture.inspect", "Read a resource. Follow the next_resource returned by the first inspection.")
                .effect(ToolEffect::Read)
                .input(ToolInputDefinition::required("resource", ToolInputKind::String, "Resource identifier."))] }
        }
    }
    impl ToolProvider for Fixture {
        fn tools(&self) -> &[ToolDefinition] {
            &self.tools
        }
        fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(call_record(&call));
            let expected = match state.accepted {
                0 => FIRST,
                1 => SECOND,
                _ => "",
            };
            if expected.is_empty() || !exact(&call, expected) {
                return Err(ToolError::Execution {
                    tool: call.name,
                    message: "Unexpected inspection or sequence".into(),
                });
            }
            state.accepted += 1;
            let value = if state.accepted == 1 {
                ToolValue::object([("resource", FIRST.into()), ("next_resource", SECOND.into())])
            } else {
                ToolValue::object([("resource", SECOND.into()), ("answer", ANSWER.into())])
            };
            Ok(ToolResult::new(value))
        }
    }
    // This wrapper records precisely the messages delivered to the adapter, without rewriting them.
    struct Measured<M, W> {
        inner: M,
        evidence: W,
        requests: usize,
    }
    impl<M: DecisionModel, W: Write> DecisionModel for Measured<M, W> {
        fn trace_settings(&self) -> Option<Value> {
            self.inner.trace_settings()
        }
        fn decide(
            &mut self,
            messages: &[DecisionMessage],
            max_tokens: u32,
        ) -> Result<String, String> {
            self.requests += 1;
            let request = json!({"event":"adapter_request","ordinal":self.requests,"messages":messages.iter().map(|m| json!({"role":m.role,"content":m.content})).collect::<Vec<_>>(),"max_tokens":max_tokens,"settings":self.trace_settings()});
            writeln!(self.evidence, "{request}")
                .and_then(|_| self.evidence.flush())
                .map_err(|e| e.to_string())?;
            let start = Instant::now();
            let result = self.inner.decide(messages, max_tokens);
            let record = json!({"event":"adapter_response","ordinal":self.requests,"adapter_latency_ms":start.elapsed().as_secs_f64()*1000.0,"latency_scope":"wall-clock DecisionModel adapter call; not server latency","response":result.as_ref().ok(),"error":result.as_ref().err()});
            writeln!(self.evidence, "{record}")
                .and_then(|_| self.evidence.flush())
                .map_err(|e| e.to_string())?;
            result
        }
    }
    fn execute(
        model: &mut impl DecisionModel,
        trace: &mut ProtocolTrace<impl Write>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let state = Arc::new(Mutex::new(State::default()));
        let mut session = AgentSession::builder()
            .provider(Fixture::new(state.clone()))
            .build()?;
        let mut authorizations = Vec::new();
        let result = run_session_traced(
            model,
            &mut session,
            TASK,
            limits(),
            |definition, call| {
                let position = authorizations.len();
                let allowed = read_only_policy(definition, call)
                    && match position {
                        0 => exact(call, FIRST),
                        1 => exact(call, SECOND),
                        _ => false,
                    };
                authorizations.push(json!({"call":call_record(call),"allowed":allowed}));
                allowed
            },
            trace,
        );
        let state = state.lock().unwrap();
        let outcome = match result {
            Ok(run) => {
                json!({"stop":format!("{:?}",run.stop),"content":run.content,"decisions":run.decisions,"tool_calls":run.tool_calls,"malformed":run.malformed,
                "task_success":run.stop == AgentStop::Completed && run.content == ANSWER && run.decisions == 3 && run.tool_calls == 2 && run.malformed == 0 && state.accepted == 2 && state.calls.len() == 2 && authorizations.len() == 2 && authorizations.iter().all(|e| e["allowed"] == true) && !trace.failed()})
            }
            Err(error) => json!({"task_success":false,"backend_or_runner_error":error.to_string()}),
        };
        Ok(
            json!({"outcome":outcome,"provider_calls":state.calls,"authorizations":authorizations,"trace_failed":trace.failed(),"source_mutation":"No source or write capability installed"}),
        )
    }
    fn private_file(path: &Path) -> std::io::Result<File> {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }
    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<String> = std::env::args().collect();
        if args.len() != 5 || args[1] != "--run-live" {
            return Err("Opt in with: --run-live EXISTING_CONTROLLED_OUTPUT_DIRECTORY MODEL ENDPOINT; offline validation: cargo test -p scope-ide --features agent --example q3_live".into());
        }
        let dir = Path::new(&args[2]);
        if !dir.is_dir() {
            return Err("Output directory must already exist".into());
        }
        let manifest = json!({"experiment":"Q3 sequential read-only","model":args[3],"endpoint":args[4],"adapter":"CellAgent/OpenAiChat","temperature":0.2,"order":[1,2,3],"max_decisions":3,"max_tool_calls":2,"max_malformed":0,"max_tokens":1500,"max_message_bytes":limits().max_message_bytes,"max_conversation_bytes":limits().max_conversation_bytes,"task":TASK,"parser":"existing strict parser; no repairs or retries","latency":"measured adapter wall-clock latency, not server latency"});
        let mut manifest_file = private_file(&dir.join("manifest.json"))?;
        writeln!(manifest_file, "{manifest}")?;
        manifest_file.sync_all()?;
        for run in 1..=3 {
            let mut model = Measured {
                inner: CellAgent::new(AgentConfig::local(&args[3], &args[4]))?,
                evidence: private_file(&dir.join(format!("{run}.messages.jsonl")))?,
                requests: 0,
            };
            let mut trace =
                ProtocolTrace::new(private_file(&dir.join(format!("{run}.trace.jsonl")))?);
            let mut record = execute(&mut model, &mut trace)?;
            record["run"] = json!(run);
            record["adapter_requests"] = json!(model.requests);
            writeln!(
                private_file(&dir.join(format!("{run}.result.json")))?,
                "{record}"
            )?;
            println!("{record}");
            if trace.failed() {
                return Err("Trace capture failed; stopping to preserve evidence".into());
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        struct Script {
            responses: std::collections::VecDeque<String>,
            seen: Vec<Vec<String>>,
        }
        impl DecisionModel for Script {
            fn decide(
                &mut self,
                messages: &[DecisionMessage],
                tokens: u32,
            ) -> Result<String, String> {
                assert_eq!(tokens, 1500);
                self.seen
                    .push(messages.iter().map(|m| m.content.clone()).collect());
                Ok(self
                    .responses
                    .pop_front()
                    .expect("unexpected retry/request"))
            }
        }
        fn inspect(resource: &str) -> String {
            json!({"type":"tool","name":"fixture.inspect","arguments":{"resource":resource}})
                .to_string()
        }
        fn complete() -> String {
            json!({"type":"complete","content":ANSWER}).to_string()
        }
        fn check(responses: Vec<String>) -> (Value, Script, String) {
            let mut model = Script {
                responses: responses.into(),
                seen: Vec::new(),
            };
            let mut bytes = Vec::new();
            let record = execute(&mut model, &mut ProtocolTrace::new(&mut bytes)).unwrap();
            (record, model, String::from_utf8(bytes).unwrap())
        }
        #[test]
        fn sequential_success_and_progressive_disclosure() {
            let (record, model, trace) = check(vec![inspect(FIRST), inspect(SECOND), complete()]);
            assert_eq!(record["outcome"]["task_success"], true);
            assert_eq!(record["provider_calls"].as_array().unwrap().len(), 2);
            let initial = model.seen[0].join("\n");
            assert!(initial.contains(FIRST));
            assert!(!initial.contains(SECOND));
            assert!(!initial.contains(ANSWER));
            let first_feedback = model.seen[1].last().unwrap();
            assert!(first_feedback.contains(SECOND));
            assert!(!model.seen[1].join("\n").contains(ANSWER));
            assert!(model.seen[2].last().unwrap().contains(ANSWER));
            assert!(trace.contains("\"event\":\"feedback\""));
        }
        #[test]
        fn skip_second_and_early_completion_fail_even_with_correct_answer() {
            for script in [vec![complete()], vec![inspect(FIRST), complete()]] {
                let (record, _, _) = check(script);
                assert_eq!(record["outcome"]["task_success"], false);
            }
        }
        #[test]
        fn reverse_wrong_arguments_and_denied_operation_never_reach_provider() {
            let wrong = json!({"type":"tool","name":"fixture.inspect","arguments":{"resource":FIRST,"extra":"no"}}).to_string();
            let denied =
                json!({"type":"tool","name":"fixture.delete","arguments":{"resource":FIRST}})
                    .to_string();
            for decision in [inspect(SECOND), inspect("wrong"), wrong, denied] {
                let (record, _, _) = check(vec![decision, complete()]);
                assert_eq!(record["outcome"]["task_success"], false);
                assert_eq!(record["provider_calls"], json!([]));
            }
        }
        #[test]
        fn malformed_batches_are_not_repaired_or_retried() {
            for batch in [
                format!("[{},{}]", inspect(FIRST), inspect(SECOND)),
                format!("{}\n{}", inspect(FIRST), inspect(SECOND)),
                "not json".into(),
            ] {
                let (record, model, _) = check(vec![batch]);
                assert_eq!(model.seen.len(), 1);
                assert_eq!(record["outcome"]["stop"], "MalformedBudget");
                assert_eq!(record["outcome"]["task_success"], false);
                assert_eq!(record["provider_calls"], json!([]));
            }
        }
        #[test]
        fn repeated_first_resource_is_denied_and_cannot_succeed() {
            let (record, _, _) = check(vec![inspect(FIRST), inspect(FIRST), complete()]);
            assert_eq!(record["outcome"]["task_success"], false);
            assert_eq!(record["provider_calls"].as_array().unwrap().len(), 1);
            assert_eq!(record["authorizations"][1]["allowed"], false);
        }
        #[test]
        fn provider_itself_enforces_sequence() {
            let state = Arc::new(Mutex::new(State::default()));
            let mut fixture = Fixture::new(state.clone());
            let call = |resource: &str| ToolCall {
                name: "fixture.inspect".into(),
                arguments: [("resource".into(), resource.into())].into_iter().collect(),
            };
            assert!(fixture.invoke(call(SECOND)).is_err());
            assert_eq!(state.lock().unwrap().accepted, 0);
            assert!(fixture.invoke(call(FIRST)).is_ok());
            assert!(fixture.invoke(call(FIRST)).is_err());
            assert_eq!(state.lock().unwrap().accepted, 1);
            assert!(fixture.invoke(call(SECOND)).is_ok());
            assert!(fixture.invoke(call(SECOND)).is_err());
        }
        #[test]
        fn wrong_final_answer_fails() {
            let (record, _, _) = check(vec![
                inspect(FIRST),
                inspect(SECOND),
                json!({"type":"complete","content":"wrong"}).to_string(),
            ]);
            assert_eq!(record["outcome"]["task_success"], false);
        }
    }
}
#[cfg(feature = "agent")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    experiment::run()
}
#[cfg(not(feature = "agent"))]
fn main() {
    eprintln!("Requires --features agent; live requests require --run-live OUTPUT MODEL ENDPOINT");
}
