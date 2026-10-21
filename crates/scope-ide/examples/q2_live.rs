//! Explicitly opt-in Q2 fixture. No model calls unless --run-live is supplied.
#[cfg(feature = "agent")]
mod experiment {
    use scope_ide::agent::{
        AgentConfig, AgentLimits, CellAgent, DecisionMessage, DecisionModel, ProtocolTrace,
        read_only_policy, run_session_traced,
    };
    use scope_ide::tool::{
        AgentSession, ToolCall, ToolDefinition, ToolEffect, ToolError, ToolInputDefinition,
        ToolInputKind, ToolProvider, ToolResult, ToolValue,
    };
    use serde_json::json;
    use std::{
        fs::{File, OpenOptions},
        io::Write,
        sync::{Arc, Mutex},
    };

    const CLARIFICATION: &str = "Return exactly one JSON decision per response. Do not return multiple JSON objects, arrays of decisions, or a sequence of actions. If you need to inspect a resource before another action, return only the tool decision for that inspection now. You will receive its tool_result before deciding what to do next. Use the existing tool or complete schema.";
    const TASK: &str = "Inspect resource sealed-card using fixture.inspect exactly once. Read its returned feedback, then complete with the exact value of its answer field. The answer is not supplied here; you must inspect before completing. Do not guess or perform other operations.";
    struct Fixture {
        tools: Vec<ToolDefinition>,
        calls: Arc<Mutex<Vec<serde_json::Value>>>,
    }
    impl ToolProvider for Fixture {
        fn tools(&self) -> &[ToolDefinition] {
            &self.tools
        }
        fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
            self.calls.lock().unwrap().push(json!({"name":call.name,"resource":call.arguments.get("resource").and_then(ToolValue::as_str)}));
            if call.name != "fixture.inspect"
                || call.arguments.get("resource").and_then(ToolValue::as_str) != Some("sealed-card")
            {
                return Err(ToolError::Execution {
                    tool: call.name,
                    message: "Expected resource sealed-card".into(),
                });
            }
            Ok(ToolResult::new(ToolValue::object([
                ("resource", "sealed-card".into()),
                ("answer", "cobalt-orchid-731".into()),
            ])))
        }
    }
    struct Model {
        inner: CellAgent,
        variant: bool,
        delivered: File,
    }
    impl DecisionModel for Model {
        fn trace_settings(&self) -> Option<serde_json::Value> {
            self.inner.trace_settings()
        }
        fn decide(
            &mut self,
            messages: &[DecisionMessage],
            max_tokens: u32,
        ) -> Result<String, String> {
            let mut messages = messages.to_vec();
            if self.variant {
                messages[0].content.push('\n');
                messages[0].content.push_str(CLARIFICATION);
            }
            let visible = json!({"messages":messages.iter().map(|m| json!({"role":m.role,"content":m.content})).collect::<Vec<_>>(),"max_tokens":max_tokens,"settings":self.trace_settings()});
            writeln!(self.delivered, "{visible}").map_err(|e| e.to_string())?;
            self.inner.decide(&messages, max_tokens)
        }
    }
    fn private_file(path: &std::path::Path) -> std::io::Result<File> {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }
    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<String> = std::env::args().collect();
        if args.len() != 3 || args[1] != "--run-live" {
            return Err("Opt in with: --run-live EXISTING_CONTROLLED_OUTPUT_DIRECTORY".into());
        }
        let dir = std::path::Path::new(&args[2]);
        if !dir.is_dir() {
            return Err("Output directory must already exist".into());
        }
        let limits = AgentLimits {
            max_decisions: 4,
            max_tool_calls: 1,
            ..AgentLimits::default()
        };
        let manifest = json!({"model":"qwen3-30b-a3b","endpoint":"http://127.0.0.1:1234/v1","adapter":"CellAgent/OpenAiChat","temperature":0.2,"max_decisions":4,"max_tool_calls":1,"max_malformed":2,"max_tokens":1500,"max_message_bytes":65536,"max_conversation_bytes":262144,"task":TASK,"clarification":CLARIFICATION,"order":["1-control","1-variant","2-control","2-variant","3-control","3-variant"]});
        writeln!(private_file(&dir.join("manifest.json"))?, "{manifest}")?;
        for pair in 1..=3 {
            for variant in [false, true] {
                let name = format!("{pair}-{}", if variant { "variant" } else { "control" });
                let calls = Arc::new(Mutex::new(Vec::new()));
                let fixture = Fixture {
                    calls: calls.clone(),
                    tools: vec![
                        ToolDefinition::new(
                            "fixture.inspect",
                            "Read the sealed-card resource and return its answer field.",
                        )
                        .effect(ToolEffect::Read)
                        .input(ToolInputDefinition::required(
                            "resource",
                            ToolInputKind::String,
                            "Resource identifier: sealed-card.",
                        )),
                    ],
                };
                let mut session = AgentSession::builder().provider(fixture).build()?;
                let mut model = Model {
                    inner: CellAgent::new(AgentConfig::local(
                        "qwen3-30b-a3b",
                        "http://127.0.0.1:1234/v1",
                    ))?,
                    variant,
                    delivered: private_file(&dir.join(format!("{name}.messages.jsonl")))?,
                };
                let mut trace =
                    ProtocolTrace::new(private_file(&dir.join(format!("{name}.trace.jsonl")))?);
                let mut policy_events = Vec::new();
                let result = run_session_traced(
                    &mut model,
                    &mut session,
                    TASK,
                    limits,
                    |definition, call| {
                        let allowed =
                            read_only_policy(definition, call) && call.name == "fixture.inspect";
                        policy_events.push(json!({"name":call.name,"allowed":allowed}));
                        allowed
                    },
                    &mut trace,
                );
                let outcome = match result {
                    Ok(run) => {
                        json!({"stop":format!("{:?}",run.stop),"content":run.content,"decisions":run.decisions,"tool_calls":run.tool_calls,"malformed":run.malformed,"task_success":run.stop == scope_ide::agent::AgentStop::Completed && run.content.trim() == "cobalt-orchid-731" && calls.lock().unwrap().len() == 1})
                    }
                    Err(error) => json!({"backend_or_runner_error":error.to_string()}),
                };
                let record = json!({"run":name,"outcome":outcome,"provider_calls":*calls.lock().unwrap(),"policy_events":policy_events,"trace_failed":trace.failed(),"source_mutation":"impossible through fixture: no source or write capability installed"});
                writeln!(
                    private_file(&dir.join(format!("{name}.result.json")))?,
                    "{record}"
                )?;
                println!("{record}");
                if trace.failed() {
                    return Err("trace capture failed; stopping rather than losing evidence".into());
                }
            }
        }
        Ok(())
    }
}
#[cfg(feature = "agent")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    experiment::run()
}
#[cfg(not(feature = "agent"))]
fn main() {
    eprintln!("Requires --features agent and explicit --run-live");
}
