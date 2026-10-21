//! Deterministic integration tests through the real terminal decision loop.
use super::*;
use crate::{agent::DecisionMessage, language::LanguageRegistry, test_support::TempWorkspace};
use scope_language::*;
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};

fn run_projected(
    target: &impl SourceTarget,
    service: &Service,
    model: &mut impl DecisionModel,
    authorize: &mut dyn for<'b> FnMut(SemanticAuthorization<'b>) -> bool,
) {
    let snapshot = target.snapshot();
    let operations = service.semantic_operations();
    let mut controller = SourceEditController::default();
    let provider = SemanticToolProvider::new(
        service,
        target,
        &mut controller,
        PermittedEditScope::Document(snapshot.id),
        None,
        |a: SemanticAuthorization<'_>| {
            a.source.id == snapshot.id
                && a.source.version == snapshot.version
                && a.source.text == snapshot.text
                && a.source.language == snapshot.language
                && operations
                    .iter()
                    .any(|op| op == a.operation && eligible(op, &ids()))
                && authorize(a)
        },
    )
    .unwrap();
    let mut session = AgentSession::builder()
        .provider(LanguageArguments::new(provider, snapshot))
        .build()
        .unwrap();
    let result = crate::agent::run_session(
        model,
        &mut session,
        "Inspect source",
        AgentLimits {
            max_tool_calls: 24,
            max_decisions: 26,
            ..AgentLimits::default()
        },
        crate::agent::read_only_policy,
    )
    .unwrap();
    assert_eq!(result.stop, crate::agent::AgentStop::Completed);
}

#[test]
fn projected_language_arguments_preserve_host_authority_and_source() {
    let (root, mut app, service) = fixture(Service::default());
    app.open_document_mut()
        .unwrap()
        .apply_edit(crate::text::TextEdit::insert(
            crate::text::Position::new(0, 15),
            "\n",
        ))
        .unwrap();
    let before = app.open_document().unwrap().clone();
    assert_eq!(before.text(), "disk unsaved é\n");
    // Capture owned baseline values before the public entry point borrows App.
    let (source_id, revision, language) = {
        let target = DocumentSource::new(app.open_document_mut().unwrap(), service.language_id());
        let snapshot = target.snapshot();
        (snapshot.id, snapshot.version, snapshot.language.to_owned())
    };
    let candidate = "candidate λ\n";
    let mut calls = vec![
        call("inspect", json!({})),
        call("candidate", json!({"candidate":candidate})),
        call("candidate", json!({"candidate":"denied"})),
        call("candidate", json!({"candidate":42})),
        call("candidate", json!({})),
        call("inspect", json!({"extra":true})),
    ];
    for field in [
        "expected_version",
        "expected_source",
        "arguments",
        "target_id",
    ] {
        let mut args = json!({"candidate":candidate});
        args[field] = json!("forged");
        calls.push(call("candidate", args));
    }
    calls.push(call("edit", json!({})));
    let mut model = Model::new(calls);
    let mut grants = vec![];
    let allowed = ids();
    let mut authorize = |a: SemanticAuthorization<'_>| {
        assert_eq!(a.source.id, source_id);
        assert_eq!(a.source.version, revision);
        assert_eq!(a.source.text.as_bytes(), b"disk unsaved \xc3\xa9\n");
        assert_eq!(a.source.language, language);
        assert_eq!(
            a.operation,
            &service
                .semantic_operations()
                .into_iter()
                .find(|op| op.id == a.operation.id)
                .unwrap()
        );
        grants.push((a.operation.id.clone(), a.arguments.clone()));
        a.arguments.get("candidate") != Some(&json!("denied"))
    };
    run_terminal_agent_model_with_projected_arguments(
        &mut app,
        None,
        &mut model,
        "Inspect without modifying source",
        AgentLimits {
            max_tool_calls: 24,
            max_decisions: 26,
            ..AgentLimits::default()
        },
        TerminalSemanticConfig {
            allowed_operation_ids: &allowed,
            authorize: &mut authorize,
        },
    )
    .unwrap();
    let initial = model.initial();
    let tools = initial["catalog"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    for (tool, required) in tools.iter().zip([json!([]), json!(["candidate"])]) {
        let schema = &tool["metadata"]["invocation_schema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], required);
        assert!(schema["properties"].get("expected_source").is_none());
        assert!(schema["properties"].get("arguments").is_none());
        assert_eq!(tool["metadata"]["effect"], "Read");
        assert_eq!(
            tool["metadata"]["host_binding"],
            json!({
                "owner": "app-document",
                "source_id": format!("{source_id:?}"),
                "original_revision": revision.to_string(),
                "language": language,
                "role": "invocation_baseline"
            })
        );
    }
    assert_eq!(tools[0]["definition"]["inputs"], json!([]));
    assert_eq!(tools[1]["definition"]["inputs"][0]["name"], "candidate");
    assert_eq!(
        tools[1]["metadata"]["invocation_schema"]["properties"]["candidate"]["type"],
        "string"
    );
    assert_eq!(tools[1]["metadata"]["purpose"], "CandidateEvaluation");
    let feedback = model.feedback();
    assert_eq!(feedback[0]["value"]["value"]["source"], before.text());
    assert_eq!(
        feedback[1]["value"]["value"],
        json!({
            "status": "rejected",
            "candidate": candidate,
            "diagnostics": [{"code": "domain_rule", "message": "candidate violates fixture rule"}]
        })
    );
    assert_eq!(feedback[2]["value"]["status"], "unauthorized");
    for (index, result) in feedback[3..10].iter().enumerate() {
        assert_eq!(result["ok"], true);
        assert_eq!(result["value"]["status"], "invalid_arguments");
        let tool = &tools[if index == 2 { 0 } else { 1 }];
        assert_eq!(
            result["value"]["invocation_schema"],
            tool["metadata"]["invocation_schema"]
        );
        let message = result["value"]["message"].as_str().unwrap();
        assert!(message.contains("invocation_schema"));
        assert!(!message.contains("Language fields belong inside arguments"));
        assert!(!message.contains("preserve host source preconditions"));
    }
    assert_eq!(feedback[10]["ok"], false);
    assert_eq!(
        grants,
        vec![
            ("inspect".into(), json!({})),
            ("candidate".into(), json!({"candidate":candidate})),
            ("candidate".into(), json!({"candidate":"denied"}))
        ]
    );
    assert_eq!(
        *service.events.lock().unwrap(),
        vec![
            json!({"id":"inspect","source":before.text(),"version":before.version(),"arguments":{}}),
            json!({"id":"candidate","source":before.text(),"version":before.version(),"arguments":{"candidate":candidate}})
        ]
    );
    assert_eq!(app.open_document().unwrap(), &before);
    assert_eq!(root.read_file("sample.fixture"), "disk");
}

#[test]
fn projected_language_arguments_stale_identity_revision_text_and_language_fail_closed() {
    use crate::{proposal::ProposalError, source_edit::SourceId};
    use std::cell::Cell;
    struct Target {
        id: Cell<SourceId>,
        version: Cell<u64>,
        text: Cell<&'static str>,
        language: Cell<&'static str>,
    }
    impl SourceTarget for Target {
        fn source_id(&self) -> SourceId {
            self.id.get()
        }
        fn version(&self) -> u64 {
            self.version.get()
        }
        fn text(&self) -> &str {
            self.text.get()
        }
        fn language_id(&self) -> &str {
            self.language.get()
        }
        fn apply_source_edit(&mut self, _: crate::text::TextEdit) -> Result<(), ProposalError> {
            panic!("projection must not edit")
        }
    }
    struct ChangingModel<'a> {
        model: Model,
        target: &'a Target,
        change: &'static str,
    }
    impl DecisionModel for ChangingModel<'_> {
        fn decide(&mut self, messages: &[DecisionMessage], tokens: u32) -> Result<String, String> {
            let reply = self.model.decide(messages, tokens);
            if self.model.conversations.len() == 1 {
                match self.change {
                    "identity" => self.target.id.set(SourceId::Scratch(99)),
                    "revision" => self.target.version.set(8),
                    "text" => self.target.text.set("changed λ\n"),
                    "language" => self.target.language.set("other-language"),
                    _ => unreachable!(),
                }
            }
            reply
        }
    }
    for change in ["identity", "revision", "text", "language"] {
        let (root, app, service) = fixture(Service::default());
        let before = app.open_document().unwrap().clone();
        let target = Target {
            id: Cell::new(SourceId::Scratch(42)),
            version: Cell::new(7),
            text: Cell::new("live é\n"),
            language: Cell::new("terminal-fixture"),
        };
        let mut model = ChangingModel {
            model: Model::new([call("inspect", json!({}))]),
            target: &target,
            change,
        };
        run_projected(&target, service, &mut model, &mut |_| {
            panic!("stale snapshot authorized")
        });
        assert_eq!(
            model.model.feedback()[0]["value"]["status"],
            if matches!(change, "identity" | "language") {
                "unauthorized"
            } else {
                "revision_conflict"
            }
        );
        assert!(service.events.lock().unwrap().is_empty());
        assert_eq!(
            target.text(),
            if change == "text" {
                "changed λ\n"
            } else {
                "live é\n"
            }
        );
        assert_eq!(app.open_document().unwrap(), &before);
        assert_eq!(root.read_file("sample.fixture"), "disk");
    }
}

#[test]
fn projected_language_arguments_recheck_source_after_execution() {
    use crate::{proposal::ProposalError, source_edit::SourceId};
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    struct Target(Arc<AtomicU64>);
    impl SourceTarget for Target {
        fn source_id(&self) -> SourceId {
            SourceId::Scratch(42)
        }
        fn version(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
        fn text(&self) -> &str {
            "live é\n"
        }
        fn language_id(&self) -> &str {
            "terminal-fixture"
        }
        fn apply_source_edit(&mut self, _: crate::text::TextEdit) -> Result<(), ProposalError> {
            panic!("projection must not edit")
        }
    }
    let version = Arc::new(AtomicU64::new(7));
    let changed = Arc::clone(&version);
    let (root, app, service) = fixture(Service {
        during_execution: Some(Box::new(move || {
            changed.store(8, Ordering::SeqCst);
        })),
        ..Service::default()
    });
    let before = app.open_document().unwrap().clone();
    let target = Target(version);
    let mut model = Model::new([call("inspect", json!({}))]);
    let mut grants = 0;
    run_projected(&target, service, &mut model, &mut |a| {
        assert_eq!(a.source.version, 7);
        grants += 1;
        true
    });
    let feedback = model.feedback();
    assert_eq!(feedback[0]["value"]["status"], "revision_conflict");
    assert!(feedback[0]["value"].get("value").is_none());
    assert_eq!(grants, 1);
    assert_eq!(service.events.lock().unwrap().len(), 1);
    assert_eq!(target.version(), 8);
    assert_eq!(app.open_document().unwrap(), &before);
    assert_eq!(root.read_file("sample.fixture"), "disk");
}

struct Model {
    replies: VecDeque<Result<String, String>>,
    conversations: Vec<Vec<DecisionMessage>>,
}
impl Model {
    fn new(calls: impl IntoIterator<Item = Value>) -> Self {
        Self {
            replies: calls
                .into_iter()
                .chain([json!({"type":"complete","content":"finished"})])
                .map(|v| Ok(v.to_string()))
                .collect(),
            conversations: vec![],
        }
    }
    fn initial(&self) -> Value {
        serde_json::from_str(&self.conversations[0][1].content).unwrap()
    }
    fn feedback(&self) -> Vec<Value> {
        self.conversations
            .last()
            .unwrap()
            .iter()
            .filter_map(|m| {
                let v: Value = serde_json::from_str(&m.content).ok()?;
                (v["kind"] == "tool_result").then_some(v)
            })
            .collect()
    }
}
impl DecisionModel for Model {
    fn decide(&mut self, messages: &[DecisionMessage], tokens: u32) -> Result<String, String> {
        assert!(tokens > 0);
        self.conversations.push(messages.to_vec());
        self.replies.pop_front().expect("unexpected model decision")
    }
}

#[derive(Default)]
struct Service {
    events: Mutex<Vec<Value>>,
    invalid: Option<&'static str>,
    empty: bool,
    during_execution: Option<Box<dyn Fn() + Send + Sync>>,
}
impl LanguageService for Service {
    fn language_id(&self) -> &'static str {
        "terminal-fixture"
    }
    fn language_name(&self) -> &'static str {
        "Terminal fixture"
    }
    fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
        true
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        Default::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        if self.empty {
            return vec![];
        }
        let inspect = SemanticOperationDescription {
            id: self.invalid.unwrap_or("inspect").into(),
            description: "Inspect live source".into(),
            arguments: vec![],
            effect: SemanticOperationEffect::Read,
            target: SemanticTargetRequirement::Document,
            constraints: None,
        };
        let mut candidate = inspect.clone();
        candidate.id = "candidate".into();
        candidate.arguments = vec![SemanticArgument {
            name: "candidate".into(),
            kind: SemanticArgumentKind::String,
            required: true,
            description: "Disposable candidate source".into(),
        }];
        candidate = candidate
            .with_purpose(SemanticOperationPurpose::CandidateEvaluation)
            .unwrap();
        let mut edit = inspect.clone();
        edit.id = "edit".into();
        edit.effect = SemanticOperationEffect::ProposeChanges;
        let mut selection = inspect.clone();
        selection.id = "selection".into();
        selection.target = SemanticTargetRequirement::Selection;
        vec![inspect, candidate, edit, selection]
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        self.events
            .lock()
            .unwrap()
            .push(json!({"id":request.operation_id,
            "source":request.document.text(), "version":request.document.version(),
            "arguments":request.arguments}));
        if let Some(hook) = &self.during_execution {
            hook();
        }
        assert!(request.selection.is_none());
        assert_ne!(request.operation_id, "edit", "prohibited edit dispatched");
        let value = if request.operation_id == "candidate" {
            match request.arguments["candidate"].as_str().unwrap() {
                "float" => json!({"number":1.5}),
                "large" => json!({"number":u64::MAX}),
                source => json!({"status":"rejected", "candidate":source,
                    "diagnostics":[{"code":"domain_rule","message":"candidate violates fixture rule"}]}),
            }
        } else {
            json!({"source":request.document.text(), "version":request.document.version()})
        };
        Ok(SemanticOperationResult::ReadOnly {
            value,
            rationale: "Fixture observation; no installation".into(),
        })
    }
}
fn fixture(service: Service) -> (TempWorkspace, App, &'static Service) {
    static NEXT_FIXTURE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let sequence = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = TempWorkspace::new(&format!("terminal_agent_semantics_{sequence}"));
    root.write_file("sample.fixture", "disk");
    let service = Box::leak(Box::new(service));
    let mut app = App::builder(root.workspace())
        .language_registry(LanguageRegistry::empty().with_language(service))
        .build()
        .unwrap();
    app.open_path("sample.fixture").unwrap();
    app.open_document_mut()
        .unwrap()
        .apply_edit(crate::text::TextEdit::insert(
            crate::text::Position::new(0, 4),
            " unsaved é",
        ))
        .unwrap();
    (root, app, service)
}
fn envelope(app: &App, arguments: Value) -> Value {
    let doc = app.open_document().unwrap();
    json!({"expected_version":doc.version().to_string(),"expected_source":doc.text(),"arguments":arguments})
}
fn call(id: &str, arguments: Value) -> Value {
    json!({"type":"tool","name":format!("language.terminal-fixture.{id}"),"arguments":arguments})
}
fn ids() -> Vec<String> {
    ["inspect", "candidate", "edit", "selection"]
        .map(String::from)
        .to_vec()
}
fn run(
    app: &mut App,
    model: &mut Model,
    allowed: &[String],
    authorize: &mut dyn for<'b> FnMut(SemanticAuthorization<'b>) -> bool,
    limits: AgentLimits,
) -> Result<AgentResponse, AgentError> {
    run_terminal_agent_model(
        app,
        None,
        model,
        "Inspect without modifying source",
        limits,
        Some(TerminalSemanticConfig {
            allowed_operation_ids: allowed,
            authorize,
        }),
    )
}

#[test]
fn discovery_and_live_unsaved_calls_preserve_document_disk_and_authority() {
    let (root, mut app, service) = fixture(Service::default());
    let before = app.open_document().unwrap().clone();
    let context = crate::agent::AgentContext::from_app(&app).render();
    let mut grants = vec![];
    let mut authorize = |a: SemanticAuthorization<'_>| {
        assert_eq!(a.source.text, before.text());
        assert_eq!(a.source.version, before.version());
        grants.push((a.operation.id.clone(), a.arguments.clone()));
        true
    };
    let candidate = "arbitrary candidate \"é\"";
    let mut model = Model::new([
        call("inspect", envelope(&app, json!({}))),
        call("candidate", envelope(&app, json!({"candidate":candidate}))),
    ]);
    assert_eq!(
        run(
            &mut app,
            &mut model,
            &ids(),
            &mut authorize,
            AgentLimits::default()
        )
        .unwrap()
        .content,
        "finished"
    );
    assert!(model.replies.is_empty());
    let initial = model.initial();
    assert!(initial["request"].as_str().unwrap().contains(before.text()));
    let tools = initial["catalog"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    for (entry, id) in tools.iter().zip(["inspect", "candidate"]) {
        assert_eq!(
            entry["definition"]["name"],
            format!("language.terminal-fixture.{id}")
        );
        let schema = &entry["metadata"]["invocation_schema"];
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["required"],
            json!(["expected_version", "expected_source", "arguments"])
        );
        assert_eq!(
            schema["properties"]["arguments"]["additionalProperties"],
            false
        );
        assert_eq!(entry["metadata"]["effect"], "Read");
    }
    assert_eq!(tools[1]["metadata"]["purpose"], "CandidateEvaluation");
    assert_eq!(
        tools[1]["metadata"]["invocation_schema"]["properties"]["arguments"]["required"],
        json!(["candidate"])
    );
    assert_eq!(
        tools[1]["metadata"]["invocation_schema"]["properties"]["arguments"]["properties"]["candidate"]
            ["type"],
        "string"
    );
    let feedback = model.feedback();
    assert_eq!(feedback.len(), 2);
    assert_eq!(feedback[0]["value"]["value"]["source"], before.text());
    assert_eq!(feedback[1]["ok"], true);
    assert_eq!(feedback[1]["value"]["status"], "read_only");
    assert_eq!(feedback[1]["value"]["value"]["status"], "rejected");
    assert_eq!(feedback[1]["value"]["value"]["candidate"], candidate);
    assert_eq!(
        feedback[1]["value"]["value"]["diagnostics"][0]["code"],
        "domain_rule"
    );
    assert_eq!(grants.len(), 2);
    assert_eq!(service.events.lock().unwrap().len(), 2);
    assert_eq!(app.open_document().unwrap(), &before);
    assert_eq!(root.read_file("sample.fixture"), "disk");
    assert_eq!(crate::agent::AgentContext::from_app(&app).render(), context);
}

#[test]
fn deny_policy_is_independent_of_candidate_discovery() {
    let (_, mut app, service) = fixture(Service::default());
    let mut seen = vec![];
    let mut policy = |a: SemanticAuthorization<'_>| {
        seen.push(a.operation.id.clone());
        a.operation.id != "candidate"
    };
    let mut model = Model::new([
        call("candidate", envelope(&app, json!({"candidate":"rejected"}))),
        call("inspect", envelope(&app, json!({}))),
    ]);
    run(
        &mut app,
        &mut model,
        &ids(),
        &mut policy,
        AgentLimits::default(),
    )
    .unwrap();
    assert_eq!(seen, ["candidate", "inspect"]);
    assert_eq!(model.feedback()[0]["value"]["status"], "unauthorized");
    assert_eq!(model.feedback()[1]["value"]["status"], "read_only");
    assert_eq!(service.events.lock().unwrap().len(), 1);
}

#[test]
fn hidden_tools_never_dispatch_or_request_host_authorization() {
    let (_, mut app, service) = fixture(Service::default());
    let args = envelope(&app, json!({}));
    let mut model = Model::new(
        ["edit", "selection", "unknown", "inspect"]
            .map(|id| call(id, args.clone()))
            .into_iter()
            .chain([
                json!({"type":"tool","name":"scope.document.replace","arguments":{}}),
                json!({"type":"tool","name":"scope.document.read","arguments":{}}),
            ]),
    );
    run(
        &mut app,
        &mut model,
        &["edit".into(), "selection".into()],
        &mut |_| panic!("hidden tool authorized"),
        AgentLimits::default(),
    )
    .unwrap();
    assert_eq!(model.initial()["catalog"]["tools"], json!([]));
    assert!(model.feedback().iter().all(|f| f["ok"] == false));
    assert!(service.events.lock().unwrap().is_empty());
}

#[test]
fn invalid_arguments_and_stale_preconditions_recover_without_dispatch() {
    let (_, mut app, service) = fixture(Service::default());
    let valid = envelope(&app, json!({"candidate":"x"}));
    let mut bad = vec![
        json!({}),
        envelope(&app, json!({})),
        envelope(&app, json!({"candidate":42})),
        envelope(&app, json!({"candidate":"x","extra":true})),
    ];
    for (field, value) in [
        ("expected_version", json!(1)),
        ("expected_version", json!("18446744073709551616")),
        ("expected_source", Value::Null),
        ("arguments", json!([])),
        ("target_id", json!("model-selected")),
    ] {
        let mut args = valid.clone();
        args[field] = value;
        bad.push(args);
    }
    let invalid_count = bad.len();
    for (field, value) in [
        ("expected_version", json!("0")),
        ("expected_source", json!("disk")),
    ] {
        let mut args = valid.clone();
        args[field] = value;
        bad.push(args);
    }
    let mut model = Model::new(bad.into_iter().map(|args| call("candidate", args)));
    run(
        &mut app,
        &mut model,
        &ids(),
        &mut |_| panic!("invalid invocation authorized"),
        AgentLimits {
            max_tool_calls: 20,
            max_decisions: 25,
            ..AgentLimits::default()
        },
    )
    .unwrap();
    for (i, feedback) in model.feedback().iter().enumerate() {
        assert_eq!(feedback["ok"], true);
        assert_eq!(
            feedback["value"]["status"],
            if i < invalid_count {
                "invalid_arguments"
            } else {
                "revision_conflict"
            }
        );
    }
    assert!(service.events.lock().unwrap().is_empty());
}

#[test]
fn absent_document_and_invalid_catalog_fail_before_model_decision() {
    let root = TempWorkspace::new("terminal_no_document");
    let mut app = App::new(root.workspace()).unwrap();
    let mut model = Model::new([]);
    assert!(matches!(
        run(
            &mut app,
            &mut model,
            &ids(),
            &mut |_| true,
            AgentLimits::default()
        ),
        Err(AgentError::Tool(ToolError::Denied { .. }))
    ));
    assert!(model.conversations.is_empty());
    for invalid in ["", "candidate"] {
        let (_, mut app, _) = fixture(Service {
            invalid: Some(invalid),
            ..Service::default()
        });
        let mut model = Model::new([]);
        assert!(matches!(
            run(
                &mut app,
                &mut model,
                &ids(),
                &mut |_| true,
                AgentLimits::default()
            ),
            Err(AgentError::Tool(ToolError::DuplicateTool { .. }))
        ));
        assert!(model.conversations.is_empty());
    }
}

#[test]
fn no_matching_registered_service_fails_before_discovery_or_authorization() {
    let root = TempWorkspace::new("terminal_agent_no_matching_service");
    root.write_file("unmatched.fixture", "live source");
    let mut app = App::builder(root.workspace())
        .language_registry(LanguageRegistry::empty())
        .build()
        .unwrap();
    app.open_path("unmatched.fixture").unwrap();
    assert!(app.open_language().is_none());
    let before = app.open_document().unwrap().clone();
    let mut model = Model::new([]);
    let result = run(
        &mut app,
        &mut model,
        &ids(),
        &mut |_| panic!("unmatched service must not request authorization"),
        AgentLimits::default(),
    );
    match result {
        Err(AgentError::Tool(ToolError::Denied { tool })) => {
            assert_eq!(
                tool,
                "language.semantic: no matching registered language service"
            );
        }
        other => panic!("expected unmatched-service denial, got {other:?}"),
    }
    assert!(model.conversations.is_empty());
    assert_eq!(model.replies.len(), 1);
    assert_eq!(app.open_document().unwrap(), &before);
    assert_eq!(root.read_file("unmatched.fixture"), "live source");
}

#[test]
fn reopening_identical_path_requires_authorizing_the_new_document_identity() {
    let (root, mut app, service) = fixture(Service::default());
    // Start from a disk snapshot so reopening preserves both text and version.
    app.open_path("sample.fixture").unwrap();
    let old = app.open_document().unwrap().clone();
    let arguments = envelope(&app, json!({}));
    let mut model = Model::new([call("inspect", arguments.clone())]);
    let mut old_policy = |a: SemanticAuthorization<'_>| {
        a.source.id == crate::source_edit::SourceId::Document(old.id())
    };
    run(
        &mut app,
        &mut model,
        &ids(),
        &mut old_policy,
        AgentLimits::default(),
    )
    .unwrap();
    assert_eq!(model.feedback()[0]["value"]["status"], "read_only");

    app.open_path("sample.fixture").unwrap();
    let reopened = app.open_document().unwrap().clone();
    assert_eq!(reopened.path(), old.path());
    assert_eq!(reopened.text(), old.text());
    assert_eq!(reopened.version(), old.version());
    assert_ne!(reopened.id(), old.id());
    let mut model = Model::new([call("inspect", arguments.clone())]);
    run(
        &mut app,
        &mut model,
        &ids(),
        &mut old_policy,
        AgentLimits::default(),
    )
    .unwrap();
    assert_eq!(model.feedback()[0]["value"]["status"], "unauthorized");
    assert_eq!(service.events.lock().unwrap().len(), 1);

    let mut model = Model::new([call("inspect", arguments)]);
    let mut new_policy = |a: SemanticAuthorization<'_>| {
        assert_eq!(
            a.source.id,
            crate::source_edit::SourceId::Document(reopened.id())
        );
        true
    };
    run(
        &mut app,
        &mut model,
        &ids(),
        &mut new_policy,
        AgentLimits::default(),
    )
    .unwrap();
    assert_eq!(model.feedback()[0]["value"]["status"], "read_only");
    assert_eq!(service.events.lock().unwrap().len(), 2);
    assert_eq!(app.open_document().unwrap(), &reopened);
    assert_eq!(root.read_file("sample.fixture"), "disk");
}

#[test]
fn conflicting_file_highlight_does_not_replace_the_open_document_target() {
    let (root, mut app, service) = fixture(Service::default());
    root.write_file("zzz-other.fixture", "not the target");
    app.refresh_files().unwrap();
    app.move_selection_down();
    assert_eq!(
        app.selected_file().unwrap().path.to_string_lossy(),
        "zzz-other.fixture"
    );
    let before = app.open_document().unwrap().clone();
    let mut model = Model::new([call("inspect", envelope(&app, json!({})))]);
    let mut policy = |a: SemanticAuthorization<'_>| {
        assert_eq!(
            a.source.id,
            crate::source_edit::SourceId::Document(before.id())
        );
        assert_eq!(a.source.text, before.text());
        true
    };
    run(
        &mut app,
        &mut model,
        &ids(),
        &mut policy,
        AgentLimits::default(),
    )
    .unwrap();
    assert_eq!(
        model.feedback()[0]["value"]["value"]["source"],
        before.text()
    );
    assert_eq!(service.events.lock().unwrap()[0]["source"], before.text());
    assert_eq!(app.open_document().unwrap(), &before);
    assert_eq!(
        app.selected_file().unwrap().path.to_string_lossy(),
        "zzz-other.fixture"
    );
    assert_eq!(root.read_file("zzz-other.fixture"), "not the target");
}

#[test]
fn absent_catalog_and_empty_allowlist_do_not_fall_back_to_generic_tools() {
    for empty_catalog in [false, true] {
        let (_, mut app, service) = fixture(Service {
            empty: empty_catalog,
            ..Service::default()
        });
        let mut model = Model::new([call("inspect", envelope(&app, json!({})))]);
        let allowed = if empty_catalog { ids() } else { vec![] };
        run(
            &mut app,
            &mut model,
            &allowed,
            &mut |_| panic!("unexpected authorization"),
            AgentLimits::default(),
        )
        .unwrap();
        assert_eq!(model.initial()["catalog"]["tools"], json!([]));
        assert_eq!(model.feedback()[0]["ok"], false);
        assert!(service.events.lock().unwrap().is_empty());
    }
}

#[test]
fn unsupported_result_numbers_are_structured_failures_and_loop_continues() {
    let (_, mut app, service) = fixture(Service::default());
    let mut model = Model::new(
        ["float", "large"].map(|s| call("candidate", envelope(&app, json!({"candidate":s})))),
    );
    run(
        &mut app,
        &mut model,
        &ids(),
        &mut |_| true,
        AgentLimits::default(),
    )
    .unwrap();
    for feedback in model.feedback() {
        assert_eq!(feedback["ok"], true);
        assert_eq!(feedback["value"]["status"], "internal_failure");
        assert!(
            feedback["value"]["message"]
                .as_str()
                .unwrap()
                .contains("representable")
        );
    }
    assert_eq!(service.events.lock().unwrap().len(), 2);
}

#[test]
fn transport_failure_and_budgets_propagate_without_semantic_dispatch() {
    let (_, mut app, service) = fixture(Service::default());
    let mut model = Model::new([]);
    model.replies = [Err("fixture transport offline".into())].into();
    let error = run(
        &mut app,
        &mut model,
        &ids(),
        &mut |_| true,
        AgentLimits::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("fixture transport offline"));
    for (limits, stop) in [
        (
            AgentLimits {
                max_decisions: 0,
                ..AgentLimits::default()
            },
            AgentStop::DecisionBudget,
        ),
        (
            AgentLimits {
                max_tool_calls: 0,
                ..AgentLimits::default()
            },
            AgentStop::ToolBudget,
        ),
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
        let mut model = Model::new([call("inspect", envelope(&app, json!({})))]);
        assert!(
            matches!(run(&mut app, &mut model, &ids(), &mut |_| true, limits), Err(AgentError::Stopped(actual)) if actual == stop)
        );
    }
    let mut model = Model::new([]);
    model.replies = [Ok("not json".into())].into();
    assert!(matches!(
        run(
            &mut app,
            &mut model,
            &ids(),
            &mut |_| true,
            AgentLimits {
                max_malformed: 0,
                ..AgentLimits::default()
            }
        ),
        Err(AgentError::Stopped(AgentStop::MalformedBudget))
    ));
    assert!(service.events.lock().unwrap().is_empty());
}
