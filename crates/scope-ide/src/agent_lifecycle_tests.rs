use super::*;
use crate::agent::{AgentSession, DecisionMessage, DecisionModel, run_session_observed};

struct Model {
    reply: Result<String, String>,
    calls: usize,
    cancel: Option<AgentCancellationToken>,
}
impl DecisionModel for Model {
    fn model_identity(&self) -> Option<String> {
        Some("offline-model".into())
    }
    fn provider_identity(&self) -> Option<String> {
        Some("offline-provider".into())
    }
    fn trace_settings(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({"secret": "do-not-copy"}))
    }
    fn decide(&mut self, _: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.calls += 1;
        if let Some(token) = &self.cancel {
            token.cancel();
        }
        self.reply.clone()
    }
}
fn model() -> Model {
    Model {
        reply: Ok(r#"{"type":"complete","content":"done"}"#.into()),
        calls: 0,
        cancel: None,
    }
}

#[test]
fn repeated_events_are_ordered_and_terminal_is_immutable() {
    let mut turn = AgentTurn::new(None, None, AgentLimits::default());
    let other = AgentTurn::new(None, None, AgentLimits::default());
    assert_ne!(turn.id(), other.id());
    turn.transition(AgentState::Deciding { request_ordinal: 1 })
        .unwrap();
    turn.record(AgentEvidence::Decision { ordinal: 1 }).unwrap();
    turn.transition(AgentState::Deciding { request_ordinal: 2 })
        .unwrap();
    turn.record(AgentEvidence::Decision { ordinal: 2 }).unwrap();
    assert_eq!(turn.transitions().len(), 2);
    assert_eq!(turn.transitions()[1].ordinal, 2);
    assert_eq!(turn.evidence().len(), 2);
    for state in [
        AgentState::Completed,
        AgentState::Failed(AgentFailure::Model),
        AgentState::Cancelled,
        AgentState::BudgetExhausted(AgentStop::ToolBudget),
    ] {
        let mut turn = turn.clone();
        turn.transition(state).unwrap();
        let terminal = turn.clone();
        assert_eq!(
            turn.transition(AgentState::Failed(AgentFailure::Model)),
            Err(AgentLifecycleError::Terminal)
        );
        assert_eq!(
            turn.record(AgentEvidence::Malformed { ordinal: 3 }),
            Err(AgentLifecycleError::Terminal)
        );
        assert_eq!(terminal, turn);
    }
}

#[test]
fn invalid_or_duplicate_transitions_are_explicit_and_do_not_mutate() {
    let mut turn = AgentTurn::new(None, None, AgentLimits::default());
    let ready = turn.clone();
    assert_eq!(
        turn.transition(AgentState::Completed),
        Err(AgentLifecycleError::InvalidTransition)
    );
    assert_eq!(
        turn.transition(AgentState::ToolRunning {
            request_ordinal: 1,
            call_ordinal: 1,
            name: "read".into()
        }),
        Err(AgentLifecycleError::InvalidTransition)
    );
    assert_eq!(turn, ready);
    let deciding = AgentState::Deciding { request_ordinal: 1 };
    turn.transition(deciding.clone()).unwrap();
    let before = turn.clone();
    assert_eq!(
        turn.transition(deciding),
        Err(AgentLifecycleError::DuplicateTransition)
    );
    assert_eq!(
        turn.transition(AgentState::Deciding { request_ordinal: 3 }),
        Err(AgentLifecycleError::InvalidTransition)
    );
    assert_eq!(
        turn.transition(AgentState::BudgetExhausted(AgentStop::Completed)),
        Err(AgentLifecycleError::InvalidTransition)
    );
    assert_eq!(turn, before);
}

#[test]
fn callback_completion_identity_and_serialization() {
    let mut session = AgentSession::builder().build().unwrap();
    let mut snapshots = vec![];
    let run = run_session_observed(
        &mut model(),
        &mut session,
        "private request",
        AgentLimits::default(),
        |_, _| true,
        Some(AgentSourceBinding {
            identity: AgentSourceIdentity::Host("host-source-v1".into()),
            revision: 7,
            language: "scope".into(),
        }),
        &AgentCancellationToken::default(),
        &mut |turn| snapshots.push(turn.clone()),
    )
    .unwrap();
    assert_eq!(snapshots.first().unwrap().state(), &AgentState::Ready);
    assert_eq!(snapshots.last(), Some(&run.turn));
    assert_eq!(run.turn.state(), &AgentState::Completed);
    assert_eq!(run.turn.model_identity(), Some("offline-model"));
    assert_eq!(run.turn.provider_identity(), Some("offline-provider"));
    assert_eq!(
        snapshots.first().unwrap().provider_identity(),
        Some("offline-provider")
    );
    assert_eq!(run.turn.source_binding().unwrap().revision, 7);
    assert_eq!(
        run.turn.source_binding().unwrap().identity,
        AgentSourceIdentity::Host("host-source-v1".into())
    );
    let serialized = serde_json::to_string(&run.turn).unwrap();
    assert!(!serialized.contains("private request"));
    assert!(!serialized.contains("do-not-copy"));
    for snapshot in &snapshots {
        let loaded: AgentTurn =
            serde_json::from_str(&serde_json::to_string(snapshot).unwrap()).unwrap();
        assert_eq!(&loaded, snapshot);
    }
    assert_eq!(run.turn.usage().decisions, run.decisions);
    assert_eq!(run.turn.usage().tool_calls, run.tool_calls);
    assert_eq!(run.turn.usage().malformed, run.malformed);
    assert!(run.turn.usage().conversation_bytes > 0);
    assert!(
        run.turn
            .transitions()
            .iter()
            .all(|transition| transition.turn_id == run.turn.id())
    );
    assert_eq!(
        run.turn.transitions()[0].trigger,
        AgentTrigger::ModelRequested
    );
    assert_eq!(run.turn.transitions()[0].request_ordinal, Some(1));
}

#[test]
fn failures_and_budget_have_distinct_terminal_callbacks() {
    let mut session = AgentSession::builder().build().unwrap();
    let mut failed = model();
    failed.reply = Err("sensitive backend error".into());
    let mut last = None;
    assert!(
        run_session_observed(
            &mut failed,
            &mut session,
            "request",
            AgentLimits::default(),
            |_, _| true,
            None,
            &AgentCancellationToken::default(),
            &mut |turn| last = Some(turn.clone())
        )
        .is_err()
    );
    assert_eq!(
        last.as_ref().unwrap().state(),
        &AgentState::Failed(AgentFailure::Model)
    );
    let failed_turn = last.as_ref().unwrap();
    assert!(matches!(
        failed_turn.evidence().last(),
        Some(AgentEvidence::ModelResponse {
            ordinal: 1,
            outcome: AgentModelResponseOutcome::Error(AgentFailure::Model),
            response_bytes: None,
        })
    ));
    let sanitized = serde_json::to_string(failed_turn).unwrap();
    assert!(!sanitized.contains("sensitive backend error"));
    assert_eq!(
        serde_json::from_str::<AgentTurn>(&sanitized).unwrap(),
        *failed_turn
    );
    let limits = AgentLimits {
        max_decisions: 0,
        ..AgentLimits::default()
    };
    let run = run_session_observed(
        &mut model(),
        &mut session,
        "request",
        limits,
        |_, _| true,
        None,
        &AgentCancellationToken::default(),
        &mut |turn| last = Some(turn.clone()),
    )
    .unwrap();
    assert_eq!(
        run.turn.state(),
        &AgentState::BudgetExhausted(AgentStop::DecisionBudget)
    );
    assert_eq!(last, Some(run.turn));
}

#[test]
fn failure_categories_do_not_retain_error_text() {
    use crate::agent::AgentError;
    let cases = [
        (
            AgentError::Chat("secret backend text".into()),
            AgentFailure::Model,
        ),
        (
            AgentError::ConfigureChat(anyhow::anyhow!("secret endpoint/key")),
            AgentFailure::Configuration,
        ),
        (
            AgentError::Tool(ToolError::DuplicateTool {
                name: "secret tool name".into(),
            }),
            AgentFailure::ToolSetup,
        ),
        (
            AgentError::App(crate::app::AppError::NoOpenDocument),
            AgentFailure::Application,
        ),
        (
            AgentError::Stopped(AgentStop::ToolBudget),
            AgentFailure::Stopped(AgentStop::ToolBudget),
        ),
    ];
    for (error, expected) in cases {
        let category = AgentFailure::from_error(&error);
        assert_eq!(category, expected);
        let json = serde_json::to_string(&AgentState::Failed(category)).unwrap();
        assert!(!json.contains("secret"));
        assert_eq!(
            serde_json::from_str::<AgentState>(&json).unwrap(),
            AgentState::Failed(category)
        );
    }
}

#[test]
fn cell_provider_identity_is_explicit_and_never_endpoint_or_key() {
    use crate::agent::{AgentConfig, CellAgent};
    let mut model = CellAgent::new(
        AgentConfig::local("safe-model", "http://127.0.0.1:9/v1").with_api_key("secret-key"),
    )
    .unwrap();
    let mut session = AgentSession::builder().build().unwrap();
    let run = run_session_observed(
        &mut model,
        &mut session,
        "request",
        AgentLimits {
            max_decisions: 0,
            ..AgentLimits::default()
        },
        |_, _| true,
        None,
        &AgentCancellationToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(run.turn.provider_identity(), Some("cell-capability-openai"));
    assert_eq!(run.turn.model_identity(), Some("safe-model"));
    let json = serde_json::to_string(&run.turn).unwrap();
    assert!(!json.contains("127.0.0.1"));
    assert!(!json.contains("secret-key"));
}

#[test]
fn response_metadata_survives_post_call_cancellation_without_text() {
    let mut session = AgentSession::builder().build().unwrap();
    for reply in [Ok("é🙂".to_owned()), Err("secret-backend-error".to_owned())] {
        let expected = match &reply {
            Ok(text) => (AgentModelResponseOutcome::Returned, Some(text.len())),
            Err(_) => (AgentModelResponseOutcome::Error(AgentFailure::Model), None),
        };
        let token = AgentCancellationToken::default();
        let mut model = Model {
            reply,
            calls: 0,
            cancel: Some(token.clone()),
        };
        let mut snapshots = vec![];
        let run = run_session_observed(
            &mut model,
            &mut session,
            "request",
            AgentLimits::default(),
            |_, _| true,
            None,
            &token,
            &mut |turn| snapshots.push(turn.clone()),
        )
        .unwrap();
        assert_eq!(run.stop, AgentStop::Cancelled);
        assert_eq!(model.calls, 1);
        assert!(
            matches!(run.turn.evidence().last(), Some(AgentEvidence::ModelResponse { ordinal: 1, outcome, response_bytes })
            if (*outcome, *response_bytes) == expected)
        );
        assert!(snapshots.iter().any(|turn| matches!(
            turn.state(),
            AgentState::Deciding { request_ordinal: 1 }
        ) && matches!(
            turn.evidence().last(),
            Some(AgentEvidence::ModelResponse { .. })
        )));
        let json = serde_json::to_string(&run.turn).unwrap();
        assert!(!json.contains("é🙂"));
        assert!(!json.contains("secret-backend-error"));
        assert_eq!(serde_json::from_str::<AgentTurn>(&json).unwrap(), run.turn);
    }
}

#[test]
fn cancellation_before_and_after_blocking_model() {
    let mut session = AgentSession::builder().build().unwrap();
    for before in [true, false] {
        let token = AgentCancellationToken::default();
        if before {
            token.cancel();
        }
        let mut model = model();
        model.cancel = Some(token.clone());
        let mut last = None;
        let run = run_session_observed(
            &mut model,
            &mut session,
            "request",
            AgentLimits::default(),
            |_, _| true,
            None,
            &token,
            &mut |turn| last = Some(turn.clone()),
        )
        .unwrap();
        assert_eq!(model.calls, usize::from(!before));
        assert_eq!(run.stop, AgentStop::Cancelled);
        assert_eq!(run.turn.state(), &AgentState::Cancelled);
        assert_eq!(last, Some(run.turn));
    }
}

#[test]
fn native_evidence_is_retained_but_output_is_not_serialized() {
    let mut turn = AgentTurn::new(None, None, AgentLimits::default());
    turn.transition(AgentState::Deciding { request_ordinal: 1 })
        .unwrap();
    turn.record(AgentEvidence::Decision { ordinal: 1 }).unwrap();
    turn.record(AgentEvidence::ModelResponse {
        ordinal: 1,
        outcome: AgentModelResponseOutcome::Returned,
        response_bytes: Some(42),
    })
    .unwrap();
    turn.transition(AgentState::ToolRunning {
        request_ordinal: 1,
        call_ordinal: 1,
        name: "read".into(),
    })
    .unwrap();
    let output = ToolValue::Integer(i64::MAX);
    turn.record(AgentEvidence::Tool {
        ordinal: 1,
        name: "read".into(),
        request_ordinal: 1,
        outcome: AgentToolOutcome::Success,
        output: Some(output.clone()),
    })
    .unwrap();
    assert!(
        matches!(&turn.evidence()[2], AgentEvidence::Tool { output: Some(value), .. } if value == &output)
    );
    let json = serde_json::to_string(&turn).unwrap();
    assert!(!json.contains("output"));
    assert!(!json.contains(&i64::MAX.to_string()));
    let mut json: serde_json::Value = serde_json::from_str(&json).unwrap();
    json["evidence"][2]["Tool"]["output"] = serde_json::json!({"String":"forged secret output"});
    let loaded: AgentTurn = serde_json::from_value(json).unwrap();
    assert!(matches!(
        &loaded.evidence()[2],
        AgentEvidence::Tool { output: None, .. }
    ));
}

#[test]
fn retained_import_rejects_inconsistent_history_and_usage() {
    let mut session = AgentSession::builder().build().unwrap();
    let run = run_session_observed(
        &mut model(),
        &mut session,
        "request",
        AgentLimits::default(),
        |_, _| true,
        None,
        &AgentCancellationToken::default(),
        &mut |_| {},
    )
    .unwrap();
    let original = serde_json::to_value(&run.turn).unwrap();
    let mutations = [
        ("/id", serde_json::json!(0)),
        ("/transitions/0/turn_id", serde_json::json!(0)),
        ("/transitions/0/ordinal", serde_json::json!(7)),
        ("/transitions/0/request_ordinal", serde_json::json!(2)),
        ("/transitions/0/trigger", serde_json::json!("ToolRequested")),
        ("/transitions/1/from", serde_json::json!("Ready")),
        ("/state", serde_json::json!("Ready")),
        ("/evidence/1/ModelResponse/ordinal", serde_json::json!(2)),
        (
            "/evidence/1/ModelResponse/response_bytes",
            serde_json::Value::Null,
        ),
        (
            "/evidence/1/ModelResponse/outcome",
            serde_json::json!({"Error":"Configuration"}),
        ),
        ("/usage/decisions", serde_json::json!(0)),
        ("/usage/conversation_bytes", serde_json::json!(usize::MAX)),
    ];
    for (path, value) in mutations {
        let mut json = original.clone();
        *json.pointer_mut(path).unwrap() = value;
        assert!(
            serde_json::from_value::<AgentTurn>(json).is_err(),
            "accepted {path}"
        );
    }
    let mut duplicate_response = original.clone();
    let response = duplicate_response["evidence"][1].clone();
    duplicate_response["evidence"]
        .as_array_mut()
        .unwrap()
        .push(response);
    assert!(serde_json::from_value::<AgentTurn>(duplicate_response).is_err());
    let mut missing_response = original.clone();
    missing_response["evidence"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    assert!(serde_json::from_value::<AgentTurn>(missing_response).is_err());
    let mut without_provider = original.clone();
    without_provider
        .as_object_mut()
        .unwrap()
        .remove("provider_identity");
    assert_eq!(
        serde_json::from_value::<AgentTurn>(without_provider)
            .unwrap()
            .provider_identity(),
        None
    );
    let mut duplicate = original;
    let event = duplicate["transitions"][0].clone();
    duplicate["transitions"]
        .as_array_mut()
        .unwrap()
        .insert(1, event);
    assert!(serde_json::from_value::<AgentTurn>(duplicate).is_err());
}

#[test]
fn combined_trace_observer_retains_source_and_terminal_failures() {
    use crate::agent::{ProtocolTrace, run_session_traced_observed};
    let mut session = AgentSession::builder().build().unwrap();
    for fail in [false, true] {
        let mut model = model();
        if fail {
            model.reply = Err("backend failure".into());
        }
        let mut bytes = vec![];
        let mut last = None;
        let source = AgentSourceBinding {
            identity: AgentSourceIdentity::ScratchBuffer("buffer-1".into()),
            revision: 42,
            language: "scope".into(),
        };
        let result = run_session_traced_observed(
            &mut model,
            &mut session,
            "private request",
            AgentLimits::default(),
            |_, _| true,
            &mut ProtocolTrace::new(&mut bytes),
            Some(source.clone()),
            &AgentCancellationToken::default(),
            &mut |turn| last = Some(turn.clone()),
        );
        assert_eq!(result.is_err(), fail);
        let last = last.unwrap();
        assert_eq!(last.source_binding(), Some(&source));
        assert_eq!(
            last.state(),
            if fail {
                &AgentState::Failed(AgentFailure::Model)
            } else {
                &AgentState::Completed
            }
        );
        let trace = String::from_utf8(bytes).unwrap();
        assert!(trace.contains("private request"));
        assert!(trace.contains("outcome"));
        assert!(
            !serde_json::to_string(&last)
                .unwrap()
                .contains("private request")
        );
    }
}

#[test]
fn workflow_calls_keep_distinct_attribution_and_shared_usage() {
    use crate::agent::{AgentWorkflow, AgentWorkflowContext, run_session_with_workflows};
    use crate::tool::{ToolCall, ToolDefinition, ToolProvider, ToolResult};
    struct Provider(Vec<ToolDefinition>);
    impl ToolProvider for Provider {
        fn tools(&self) -> &[ToolDefinition] {
            &self.0
        }
        fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new(ToolValue::Integer(i64::MAX)))
        }
    }
    struct Workflow;
    impl AgentWorkflow for Workflow {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("compound", "compound read")
        }
        fn execute(
            &mut self,
            _: ToolCall,
            context: &mut AgentWorkflowContext<'_, '_>,
        ) -> Result<ToolResult, ToolError> {
            context.invoke(ToolCall::new("read"))
        }
    }
    let mut session = AgentSession::builder()
        .provider(Provider(vec![ToolDefinition::new("read", "read")]))
        .build()
        .unwrap();
    let mut model = model();
    model.reply = Ok(r#"{"type":"tool","name":"compound","arguments":{}}"#.into());
    let run = run_session_with_workflows(
        &mut model,
        &mut session,
        "request",
        AgentLimits {
            max_decisions: 1,
            ..AgentLimits::default()
        },
        |_, _| true,
        &mut [&mut Workflow],
    )
    .unwrap();
    assert_eq!(run.tool_calls, 2);
    assert_eq!(run.turn.usage().tool_calls, 2);
    let calls = run
        .turn
        .evidence()
        .iter()
        .filter_map(|e| match e {
            AgentEvidence::Tool {
                ordinal,
                request_ordinal,
                name,
                outcome,
                ..
            } => Some((*ordinal, *request_ordinal, name.as_str(), *outcome)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        calls,
        [
            (2, 1, "read", AgentToolOutcome::Success),
            (1, 1, "compound", AgentToolOutcome::Success)
        ]
    );
    assert!(
        run.turn
            .transitions()
            .iter()
            .any(|t| t.trigger == AgentTrigger::ToolRequested
                && t.call_ordinal == Some(2)
                && t.request_ordinal == Some(1))
    );
    let loaded: AgentTurn =
        serde_json::from_value(serde_json::to_value(&run.turn).unwrap()).unwrap();
    assert_eq!(loaded.usage(), run.turn.usage());
    assert!(loaded.usage().conversation_bytes > 0);
}

#[test]
fn tool_error_categories_are_typed_without_error_text() {
    let errors = [
        (
            ToolError::UnknownTool {
                name: "read".into(),
            },
            AgentToolOutcome::UnknownTool,
        ),
        (
            ToolError::DuplicateTool {
                name: "read".into(),
            },
            AgentToolOutcome::DuplicateTool,
        ),
        (
            ToolError::MissingArgument {
                tool: "read".into(),
                argument: "secret".into(),
            },
            AgentToolOutcome::MissingArgument,
        ),
        (
            ToolError::InvalidArgument {
                tool: "read".into(),
                argument: "secret".into(),
                message: "secret".into(),
            },
            AgentToolOutcome::InvalidArgument,
        ),
        (
            ToolError::Denied {
                tool: "read".into(),
            },
            AgentToolOutcome::Denied,
        ),
        (
            ToolError::Conflict {
                tool: "read".into(),
                path: "secret".into(),
                current_revision: None,
                message: "secret".into(),
            },
            AgentToolOutcome::Conflict,
        ),
        (
            ToolError::Execution {
                tool: "read".into(),
                message: "secret".into(),
            },
            AgentToolOutcome::Execution,
        ),
    ];
    for (error, expected) in errors {
        let outcome = AgentToolOutcome::from_result(&Err(error));
        assert_eq!(outcome, expected);
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(!json.contains("secret"));
        assert_eq!(
            serde_json::from_str::<AgentToolOutcome>(&json).unwrap(),
            outcome
        );
    }
}

#[test]
fn live_callback_can_cancel_before_model_dispatch() {
    let token = AgentCancellationToken::default();
    let mut model = model();
    let mut session = AgentSession::builder().build().unwrap();
    let run = run_session_observed(
        &mut model,
        &mut session,
        "request",
        AgentLimits::default(),
        |_, _| true,
        None,
        &token,
        &mut |turn| {
            if matches!(turn.state(), AgentState::Deciding { .. }) {
                token.cancel();
            }
        },
    )
    .unwrap();
    assert_eq!(model.calls, 0);
    assert_eq!(run.turn.state(), &AgentState::Cancelled);
}

#[test]
fn cancellation_around_tool_dispatch_retains_native_result() {
    use crate::tool::{ToolCall, ToolDefinition, ToolError, ToolProvider, ToolResult};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Provider {
        tools: Vec<ToolDefinition>,
        token: AgentCancellationToken,
        calls: Arc<AtomicUsize>,
    }
    impl ToolProvider for Provider {
        fn tools(&self) -> &[ToolDefinition] {
            &self.tools
        }
        fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.token.cancel();
            Ok(ToolResult {
                value: ToolValue::Integer(i64::MAX),
            })
        }
    }
    for cancel_before in [true, false] {
        let token = AgentCancellationToken::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Provider {
            tools: vec![ToolDefinition::new("read", "read")],
            token: token.clone(),
            calls: calls.clone(),
        };
        let mut session = AgentSession::builder().provider(provider).build().unwrap();
        let mut model = model();
        model.reply = Ok(r#"{"type":"tool","name":"read","arguments":{}}"#.into());
        let run = run_session_observed(
            &mut model,
            &mut session,
            "request",
            AgentLimits::default(),
            |_, _| true,
            None,
            &token,
            &mut |turn| {
                if cancel_before && matches!(turn.state(), AgentState::ToolRunning { .. }) {
                    token.cancel();
                }
            },
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), usize::from(!cancel_before));
        assert_eq!(run.turn.state(), &AgentState::Cancelled);
        if !cancel_before {
            assert!(run.turn.evidence().iter().any(|e| matches!(
                e,
                AgentEvidence::Tool {
                    output: Some(ToolValue::Integer(i64::MAX)),
                    ..
                }
            )));
        }
    }
}
