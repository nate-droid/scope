use super::*;
use crate::tool::{ToolError, ToolProvider, ToolResult};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct FakeModel {
    replies: VecDeque<Result<String, String>>,
    conversations: Vec<Vec<(String, String)>>,
}
impl FakeModel {
    fn new(replies: &[&str]) -> Self {
        Self {
            replies: replies.iter().map(|text| Ok((*text).into())).collect(),
            conversations: Vec::new(),
        }
    }
}
impl DecisionModel for FakeModel {
    fn decide(&mut self, messages: &[Message], max_tokens: u32) -> Result<String, String> {
        assert!(max_tokens > 0);
        self.conversations.push(
            messages
                .iter()
                .map(|m| (m.role.clone(), m.content.clone()))
                .collect(),
        );
        self.replies.pop_front().expect("unexpected model request")
    }
}
struct Echo {
    tools: Vec<ToolDefinition>,
    calls: Arc<AtomicUsize>,
}
impl ToolProvider for Echo {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if call.name == "fail" {
            return Err(ToolError::Conflict {
                tool: call.name,
                path: "fixture".into(),
                current_revision: Some("new".into()),
                message: "stale revision".into(),
            });
        }
        Ok(ToolResult::new(ToolValue::Object(call.arguments)))
    }
}
fn session() -> (AgentSession<'static>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Echo {
        calls: calls.clone(),
        tools: vec![
            ToolDefinition::new("echo", "echo arguments"),
            ToolDefinition::new("write", "mutation").effect(ToolEffect::Write),
            ToolDefinition::new("fail", "conflict"),
        ],
    };
    (
        AgentSession::builder().provider(provider).build().unwrap(),
        calls,
    )
}
#[test]
fn protocol_failures_are_not_language_diagnostics() {
    for text in ["", "  ", "{\"type\":"] {
        let error = parse_decision(text).unwrap_err();
        assert_eq!(error.category(), "incomplete_response");
        assert!(error.to_string().contains("not a language diagnostic"));
    }
    assert_eq!(
        parse_decision("not json").unwrap_err().category(),
        "malformed_response"
    );
}

const MALFORMED_MULTILINE: &str = "{\"type\":\"complete\",\"content\":\"first\nsecond\"}";
const MULTILINE_COMPLETE: &str = r#"{"type":"complete","content":"first\nsecond"}"#;

#[test]
fn schema_errors_are_invalid_decisions_and_recover_without_dispatch() {
    for text in [
        r#"{"type":"complete","content":"done","extra":true}"#,
        r#"{"type":"complete"}"#,
        r#"{"type":"complete","content":42}"#,
        r#"{"type":"other","content":"done"}"#,
        r#"{"type":"tool","name":"echo","arguments":[]}"#,
        r#"{"type":"tool","name":"echo","arguments":{"x":1.5}}"#,
        r#"{"type":"tool","name":"echo","arguments":{"x":1,"x":2}}"#,
    ] {
        assert_eq!(
            parse_decision(text).unwrap_err().category(),
            "invalid_decision"
        );
        let (mut session, calls) = session();
        let mut model = FakeModel::new(&[text, COMPLETE]);
        let run = run_session(
            &mut model,
            &mut session,
            "inspect",
            AgentLimits::default(),
            read_only_policy,
        )
        .unwrap();
        assert_eq!((run.decisions, run.tool_calls, run.malformed), (2, 0, 1));
        assert_eq!(run.stop, AgentStop::Completed);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let feedback: Value =
            serde_json::from_str(&model.conversations[1].last().unwrap().1).unwrap();
        assert_eq!(feedback["category"], "invalid_decision");
    }
}

#[test]
fn decision_framing_failures_never_dispatch() {
    let invalid = [
        "{\"type\":\"complete\",\"content\":\"first\nsecond\"}",
        r#"{"type":"tool","name":"echo","arguments":{}} {"type":"complete","content":"done"}"#,
        "```json\n{\"type\":\"complete\",\"content\":\"done\"}\n```",
        r#"Here is the decision: {"type":"complete","content":"done"}"#,
    ];
    for text in invalid {
        assert_eq!(
            parse_decision(text).unwrap_err().category(),
            "malformed_response"
        );
        let (mut session, calls) = session();
        let mut model = FakeModel::new(&[text, COMPLETE]);
        let run = run_session(
            &mut model,
            &mut session,
            "inspect",
            AgentLimits::default(),
            read_only_policy,
        )
        .unwrap();
        assert_eq!(run.stop, AgentStop::Completed);
        assert_eq!((run.decisions, run.tool_calls, run.malformed), (2, 0, 1));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let feedback: Value =
            serde_json::from_str(&model.conversations[1].last().unwrap().1).unwrap();
        assert_eq!(feedback["kind"], "protocol_error");
        assert_eq!(feedback["category"], "malformed_response");
    }
    match parse_decision(r#"{"type":"complete","content":"first\nsecond"}"#).unwrap() {
        Decision::Complete { content } => assert_eq!(content, "first\nsecond"),
        _ => panic!("expected completion"),
    }
}

#[test]
fn default_malformed_budget_stops_on_third_failure_without_completion() {
    let (mut session, calls) = session();
    let mut model = FakeModel::new(&[MALFORMED_MULTILINE; 3]);
    let run = run_session(
        &mut model,
        &mut session,
        "inspect",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::MalformedBudget);
    assert_eq!((run.decisions, run.tool_calls, run.malformed), (3, 0, 3));
    assert!(run.content.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(model.conversations.len(), 3);
}

#[test]
fn recovery_after_successful_write_never_replays_the_write() {
    let (mut session, calls) = session();
    let mut model = FakeModel::new(&[
        r#"{"type":"tool","name":"write","arguments":{}}"#,
        "",
        COMPLETE,
    ]);
    let run = run_session(
        &mut model,
        &mut session,
        "propose",
        AgentLimits::default(),
        |_, _| true,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::Completed);
    assert_eq!(run.malformed, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let feedback = &model.conversations[2].last().unwrap().1;
    assert!(feedback.contains("incomplete_response"));
}

// Q1 experiment only: the production system prompt and parser stay unchanged.
const Q1_CLARIFICATION: &str = "Return exactly one JSON decision per response. Do not return multiple JSON objects, arrays of decisions, or a sequence of actions. If you need to inspect a resource before another action, return only the tool decision for that inspection now. You will receive its tool_result before deciding what to do next. Use the existing tool or complete schema.";

struct Q1Model {
    inner: FakeModel,
    clarified: bool,
}
impl DecisionModel for Q1Model {
    fn decide(&mut self, messages: &[Message], max_tokens: u32) -> Result<String, String> {
        let mut delivered = messages.to_vec();
        if self.clarified {
            assert_eq!(delivered[0].role, "system");
            delivered[0].content.push('\n');
            delivered[0].content.push_str(Q1_CLARIFICATION);
        }
        self.inner.decide(&delivered, max_tokens)
    }
}

#[test]
fn q1_single_decision_control_and_instruction_only_variant() {
    for clarified in [false, true] {
        let (mut session, calls) = session();
        let mut model = Q1Model {
            inner: FakeModel::new(&[ECHO, COMPLETE]),
            clarified,
        };
        let run = run_session(
            &mut model,
            &mut session,
            "inspect",
            AgentLimits::default(),
            read_only_policy,
        )
        .unwrap();
        assert_eq!(
            (run.stop, run.tool_calls, run.malformed),
            (AgentStop::Completed, 1, 0)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let expected = if clarified {
            format!("{PROTOCOL}\n{Q1_CLARIFICATION}")
        } else {
            PROTOCOL.to_owned()
        };
        assert_eq!(model.inner.conversations[0][0].1, expected);
        let feedback: Value =
            serde_json::from_str(&model.inner.conversations[1].last().unwrap().1).unwrap();
        assert_eq!(feedback["kind"], "tool_result");
        assert_eq!(feedback["ok"], true);
    }
}

#[test]
fn q1_multi_decisions_never_reach_authorization_or_mutation() {
    let write = r#"{"type":"tool","name":"write","arguments":{}}"#;
    let responses = [
        format!("{write}{ECHO}"),
        format!("[{write},{ECHO}]"),
        format!("```json\n{write}\n```\n```json\n{ECHO}\n```"),
        format!(r#"{{"type":"tool","name":"write","arguments":{{}},"actions":[{ECHO}]}}"#),
    ];
    for clarified in [false, true] {
        for (index, response) in responses.iter().enumerate() {
            let category = if index == 1 || index == 3 {
                "invalid_decision"
            } else {
                "malformed_response"
            };
            assert_eq!(parse_decision(response).unwrap_err().category(), category);
            for recover in [false, true] {
                let (mut session, calls) = session();
                let replies = if recover {
                    vec![response.as_str(), COMPLETE]
                } else {
                    vec![response.as_str(); 3]
                };
                let mut model = Q1Model {
                    inner: FakeModel::new(&replies),
                    clarified,
                };
                let mut policy_calls = 0;
                let run = run_session(
                    &mut model,
                    &mut session,
                    "inspect",
                    AgentLimits::default(),
                    |_, _| {
                        policy_calls += 1;
                        true // Even authorized writes must never dispatch from malformed output.
                    },
                )
                .unwrap();
                assert_eq!(
                    run.stop,
                    if recover {
                        AgentStop::Completed
                    } else {
                        AgentStop::MalformedBudget
                    }
                );
                assert_eq!(run.malformed, if recover { 1 } else { 3 });
                assert_eq!(run.tool_calls, 0);
                assert_eq!(policy_calls, 0);
                // This provider's sole invocation entry point includes all write effects.
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                let feedback: Value =
                    serde_json::from_str(&model.inner.conversations[1].last().unwrap().1).unwrap();
                assert_eq!(feedback["category"], category);
            }
        }
    }
}

const COMPLETE: &str = r#"{"type":"complete","content":"done"}"#;
const ECHO: &str = r#"{"type":"tool","name":"echo","arguments":{"path":"approved","nested":[null,true,-2,{"text":"héllo"}]}}"#;

#[test]
fn discovery_feedback_and_completion() {
    let (mut session, calls) = session();
    let mut model = FakeModel::new(&[ECHO, COMPLETE]);
    let run = run_session(
        &mut model,
        &mut session,
        "inspect",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::Completed);
    assert_eq!(run.content, "done");
    assert_eq!((run.decisions, run.tool_calls), (2, 1));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let initial: Value = serde_json::from_str(&model.conversations[0][1].1).unwrap();
    assert_eq!(initial["catalog"]["tools"][0]["definition"]["name"], "echo");
    let messages = &model.conversations[1];
    assert_eq!(messages[2], ("assistant".into(), ECHO.into()));
    assert_eq!(messages[3].0, "user");
    let feedback: Value = serde_json::from_str(&messages[3].1).unwrap();
    assert_eq!(feedback["ok"], true);
    assert_eq!(feedback["value"]["nested"][3]["text"], "héllo");
}

#[test]
fn denied_unknown_and_conflict_are_feedback_not_completion() {
    let (mut session, calls) = session();
    let mut model = FakeModel::new(&[
        r#"{"type":"tool","name":"write","arguments":{}}"#,
        r#"{"type":"tool","name":"missing","arguments":{}}"#,
        r#"{"type":"tool","name":"fail","arguments":{}}"#,
        COMPLETE,
    ]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::Completed);
    assert_eq!(run.tool_calls, 3);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    for index in 1..=3 {
        let feedback: Value =
            serde_json::from_str(&model.conversations[index].last().unwrap().1).unwrap();
        assert_eq!(feedback["ok"], false);
        assert!(feedback["error"].as_str().unwrap().len() > 3);
    }
}

#[test]
fn call_policy_inspects_arguments_and_explicitly_allows_writes() {
    let (mut session, calls) = session();
    let mut model = FakeModel::new(&[
        ECHO,
        r#"{"type":"tool","name":"write","arguments":{"path":"fixture"}}"#,
        COMPLETE,
    ]);
    let mut checked = 0;
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits::default(),
        |definition, call| {
            checked += 1;
            definition.effect == ToolEffect::Write
                && call.arguments.get("path").and_then(ToolValue::as_str) == Some("fixture")
        },
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::Completed);
    assert_eq!(checked, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn strict_protocol_rejects_invalid_decisions() {
    for text in [
        r#"{"type":"tool","name":"echo","arguments":{"path":"one","path":"two"}}"#,
        r#"{"type":"tool","name":"echo","arguments":{"nested":{"n":1,"n":2}}}"#,
        r#"{"type":"tool","name":"echo","arguments":{"n":1.0}}"#,
        "not json",
        "```json\n{}\n```",
        "[]",
        "{}",
        r#"{"type":"complete","content":" "}"#,
        r#"{"type":"complete","content":"done","extra":true}"#,
        r#"{"type":"complete","content":"a","content":"b"}"#,
        r#"{"type":"tool","name":"echo","arguments":{"n":1.5}}"#,
        r#"{"type":"tool","name":"echo","arguments":{"n":18446744073709551615}}"#,
        r#"{"type":"tool","name":"","arguments":{}}"#,
        r#"{"type":"tool","name":"echo","arguments":[]} "#,
        r#"{"type":"complete","content":"done"} {}"#,
    ] {
        assert!(parse_decision(text).is_err(), "accepted {text}");
    }
}

#[test]
fn malformed_recovery_and_exhaustion_never_dispatch() {
    let (mut session, calls) = session();
    let mut model = FakeModel::new(&["bad", COMPLETE]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::Completed);
    assert_eq!(run.malformed, 1);
    assert!(
        model.conversations[1]
            .last()
            .unwrap()
            .1
            .contains("protocol_error")
    );
    let mut model = FakeModel::new(&["bad", "bad"]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits {
            max_malformed: 1,
            ..AgentLimits::default()
        },
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::MalformedBudget);
    assert_eq!(run.decisions, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn decision_and_tool_budgets_bound_dispatch() {
    let (mut session, calls) = session();
    let mut model = FakeModel::new(&[]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits {
            max_decisions: 0,
            ..AgentLimits::default()
        },
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::DecisionBudget);
    assert!(model.conversations.is_empty());
    let mut model = FakeModel::new(&[ECHO, ECHO]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits {
            max_tool_calls: 1,
            ..AgentLimits::default()
        },
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::ToolBudget);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut model = FakeModel::new(&[ECHO]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits {
            max_decisions: 1,
            ..AgentLimits::default()
        },
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::DecisionBudget);
    // A final answer is still allowed after spending the tool budget.
    let mut model = FakeModel::new(&[COMPLETE]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits {
            max_tool_calls: 0,
            ..AgentLimits::default()
        },
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::Completed);
}

#[test]
fn message_conversation_and_model_failures() {
    let (mut session, calls) = session();
    for (limits, expected) in [
        (
            AgentLimits {
                max_message_bytes: 1,
                ..AgentLimits::default()
            },
            AgentStop::MessageBudget,
        ),
        (
            AgentLimits {
                max_conversation_bytes: 1,
                ..AgentLimits::default()
            },
            AgentStop::ConversationBudget,
        ),
    ] {
        let mut model = FakeModel::new(&[]);
        assert_eq!(
            run_session(&mut model, &mut session, "task", limits, read_only_policy)
                .unwrap()
                .stop,
            expected
        );
        assert!(model.conversations.is_empty());
    }
    let oversized = "x".repeat(AgentLimits::default().max_message_bytes + 1);
    let mut model = FakeModel::new(&[&oversized]);
    assert_eq!(
        run_session(
            &mut model,
            &mut session,
            "task",
            AgentLimits::default(),
            read_only_policy
        )
        .unwrap()
        .stop,
        AgentStop::MessageBudget
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut model = FakeModel::new(&[]);
    model
        .replies
        .push_back(Err("fake model unavailable".into()));
    let error = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap_err();
    assert!(error.to_string().contains("fake model unavailable"));
}

#[test]
fn conversation_growth_stops_before_dispatch_or_next_model_call() {
    let (mut session, calls) = session();
    let mut probe = FakeModel::new(&[COMPLETE]);
    run_session(
        &mut probe,
        &mut session,
        "task",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap();
    let initial_bytes = probe.conversations[0]
        .iter()
        .map(|(_, text)| text.len())
        .sum::<usize>();
    let mut model = FakeModel::new(&[ECHO]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits {
            max_conversation_bytes: initial_bytes + ECHO.len() - 1,
            ..AgentLimits::default()
        },
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::ConversationBudget);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut model = FakeModel::new(&[ECHO]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits {
            max_conversation_bytes: initial_bytes + ECHO.len(),
            ..AgentLimits::default()
        },
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::ConversationBudget);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(model.conversations.len(), 1);
    let mut model = FakeModel::new(&[]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits {
            max_tokens_per_decision: 0,
            ..AgentLimits::default()
        },
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::DecisionBudget);
    assert!(model.conversations.is_empty());
}

#[test]
fn oversized_tool_feedback_stops_without_rollback_or_another_model_call() {
    struct Large(Vec<ToolDefinition>);
    impl ToolProvider for Large {
        fn tools(&self) -> &[ToolDefinition] {
            &self.0
        }
        fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new(ToolValue::String(
                "x".repeat(AgentLimits::default().max_message_bytes),
            )))
        }
    }
    let mut session = AgentSession::builder()
        .provider(Large(vec![ToolDefinition::new("large", "large result")]))
        .build()
        .unwrap();
    let mut model = FakeModel::new(&[r#"{"type":"tool","name":"large","arguments":{}}"#]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::MessageBudget);
    assert_eq!(run.tool_calls, 1);
    assert_eq!(model.conversations.len(), 1);
}

#[test]
fn trace_associates_responses_feedback_and_preserves_execution() {
    let replies = [ECHO, ECHO, ECHO, MALFORMED_MULTILINE, MULTILINE_COMPLETE];
    let (mut plain_session, plain_calls) = session();
    let mut plain_model = FakeModel::new(&replies);
    let plain = run_session(
        &mut plain_model,
        &mut plain_session,
        "inspect sequentially",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap();
    let (mut traced_session, traced_calls) = session();
    let mut traced_model = FakeModel::new(&replies);
    let mut bytes = Vec::new();
    {
        let mut trace = super::super::ProtocolTrace::new(&mut bytes);
        let traced = run_session_traced(
            &mut traced_model,
            &mut traced_session,
            "inspect sequentially",
            AgentLimits::default(),
            read_only_policy,
            &mut trace,
        )
        .unwrap();
        assert_eq!(plain, traced);
        trace.record_presentation(&traced.content);
        assert!(!trace.failed());
    }
    assert_eq!(plain_model.conversations, traced_model.conversations);
    assert_eq!(plain_calls.load(Ordering::SeqCst), 3);
    assert_eq!(traced_calls.load(Ordering::SeqCst), 3);
    let events: Vec<Value> = std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for (index, expected) in replies.iter().enumerate() {
        let ordinal = index + 1;
        let matching: Vec<_> = events.iter().filter(|e| e["ordinal"] == ordinal).collect();
        assert_eq!(matching[0]["event"], "request");
        let recorded: Vec<(String, String)> = matching[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["role"].as_str().unwrap().into(),
                    m["content"].as_str().unwrap().into(),
                )
            })
            .collect();
        assert_eq!(recorded, traced_model.conversations[index]);
        assert_eq!(matching[1]["event"], "response");
        assert_eq!(matching[1]["raw"], *expected);
        assert_eq!(matching[2]["event"], "decision");
        assert_eq!(matching[2]["accepted"], index != 3);
    }
    let correction = events
        .iter()
        .find(|e| e["event"] == "feedback" && e["ordinal"] == 4)
        .unwrap();
    assert_eq!(plain.content, "first\nsecond");
    assert_eq!(correction["envelope"]["category"], "malformed_response");
    let instruction = correction["envelope"]["instruction"].as_str().unwrap();
    assert!(instruction.contains(r#"newlines inside JSON strings as \n"#));
    assert!(instruction.contains("exactly one complete JSON decision"));
    assert_eq!(correction["envelope"]["kind"], "protocol_error");
    assert_eq!(correction["retained"], true);
    assert_eq!(events[events.len() - 2]["event"], "outcome");
    assert_eq!(events.last().unwrap()["event"], "presentation");
}

#[test]
fn tracing_is_disabled_by_default_and_writer_failures_are_nonfatal() {
    struct NoMetadata(FakeModel);
    impl DecisionModel for NoMetadata {
        fn trace_settings(&self) -> Option<Value> {
            panic!("disabled trace requested metadata")
        }
        fn decide(&mut self, messages: &[Message], max_tokens: u32) -> Result<String, String> {
            self.0.decide(messages, max_tokens)
        }
    }
    let (mut session, _) = session();
    let plain = run_session(
        &mut NoMetadata(FakeModel::new(&[COMPLETE])),
        &mut session,
        "task",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap();
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut trace = super::super::ProtocolTrace::new(Broken);
    let traced = run_session_traced(
        &mut FakeModel::new(&[COMPLETE]),
        &mut session,
        "task",
        AgentLimits::default(),
        read_only_policy,
        &mut trace,
    )
    .unwrap();
    assert!(trace.failed());
    assert_eq!(plain, traced);
}

#[test]
fn trace_captures_transport_and_budget_stops() {
    let (mut session, calls) = session();
    for oversized in [false, true] {
        let mut model = FakeModel::new(&[]);
        model.replies.push_back(if oversized {
            Ok("x".repeat(AgentLimits::default().max_message_bytes + 1))
        } else {
            Err("transport failed".into())
        });
        let mut bytes = Vec::new();
        let mut trace = super::super::ProtocolTrace::new(&mut bytes);
        let result = run_session_traced(
            &mut model,
            &mut session,
            "task",
            AgentLimits::default(),
            read_only_policy,
            &mut trace,
        );
        if oversized {
            assert_eq!(result.unwrap().stop, AgentStop::MessageBudget);
        } else {
            assert!(result.is_err());
        }
        let events: Vec<Value> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(events.len(), 3);
        assert_eq!(events[1]["event"], "response");
        assert_eq!(events[2]["event"], "outcome");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn builtin_read_and_denied_edit_preserve_active_buffer_and_disk() {
    use crate::{app::App, test_support::TempWorkspace};
    let root = TempWorkspace::new("decision_loop_read_only");
    root.write_file("main.scope", "fn original() {}\n");
    let mut app = App::new(root.workspace()).unwrap();
    app.open_path("main.scope").unwrap();
    let original = app.open_document().unwrap().text().to_owned();
    let mut session = AgentSession::for_app(&mut app).unwrap();
    let mut model = FakeModel::new(&[
        r#"{"type":"tool","name":"scope.document.read","arguments":{"path":"main.scope"}}"#,
        r#"{"type":"tool","name":"scope.document.replace","arguments":{"path":"main.scope","replacement":"changed"}}"#,
        COMPLETE,
    ]);
    let run = run_session(
        &mut model,
        &mut session,
        "task",
        AgentLimits::default(),
        read_only_policy,
    )
    .unwrap();
    assert_eq!(run.stop, AgentStop::Completed);
    assert!(model.conversations[2].last().unwrap().1.contains("denied"));
    drop(session);
    assert_eq!(app.open_document().unwrap().text(), original);
    assert_eq!(
        app.workspace().load_file("main.scope").unwrap().contents,
        original
    );
}
