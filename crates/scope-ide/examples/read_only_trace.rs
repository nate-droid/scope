//! Offline synthetic protocol fixture, not Forth execution or language evidence.
//! Run: cargo run --offline -p scope-ide --features agent --example read_only_trace

#[cfg(feature = "agent")]
mod fixture {
    use scope_ide::agent::{
        AgentLimits, AgentStop, DecisionMessage, DecisionModel, ProtocolTrace, read_only_policy,
        run_session_traced,
    };
    use scope_ide::tool::{
        AgentSession, ToolCall, ToolDefinition, ToolEffect, ToolError, ToolInputDefinition,
        ToolInputKind, ToolProvider, ToolResult, ToolValue,
    };

    struct FixtureProvider {
        tools: Vec<ToolDefinition>,
    }

    impl FixtureProvider {
        fn new() -> Self {
            Self {
                tools: vec![
                    ToolDefinition::new(
                        "fixture.inspect_word",
                        "Return synthetic fixture evidence for alpha, beta, or gamma; not actual Forth inspection.",
                    )
                    .effect(ToolEffect::Read)
                    .input(ToolInputDefinition::required(
                        "word",
                        ToolInputKind::String,
                        "Synthetic fixture word: alpha, beta, or gamma.",
                    )),
                ],
            }
        }
    }

    impl ToolProvider for FixtureProvider {
        fn tools(&self) -> &[ToolDefinition] {
            &self.tools
        }

        fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
            let word = call.arguments.get("word").and_then(ToolValue::as_str);
            let evidence = match (call.name.as_str(), word) {
                ("fixture.inspect_word", Some("alpha")) => {
                    "Synthetic alpha evidence: fixture entry one."
                }
                ("fixture.inspect_word", Some("beta")) => {
                    "Synthetic beta evidence: fixture entry two."
                }
                ("fixture.inspect_word", Some("gamma")) => {
                    "Synthetic gamma evidence: fixture entry three."
                }
                _ => {
                    return Err(ToolError::Execution {
                        tool: call.name,
                        message: "expected a fixture word: alpha, beta, or gamma".into(),
                    });
                }
            };
            Ok(ToolResult::new(ToolValue::object([
                ("word", word.expect("matched fixture word").into()),
                ("evidence", evidence.into()),
                (
                    "provenance",
                    "Synthetic native fixture; not actual Forth, source inspection, or execution."
                        .into(),
                ),
            ])))
        }
    }

    struct FakeModel {
        next: usize,
    }

    impl DecisionModel for FakeModel {
        fn decide(
            &mut self,
            _messages: &[DecisionMessage],
            _max_tokens: u32,
        ) -> Result<String, String> {
            let response = match self.next {
                0 => {
                    r#"{"type":"tool","name":"fixture.inspect_word","arguments":{"word":"alpha"}}"#
                }
                1 => r#"{"type":"tool","name":"fixture.inspect_word","arguments":{"word":"beta"}}"#,
                2 => {
                    r#"{"type":"tool","name":"fixture.inspect_word","arguments":{"word":"gamma"}}"#
                }
                // This is a literal newline inside a JSON string, not an escaped newline.
                3 => {
                    "{\"type\":\"complete\",\"content\":\"Synthetic inspection complete.\nThis literal newline is invalid JSON.\"}"
                }
                4 => {
                    r#"{"type":"complete","content":"Inspected alpha, beta, and gamma successfully using synthetic fixture evidence only, not actual Forth. My previous completion contained a literal newline and caused a protocol error; this valid completion corrects it. No network requests or source writes occurred."}"#
                }
                _ => return Err("deterministic fixture sequence exhausted".into()),
            };
            self.next += 1;
            Ok(response.into())
        }
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = AgentSession::builder()
            .provider(FixtureProvider::new())
            .build()?;
        let mut model = FakeModel { next: 0 };
        let mut trace = ProtocolTrace::new(std::io::stdout().lock());
        let run = run_session_traced(
            &mut model,
            &mut session,
            "Inspect alpha, beta, and gamma using synthetic fixture evidence only. Do not use actual Forth, network access, or source writes.",
            AgentLimits::default(),
            read_only_policy,
            &mut trace,
        )?;
        trace.record_presentation(&run.content);
        assert_eq!(run.stop, AgentStop::Completed);
        assert_eq!((run.decisions, run.tool_calls, run.malformed), (5, 3, 1));
        if trace.failed() {
            return Err(std::io::Error::other("protocol trace stdout write failed").into());
        }
        Ok(())
    }
}

#[cfg(feature = "agent")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    fixture::run()
}

#[cfg(not(feature = "agent"))]
fn main() {
    eprintln!("Run with --features agent");
}
