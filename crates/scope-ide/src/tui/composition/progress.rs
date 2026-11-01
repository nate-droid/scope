//! Presentation of authoritative lifecycle snapshots, never model prose or tool output.
use crate::agent::{AgentEvidence, AgentSourceIdentity, AgentState, AgentTurn};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::{Block, Borders, Clear, Paragraph},
};

fn escaped(value: &str) -> String {
    // Bound host/model-controlled identities and escape terminal controls.
    let mut bounded = String::new();
    for ch in value.chars() {
        let encoded = serde_json::to_string(&ch.to_string()).expect("string serialization");
        let escaped = &encoded[1..encoded.len() - 1];
        if bounded.len() + escaped.len() > 24 {
            bounded.push('…');
            break;
        }
        bounded.push_str(escaped);
    }
    format!("\"{bounded}\"")
}

fn text(turn: &AgentTurn) -> String {
    let usage = turn.usage();
    let state = match turn.state() {
        AgentState::Ready => "Ready".into(),
        AgentState::Deciding { request_ordinal } => format!("Model deciding #{request_ordinal}"),
        AgentState::ToolRunning { name, .. } => format!("Tool running {}", escaped(name)),
        AgentState::Completed => "Completed".into(),
        AgentState::BudgetExhausted(stop) => format!("Budget exhausted: {stop:?}"),
        AgentState::Cancelled => "Cancelled".into(),
        AgentState::Failed(failure) => format!("Failed: {failure:?}"),
    };
    let recent = turn
        .evidence()
        .iter()
        .rev()
        .find_map(|evidence| match evidence {
            AgentEvidence::Tool { name, outcome, .. } => Some(format!(
                "{outcome:?}: {} (provider only; not acceptance)",
                escaped(name)
            )),
            _ => None,
        })
        .unwrap_or_else(|| "none recorded; submission is not success".into());
    let recent = match turn.state() {
        AgentState::ToolRunning {
            call_ordinal, name, ..
        } if !turn.evidence().iter().any(|e| {
            matches!(e,
                AgentEvidence::Tool { ordinal, .. } if ordinal == call_ordinal)
        }) =>
        {
            format!("Submitted {} (not success)", escaped(name))
        }
        _ => recent,
    };
    let source = turn
        .source_binding()
        .map(|source| {
            let (kind, identity) = match &source.identity {
                AgentSourceIdentity::ScratchBuffer(value) => ("scratch", value),
                AgentSourceIdentity::File(value) => ("file", value),
                AgentSourceIdentity::Host(value) => ("host", value),
            };
            format!("{kind} {} rev {}", escaped(identity), source.revision)
        })
        .unwrap_or_else(|| "unbound".into());
    format!(
        "{} • turn {}\nRecent: {}\nModel {} / {}\nSource {}\nDecisions {} • calls {} • malformed {} • bytes {}\nBoundary updates only; synchronous calls block input/cancellation.",
        state,
        turn.id(),
        recent,
        escaped(turn.model_identity().unwrap_or("unknown")),
        escaped(turn.provider_identity().unwrap_or("unknown provider")),
        source,
        usage.decisions,
        usage.tool_calls,
        usage.malformed,
        usage.conversation_bytes
    )
}

pub(super) fn render(frame: &mut Frame<'_>, turn: &AgentTurn) {
    let areas = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length((frame.area().height / 3).clamp(1, 8)),
        Constraint::Length(1),
    ])
    .split(frame.area());
    frame.render_widget(Clear, areas[1]);
    frame.render_widget(
        Paragraph::new(text(turn)).block(Block::default().borders(Borders::ALL).title("output")),
        areas[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentLimits;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn observed_progress_shows_submission_outcome_and_completion() {
        use crate::agent::{
            AgentCancellationToken, AgentSession, AgentSourceBinding, DecisionMessage,
            DecisionModel, run_session_observed,
        };
        use crate::tool::{
            ToolCall, ToolDefinition, ToolError, ToolProvider, ToolResult, ToolValue,
        };

        struct Model(usize);
        impl DecisionModel for Model {
            fn model_identity(&self) -> Option<String> {
                Some("model\u{1b}[2J\nspoof".into())
            }
            fn decide(&mut self, _: &[DecisionMessage], _: u32) -> Result<String, String> {
                self.0 += 1;
                Ok(if self.0 == 1 {
                    r#"{"type":"tool","name":"inspect_workspace","arguments":{}}"#
                } else {
                    r#"{"type":"complete","content":"SECRET_MODEL_OUTPUT"}"#
                }
                .into())
            }
        }
        struct Provider(Vec<ToolDefinition>);
        impl ToolProvider for Provider {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::new(ToolValue::String(
                    "SECRET_TOOL_OUTPUT".into(),
                )))
            }
        }
        let mut session = AgentSession::builder()
            .provider(Provider(vec![ToolDefinition::new(
                "inspect_workspace",
                "inspect",
            )]))
            .build()
            .unwrap();
        let mut snapshots = Vec::new();
        run_session_observed(
            &mut Model(0),
            &mut session,
            "SECRET_REQUEST",
            AgentLimits::default(),
            |_, _| true,
            Some(AgentSourceBinding {
                identity: AgentSourceIdentity::Host(format!("source\u{1b}\n{}", "x".repeat(200))),
                revision: 7,
                language: "scope".into(),
            }),
            &AgentCancellationToken::default(),
            &mut |turn| snapshots.push(turn.clone()),
        )
        .unwrap();
        assert!(
            snapshots
                .iter()
                .any(|turn| matches!(turn.state(), AgentState::Deciding { .. }))
        );
        assert!(
            snapshots
                .iter()
                .any(|turn| matches!(turn.state(), AgentState::ToolRunning { .. }))
        );
        assert_eq!(snapshots.last().unwrap().state(), &AgentState::Completed);
        assert!(
            snapshots
                .last()
                .unwrap()
                .evidence()
                .iter()
                .any(|e| matches!(e,
            AgentEvidence::Tool { output: Some(ToolValue::String(value)), .. }
                if value == "SECRET_TOOL_OUTPUT"))
        );
        for (width, height) in [(80, 24), (120, 30)] {
            for turn in &snapshots {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|frame| render(frame, turn)).unwrap();
                let screen = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                assert!(!screen.contains("SECRET_"));
                assert!(!screen.contains('\u{1b}'));
                assert!(screen.contains("Model \"model\\u001b"));
                assert!(screen.contains("Source host \"source\\u001b\\n"));
                assert!(screen.contains("Decisions "));
                if matches!(turn.state(), AgentState::ToolRunning { .. }) {
                    assert!(screen.contains("Tool running \"inspect_workspace\""));
                    if turn
                        .evidence()
                        .iter()
                        .any(|e| matches!(e, AgentEvidence::Tool { .. }))
                    {
                        assert!(screen.contains("Recent: Success:"));
                    } else {
                        assert!(screen.contains("Submitted \"inspect_workspace\" (not success)"));
                        assert!(!screen.contains("Success"));
                    }
                }
                if matches!(turn.state(), AgentState::Completed) {
                    assert!(screen.contains("Completed"));
                    assert!(screen.contains("Recent: Success:"));
                    assert!(screen.contains("not acceptance"));
                }
            }
        }
    }

    #[test]
    fn current_state_and_recent_operation_fit_workbench_sizes_without_secrets() {
        let turn = AgentTurn::new(
            Some("model\u{1b}[2J\nspoof".into()),
            None,
            AgentLimits::default(),
        );
        for (width, height) in [(80, 24), (120, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(frame, &turn)).unwrap();
            let buffer = terminal.backend().buffer();
            let screen = buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(screen.contains("Ready"));
            assert!(screen.contains("Recent: none recorded; submission is not success"));
            assert!(!screen.contains('\u{1b}'));
            assert!(screen.contains("\\u001b"));
            assert!(!screen.contains("Completed"));
        }
    }
}
