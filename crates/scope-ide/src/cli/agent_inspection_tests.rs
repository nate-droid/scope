#![cfg(feature = "agent")]

use super::*;
use crate::agent::{DecisionMessage, DecisionModel};
use crate::test_support::TempWorkspace;
use scope_language::{
    EvaluationResult, EvaluationStatus, InspectionError, InteractiveLanguageSession,
    InteractiveSessionProvider, SessionError, SymbolDescription, SymbolKind, SymbolOrigin,
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};

const INSPECT: &str = "scope.interactive.inspect";

struct Model {
    replies: VecDeque<String>,
    conversations: Vec<Vec<DecisionMessage>>,
}

impl Model {
    fn new(calls: impl IntoIterator<Item = Value>) -> Self {
        Self {
            replies: calls
                .into_iter()
                .chain([json!({"type":"complete", "content":"finished"})])
                .map(|value| value.to_string())
                .collect(),
            conversations: Vec::new(),
        }
    }

    fn feedback(&self) -> Vec<Value> {
        self.conversations
            .last()
            .unwrap()
            .iter()
            .filter_map(|message| {
                let value: Value = serde_json::from_str(&message.content).ok()?;
                (value["kind"] == "tool_result").then_some(value)
            })
            .collect()
    }

    fn initial(&self) -> Value {
        serde_json::from_str(&self.conversations[0][1].content).unwrap()
    }
}

impl DecisionModel for Model {
    fn decide(&mut self, messages: &[DecisionMessage], max_tokens: u32) -> Result<String, String> {
        assert!(max_tokens > 0);
        self.conversations.push(messages.to_vec());
        Ok(self.replies.pop_front().expect("unexpected model decision"))
    }
}

fn tool(name: &str, arguments: Value) -> Value {
    json!({"type":"tool", "name":name, "arguments":arguments})
}

fn inspect(name: &str) -> Value {
    tool(INSPECT, json!({"name":name}))
}

struct Provider(Mutex<Vec<String>>);
struct Session {
    provider: &'static Provider,
    source: String,
}

// Each test owns independent event state, as in the interactive CLI fixtures.
impl InteractiveSessionProvider for &'static Provider {
    fn language_id(&self) -> &'static str {
        "cli.inspection"
    }
    fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
        self.0.lock().unwrap().push("create".into());
        Ok(Box::new(Session {
            provider: self,
            source: String::new(),
        }))
    }
}

impl InteractiveLanguageSession for Session {
    fn language_id(&self) -> &str {
        "cli.inspection"
    }
    fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
        self.provider
            .0
            .lock()
            .unwrap()
            .push(format!("evaluate:{source}"));
        self.source = source.into();
        Ok(EvaluationResult {
            output: vec![],
            diagnostics: vec![],
            status: EvaluationStatus::Success,
        })
    }
    fn reset(&mut self) -> Result<(), SessionError> {
        panic!("inspection must not reset")
    }
    fn close(&mut self) -> Result<(), SessionError> {
        self.provider.0.lock().unwrap().push("close".into());
        Ok(())
    }
    fn inspect(&self, name: &str) -> Result<Option<SymbolDescription>, InspectionError> {
        match name {
            "unknown" => Ok(None),
            "unsupported" => Err(InspectionError::Unsupported),
            "authorization" => Err(InspectionError::Authorization {
                code: "host_denied".into(),
                message: "host approval required".into(),
            }),
            "infrastructure" => Err(InspectionError::Infrastructure {
                code: "offline".into(),
                message: "provider unavailable".into(),
            }),
            _ => Ok(Some(SymbolDescription {
                name: name.into(),
                kind: SymbolKind::Variable,
                signature: None,
                documentation: Some(self.source.clone()),
                origin: Some(SymbolOrigin::Interactive),
                definition: None,
                location: None,
            })),
        }
    }
}

fn fixture() -> (&'static Provider, LanguageRegistry, InteractiveSession) {
    let provider: &'static Provider = Box::leak(Box::new(Provider(Mutex::new(Vec::new()))));
    let registration = Box::leak(Box::new(provider));
    let registry = LanguageRegistry::empty().with_interactive_provider(registration);
    (provider, registry, InteractiveSession::new())
}

fn run(app: &mut App, session: Option<&InteractiveSession>, calls: Vec<Value>) -> Model {
    let mut model = Model::new(calls);
    let response =
        run_cli_agent_model(&mut model, app, session, "inspect without evaluation").unwrap();
    assert_eq!(response.content, "finished");
    assert!(model.replies.is_empty());
    model
}

fn authority(app: &App) -> Vec<u8> {
    let mut output = Vec::new();
    print_capabilities(app.workspace(), &mut output).unwrap();
    print_authority(app.workspace(), &mut output).unwrap();
    output
}

#[test]
fn known_unsaved_session_metadata_matches_human_inspection_without_side_effects() {
    let root = TempWorkspace::new("cli_agent_inspection_known");
    root.write_file("sample.txt", "disk text\n");
    let mut app = App::new(root.workspace()).unwrap();
    app.open_path("sample.txt").unwrap();
    let (provider, registry, mut session) = fixture();
    session.select(&registry, "cli.inspection").unwrap();
    session.evaluate("unsaved session definition").unwrap();
    let events = provider.0.lock().unwrap().clone();
    let context = crate::agent::AgentContext::from_app(&app).render();
    let grants = authority(&app);
    let name = " exact name \"é\" ";
    let model = run(&mut app, Some(&session), vec![inspect(name), inspect(name)]);
    let feedback = model.feedback();
    assert_eq!(feedback.len(), 2);
    for result in &feedback {
        assert_eq!(result["ok"], true);
        assert_eq!(
            result["value"],
            json!({"status":"known", "description":{
                "name":name, "kind":"variable", "signature":null,
                "documentation":"unsaved session definition", "origin":"interactive",
                "definition":null, "location":null,
            }})
        );
    }
    let mut human = Vec::new();
    print_inspection("describe", name, session.inspect(name), &mut human).unwrap();
    let human = String::from_utf8(human).unwrap();
    assert!(human.contains(&format!("symbol: {name}")));
    assert!(human.contains("documentation: unsaved session definition"));
    assert!(human.contains("signature: [unavailable]"));
    assert!(human.contains("origin: interactive"));
    assert_eq!(provider.0.lock().unwrap().as_slice(), events.as_slice());
    assert_eq!(crate::agent::AgentContext::from_app(&app).render(), context);
    assert_eq!(authority(&app), grants);
    assert_eq!(root.read_file("sample.txt"), "disk text\n");
    assert!(
        model.initial()["request"]
            .as_str()
            .unwrap()
            .contains(&context)
    );
}

#[test]
fn inspection_outcomes_are_structured_feedback_not_agent_failures_or_grants() {
    let root = TempWorkspace::new("cli_agent_inspection_outcomes");
    let mut app = App::new(root.workspace()).unwrap();
    let (provider, registry, mut session) = fixture();
    session.select(&registry, "cli.inspection").unwrap();
    let before = authority(&app);
    let names = ["unknown", "unsupported", "authorization", "infrastructure"];
    let model = run(
        &mut app,
        Some(&session),
        names.iter().map(|name| inspect(name)).collect(),
    );
    for (name, result) in names.iter().zip(model.feedback()) {
        assert_eq!(result["ok"], true);
        assert_eq!(result["value"]["status"], *name);
        let mut human = Vec::new();
        print_inspection("describe", name, session.inspect(name), &mut human).unwrap();
        let human = String::from_utf8(human).unwrap();
        if *name == "unknown" {
            assert_eq!(human, "unknown symbol: unknown\n");
        } else {
            assert!(human.starts_with("error: "));
        }
        if let Some(message) = result["value"]["message"].as_str() {
            assert!(human.contains(message));
        }
    }
    let results = model.feedback();
    assert_eq!(results[2]["value"]["code"], "host_denied");
    assert_eq!(results[3]["value"]["code"], "offline");
    assert_eq!(authority(&app), before);
    assert_eq!(*provider.0.lock().unwrap(), vec!["create"]);
}

#[test]
fn session_replacement_and_closure_are_observed_by_each_cli_request() {
    let root = TempWorkspace::new("cli_agent_inspection_lifecycle");
    let mut app = App::new(root.workspace()).unwrap();
    let (provider, registry, mut session) = fixture();
    let model = run(&mut app, Some(&session), vec![inspect("x")]);
    assert_eq!(model.feedback()[0]["value"]["status"], "no_session");
    session.select(&registry, "cli.inspection").unwrap();
    session.evaluate("old unsaved value").unwrap();
    assert_eq!(
        run(&mut app, Some(&session), vec![inspect("x")]).feedback()[0]["value"]["description"]["documentation"],
        "old unsaved value"
    );
    session.select(&registry, "cli.inspection").unwrap();
    assert_eq!(
        run(&mut app, Some(&session), vec![inspect("x")]).feedback()[0]["value"]["description"]["documentation"],
        ""
    );
    session.clear().unwrap();
    assert_eq!(
        run(&mut app, Some(&session), vec![inspect("x")]).feedback()[0]["value"]["status"],
        "no_session"
    );
    assert_eq!(
        *provider.0.lock().unwrap(),
        vec![
            "create",
            "evaluate:old unsaved value",
            "create",
            "close",
            "close"
        ]
    );
}

#[test]
fn discovery_is_opt_in_and_existing_tools_remain_read_only() {
    let root = TempWorkspace::new("cli_agent_inspection_catalog");
    root.write_file("sample.txt", "unchanged");
    let mut app = App::new(root.workspace()).unwrap();
    let (_, _, session) = fixture();
    let before = authority(&app);
    for interactive in [Some(&session), None] {
        let model = run(
            &mut app,
            interactive,
            vec![
                tool("scope.document.read", json!({"path":"sample.txt"})),
                tool(
                    "scope.document.replace",
                    json!({
                        "path":"sample.txt", "expected_revision":"unused", "start_line":0,
                        "start_column":0, "end_line":0, "end_column":0, "replacement":"MUTATED",
                    }),
                ),
                inspect("x"),
            ],
        );
        let initial = model.initial();
        let tools = initial["catalog"]["tools"].as_array().unwrap();
        let inspection: Vec<_> = tools
            .iter()
            .filter(|entry| entry["definition"]["name"] == INSPECT)
            .collect();
        assert_eq!(inspection.len(), usize::from(interactive.is_some()));
        assert!(
            tools
                .iter()
                .any(|entry| entry["definition"]["name"] == "scope.document.read")
        );
        if interactive.is_some() {
            let catalog = inspection[0].to_string();
            assert!(catalog.contains("scope.interactive"));
            assert!(catalog.contains("no_session"));
            assert!(catalog.contains("name"));
        }
        let results = model.feedback();
        assert_eq!(results[0]["ok"], true);
        assert_eq!(results[0]["value"]["text"], "unchanged");
        assert_eq!(results[1]["ok"], false);
        assert!(
            results[1]["error"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("denied")
        );
        if interactive.is_some() {
            assert_eq!(results[2]["value"]["status"], "no_session");
        } else {
            assert_eq!(results[2]["ok"], false);
            assert!(results[2]["error"].as_str().unwrap().contains(INSPECT));
        }
        assert_eq!(root.read_file("sample.txt"), "unchanged");
        assert_eq!(authority(&app), before);
    }
}
