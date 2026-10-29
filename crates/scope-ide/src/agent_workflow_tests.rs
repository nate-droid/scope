use super::*;
use crate::tool::ToolProvider;

struct Model {
    decisions: usize,
}
impl DecisionModel for Model {
    fn decide(&mut self, messages: &[Message], _: u32) -> Result<String, String> {
        assert!(messages[1].content.contains("host.workflow"));
        self.decisions += 1;
        Ok(if self.decisions <= 2 {
            r#"{"type":"tool","name":"host.workflow","arguments":{}}"#
        } else {
            r#"{"type":"complete","content":"done"}"#
        }
        .into())
    }
}
struct Provider(Vec<ToolDefinition>);
impl ToolProvider for Provider {
    fn tools(&self) -> &[ToolDefinition] {
        &self.0
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        assert_eq!(call.name, "read");
        let value = if call.arguments.contains_key("large") {
            "x".repeat(10000)
        } else {
            "value".into()
        };
        Ok(ToolResult::new(value.into()))
    }
}
struct Workflow {
    names: Vec<&'static str>,
    results: Vec<bool>,
    executions: usize,
    payload: usize,
}
impl AgentWorkflow for Workflow {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("host.workflow", "host compound read")
    }
    fn execute(
        &mut self,
        _: ToolCall,
        context: &mut AgentWorkflowContext<'_, '_>,
    ) -> Result<ToolResult, ToolError> {
        self.executions += 1;
        for name in &self.names {
            let call =
                ToolCall::new(*name).with_argument("path", "blocked".repeat(self.payload).into());
            self.results.push(context.invoke(call).is_ok());
        }
        Ok(ToolResult::new("workflow result".into()))
    }
}
fn session() -> AgentSession<'static> {
    AgentSession::builder()
        .provider(Provider(vec![
            ToolDefinition::new("read", "read"),
            ToolDefinition::new("write", "write").effect(ToolEffect::Write),
        ]))
        .build()
        .unwrap()
}
fn workflow(names: Vec<&'static str>) -> Workflow {
    Workflow {
        names,
        results: vec![],
        executions: 0,
        payload: 1,
    }
}
#[test]
fn discovery_repeated_calls_and_call_aware_policy() {
    let mut workflow = workflow(vec!["read", "write", "missing", "host.workflow"]);
    let mut seen = Vec::new();
    let run = run_session_with_workflows(
        &mut Model { decisions: 0 },
        &mut session(),
        "inspect",
        AgentLimits {
            max_tool_calls: 20,
            ..AgentLimits::default()
        },
        |definition, call| {
            seen.push(call.name.clone());
            definition.name == "host.workflow"
                || call.arguments.get("path").and_then(ToolValue::as_str) != Some("blocked")
        },
        &mut [&mut workflow],
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::Completed);
    assert_eq!(run.tool_calls, 10);
    assert_eq!(workflow.executions, 2);
    assert_eq!(workflow.results, vec![false; 8]);
    assert_eq!(
        seen,
        [
            "host.workflow",
            "read",
            "write",
            "host.workflow",
            "read",
            "write"
        ]
    );
}
#[test]
fn read_only_gate_and_shared_latched_budget() {
    let mut workflow = workflow(vec!["read", "write", "missing", "read", "read"]);
    let run = run_session_with_workflows(
        &mut Model { decisions: 0 },
        &mut session(),
        "inspect",
        AgentLimits {
            max_tool_calls: 4,
            ..AgentLimits::default()
        },
        |_, _| true,
        &mut [&mut workflow],
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::ToolBudget);
    assert_eq!(run.tool_calls, 4);
    assert_eq!(run.decisions, 1);
    assert_eq!(workflow.results, [true, false, false, false, false]);
}
#[test]
fn outer_denial_prevents_execution() {
    let mut workflow = workflow(vec!["read"]);
    let run = run_session_with_workflows(
        &mut Model { decisions: 0 },
        &mut session(),
        "inspect",
        AgentLimits::default(),
        |_, _| false,
        &mut [&mut workflow],
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::Completed);
    assert_eq!(run.tool_calls, 2);
    assert_eq!(workflow.executions, 0);
}
#[test]
fn inner_argument_bytes_latch() {
    let mut workflow = workflow(vec!["read", "read"]);
    workflow.payload = 5000;
    let run = run_session_with_workflows(
        &mut Model { decisions: 0 },
        &mut session(),
        "inspect",
        AgentLimits {
            max_message_bytes: 8192,
            ..AgentLimits::default()
        },
        |_, _| true,
        &mut [&mut workflow],
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::MessageBudget);
    assert_eq!(run.tool_calls, 2);
    assert_eq!(workflow.results, [false, false]);
}
#[test]
fn cumulative_inner_bytes_stop_run() {
    let mut workflow = workflow(vec!["read"; 10]);
    workflow.payload = 300;
    let run = run_session_with_workflows(
        &mut Model { decisions: 0 },
        &mut session(),
        "inspect",
        AgentLimits {
            max_tool_calls: 30,
            max_message_bytes: 8192,
            max_conversation_bytes: 8192,
            ..AgentLimits::default()
        },
        |_, _| true,
        &mut [&mut workflow],
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::ConversationBudget);
    assert!(workflow.results.iter().any(|result| *result));
    assert!(workflow.results.iter().any(|result| !*result));
}
#[test]
fn oversized_inner_result_is_not_returned_to_workflow() {
    struct Large;
    impl AgentWorkflow for Large {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("host.workflow", "large result")
        }
        fn execute(
            &mut self,
            _: ToolCall,
            context: &mut AgentWorkflowContext<'_, '_>,
        ) -> Result<ToolResult, ToolError> {
            assert!(
                context
                    .invoke(ToolCall::new("read").with_argument("large", true.into()))
                    .is_err()
            );
            assert!(context.invoke(ToolCall::new("read")).is_err());
            Ok(ToolResult::new(ToolValue::Null))
        }
    }
    let run = run_session_with_workflows(
        &mut Model { decisions: 0 },
        &mut session(),
        "inspect",
        AgentLimits {
            max_message_bytes: 8192,
            ..AgentLimits::default()
        },
        |_, _| true,
        &mut [&mut Large],
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::MessageBudget);
    assert_eq!(run.tool_calls, 2);
}
#[test]
fn duplicate_names_rejected_before_model() {
    let mut a = workflow(vec![]);
    let mut b = workflow(vec![]);
    assert!(matches!(
        run_session_with_workflows(
            &mut Model { decisions: 0 },
            &mut session(),
            "inspect",
            AgentLimits::default(),
            |_, _| true,
            &mut [&mut a, &mut b]
        ),
        Err(AgentError::Tool(ToolError::DuplicateTool { .. }))
    ));
}
