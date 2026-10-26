//! Downstream-style tests: public shell, host hook, and caller-owned sources only.
//! Wire in cli.rs with #[cfg(all(test, feature = "agent"))] mod retained_agent_shell_tests;
use crate::{
    agent::{AgentLimits, AgentStop, DecisionMessage, DecisionModel},
    app::App,
    cli::{
        ReadOnlyCommand, RetainedScratchAgentRun, SemanticAgentBinding, run_borrowed_with_sources,
    },
    document::Document,
    language::{InteractiveSession, LanguageRegistry},
    scratch::ScratchBuffers,
    test_support::TempWorkspace,
    text::{Position, TextEdit, TextRange},
    tool::semantic::SemanticAuthorization,
};
use scope_language::*;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{Cursor, Write},
    sync::Mutex,
};

#[derive(Default)]
struct Service {
    calls: Mutex<Vec<Value>>,
}
impl LanguageService for Service {
    fn language_id(&self) -> &'static str {
        "retained-fixture"
    }
    fn language_name(&self) -> &'static str {
        "Retained fixture"
    }
    fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
        true
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        Default::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        let inspect = SemanticOperationDescription {
            id: "inspect".into(),
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
            description: "Disposable candidate".into(),
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
        assert!(request.selection.is_none());
        assert!(matches!(request.operation_id, "inspect" | "candidate"));
        self.calls
            .lock()
            .unwrap()
            .push(json!({"operation": request.operation_id,
            "text": request.document.text(), "version": request.document.version(),
            "arguments": request.arguments}));
        Ok(SemanticOperationResult::ReadOnly {
            value: json!({"text": request.document.text(), "status":"rejected"}),
            rationale: "Observation only; candidate is never installed".into(),
        })
    }
}

struct Model {
    replies: VecDeque<Result<String, String>>,
    conversations: Vec<Vec<DecisionMessage>>,
}
impl Model {
    fn new(replies: impl IntoIterator<Item = Value>) -> Self {
        Self {
            replies: replies
                .into_iter()
                .chain([json!({"type":"complete","content":"finished"})])
                .map(|value| Ok(value.to_string()))
                .collect(),
            conversations: vec![],
        }
    }
}
impl DecisionModel for Model {
    fn decide(&mut self, messages: &[DecisionMessage], tokens: u32) -> Result<String, String> {
        assert!(tokens > 0);
        self.conversations.push(messages.to_vec());
        self.replies.pop_front().expect("unexpected model request")
    }
}
fn call(id: &str, arguments: Value) -> Value {
    json!({"type":"tool", "name":format!("language.retained-fixture.{id}"), "arguments":arguments})
}
struct Host<'a> {
    service: &'a Service,
    model: Model,
    ids: Vec<String>,
    limits: AgentLimits,
    authorize: &'a mut dyn for<'b> FnMut(SemanticAuthorization<'b>) -> bool,
    observe: &'a mut dyn FnMut(&RetainedScratchAgentRun<'_>),
}
impl ReadOnlyCommand for Host<'_> {
    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("App command fallback")
    }
    fn semantic_agent(&mut self) -> Option<SemanticAgentBinding<'_>> {
        Some(SemanticAgentBinding {
            service: self.service,
            trace: None,
            allowed_operation_ids: &self.ids,
            authorize: self.authorize,
            limits: self.limits,
            model: Some(&mut self.model),
            observe: self.observe,
        })
    }
}
fn host<'a>(
    service: &'a Service,
    model: Model,
    authorize: &'a mut dyn for<'b> FnMut(SemanticAuthorization<'b>) -> bool,
    observe: &'a mut dyn FnMut(&RetainedScratchAgentRun<'_>),
) -> Host<'a> {
    Host {
        service,
        model,
        authorize,
        observe,
        limits: AgentLimits::default(),
        ids: ["inspect", "candidate", "edit", "selection"]
            .map(str::to_owned)
            .into(),
    }
}
fn fixture() -> (TempWorkspace, App, &'static Service, ScratchBuffers) {
    static NEXT_FIXTURE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let sequence = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = TempWorkspace::new(&format!("retained-agent-shell-{sequence}"));
    root.write_file("sample.fixture", "disk");
    let service = Box::leak(Box::new(Service::default()));
    let mut app = App::builder(root.workspace())
        .language_registry(LanguageRegistry::empty().with_language(service))
        .build()
        .unwrap();
    app.open_path("sample.fixture").unwrap();
    app.open_document_mut()
        .unwrap()
        .apply_edit(TextEdit::insert(Position::new(0, 4), " App draft"))
        .unwrap();
    let mut sources = ScratchBuffers::default();
    let id = sources.create("retained-fixture", "unsaved café 🦀\r\n");
    sources.bind_semantic_target(Some(id)).unwrap();
    (root, app, service, sources)
}
fn shell(app: &mut App, sources: &mut ScratchBuffers, host: &mut Host<'_>, script: &str) -> String {
    let mut output = vec![];
    run_borrowed_with_sources(
        app,
        &mut InteractiveSession::new(),
        Some(host),
        sources,
        Cursor::new(script),
        &mut output,
    )
    .unwrap();
    String::from_utf8(output).unwrap()
}
const RUN: &str = ":scope agent inspect retained source\n:scope quit\n";

fn trace_events(run: &RetainedScratchAgentRun<'_>) -> Vec<Value> {
    assert!(
        !run.trace_truncated,
        "small fixture trace unexpectedly truncated"
    );
    std::str::from_utf8(&run.trace_jsonl)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

// ProtocolTrace has no separate discovery/dispatch event: discovery is the
// request catalog, and a dispatched call is recorded by tool_result feedback.
fn assert_discovery_and_dispatch(events: &[Value]) {
    let request = events
        .iter()
        .find(|event| event["event"] == "request")
        .unwrap();
    let initial: Value =
        serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(initial["kind"], "request");
    assert!(
        initial["catalog"]
            .to_string()
            .contains("language.retained-fixture.inspect")
    );
    assert!(events.iter().any(|event| event["event"] == "decision"
        && event["accepted"] == true
        && event["kind"] == "tool"));
    assert!(events.iter().any(|event| event["event"] == "feedback"
        && event["envelope"]["kind"] == "tool_result"
        && event["envelope"]["name"] == "language.retained-fixture.inspect"
        && event["envelope"]["ok"] == true
        && event["retained"] == true));
}

#[test]
fn j_normal_shell_corrects_malformed_decision_before_dispatch() {
    let (_root, mut app, service, mut sources) = fixture();
    let mut authorize = |_: SemanticAuthorization<'_>| true;
    let mut observed = 0;
    let mut observe = |run: &RetainedScratchAgentRun<'_>| {
        observed += 1;
        assert_eq!(run.run.stop, AgentStop::Completed);
        assert_eq!(
            (run.run.decisions, run.run.tool_calls, run.run.malformed),
            (3, 1, 1)
        );
        let events = trace_events(run);
        assert_discovery_and_dispatch(&events);
        assert!(events.iter().any(|event| event["event"] == "decision"
            && event["accepted"] == false
            && event["category"] == "incomplete_response"));
        let correction = events
            .iter()
            .find(|event| {
                event["event"] == "feedback" && event["envelope"]["kind"] == "protocol_error"
            })
            .unwrap();
        assert_eq!(correction["retained"], true);
        assert!(
            correction["envelope"]["instruction"]
                .as_str()
                .unwrap()
                .contains("no tool was dispatched")
                || correction["envelope"]["instruction"]
                    .as_str()
                    .unwrap()
                    .contains("No tool was dispatched")
        );
        assert_eq!(events.last().unwrap()["event"], "outcome");
        assert_eq!(events.last().unwrap()["stop"], "Completed");
    };
    let mut model = Model::new([call("inspect", json!({}))]);
    model.replies.push_front(Ok("{\"type\":".into()));
    let mut host = host(service, model, &mut authorize, &mut observe);
    shell(&mut app, &mut sources, &mut host, RUN);
    let correction: Value =
        serde_json::from_str(&host.model.conversations[1].last().unwrap().content).unwrap();
    assert_eq!(correction["kind"], "protocol_error");
    assert_eq!(service.calls.lock().unwrap().len(), 1);
    assert_eq!(observed, 1);
}

#[test]
fn k_normal_shell_stops_malformed_no_progress_without_dispatch() {
    let (_root, mut app, service, mut sources) = fixture();
    let mut authorize =
        |_: SemanticAuthorization<'_>| -> bool { panic!("malformed decision authorized") };
    let mut observed = 0;
    let mut observe = |run: &RetainedScratchAgentRun<'_>| {
        observed += 1;
        assert_eq!(run.run.stop, AgentStop::MalformedBudget);
        assert_eq!(
            (run.run.decisions, run.run.tool_calls, run.run.malformed),
            (2, 0, 2)
        );
        let events = trace_events(run);
        assert_eq!(
            events
                .iter()
                .filter(|event| event["event"] == "decision" && event["accepted"] == false)
                .count(),
            2
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event["event"] == "feedback"
                    && event["envelope"]["kind"] == "protocol_error")
                .count(),
            1
        );
        assert!(!events.iter().any(
            |event| event["event"] == "feedback" && event["envelope"]["kind"] == "tool_result"
        ));
        assert_eq!(events.last().unwrap()["stop"], "MalformedBudget");
    };
    let mut model = Model::new([]);
    model.replies = [Ok("not json".into()), Ok("not json again".into())].into();
    let mut host = host(service, model, &mut authorize, &mut observe);
    host.limits.max_malformed = 1;
    shell(&mut app, &mut sources, &mut host, RUN);
    assert_eq!(host.model.conversations.len(), 2);
    assert!(service.calls.lock().unwrap().is_empty());
    assert_eq!(observed, 1);
}

#[test]
fn l_saved_evidence_keeps_its_revision_and_next_run_gets_fresh_evidence() {
    let (root, mut app, service, mut sources) = fixture();
    let mut saved = vec![];
    let mut authorize = |_: SemanticAuthorization<'_>| true;
    let mut observe = |run: &RetainedScratchAgentRun<'_>| {
        let events = trace_events(run);
        assert_discovery_and_dispatch(&events);
        assert_eq!(events[0]["event"], "source_binding");
        assert_eq!(events[0]["version"], run.source.version);
        saved.push((
            run.source.id,
            run.source.version,
            run.source.language.to_owned(),
            run.source.text.to_owned(),
            run.run.clone(),
            run.trace_jsonl.clone(),
        ));
    };
    let mut host = host(
        service,
        Model::new([call("inspect", json!({}))]),
        &mut authorize,
        &mut observe,
    );
    shell(&mut app, &mut sources, &mut host, RUN);
    sources
        .get_mut(0)
        .unwrap()
        .apply_edit(TextEdit::insert(Position::new(0, 0), "revision two: "))
        .unwrap();
    host.model = Model::new([call("inspect", json!({}))]);
    shell(&mut app, &mut sources, &mut host, RUN);
    // The next invocation starts a new conversation, not the previous evidence.
    assert_eq!(host.model.conversations[0].len(), 2);
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].0, saved[1].0);
    assert!(saved[1].1 > saved[0].1);
    assert_eq!(saved[0].2, "retained-fixture");
    assert_eq!(saved[0].3, "unsaved café 🦀\r\n");
    assert_eq!(saved[1].3, "revision two: unsaved café 🦀\r\n");
    for (index, evidence) in saved.iter().enumerate() {
        assert_eq!(evidence.4.stop, AgentStop::Completed);
        let events: Vec<Value> = std::str::from_utf8(&evidence.5)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events[0]["source_id"], format!("{:?}", evidence.0));
        assert_eq!(events[0]["version"], evidence.1);
        let result = events
            .iter()
            .find(|event| {
                event["event"] == "feedback" && event["envelope"]["kind"] == "tool_result"
            })
            .unwrap();
        assert_eq!(result["envelope"]["value"]["value"]["text"], evidence.3);
        if index == 0 {
            assert!(
                !std::str::from_utf8(&evidence.5)
                    .unwrap()
                    .contains("revision two:")
            );
        }
    }
    let calls = service.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    for (call, evidence) in calls.iter().zip(&saved) {
        assert_eq!(call["version"], evidence.1);
        assert_eq!(call["text"], evidence.3);
    }
    assert_eq!(root.read_file("sample.fixture"), "disk");
    assert_eq!(app.open_document().unwrap().text(), "disk App draft");
}

#[test]
fn m_opt_out_and_absent_hosts_use_normal_model_without_requiring_semantic_target() {
    struct OptOut;
    impl ReadOnlyCommand for OptOut {
        fn invoke(
            &mut self,
            _: &Document,
            _: TextRange,
            _: &str,
            _: &mut dyn Write,
        ) -> Result<(), Box<dyn std::error::Error>> {
            panic!("host operation fallback")
        }
    }
    for has_host in [false, true] {
        let (_root, mut app, service, mut sources) = fixture();
        sources.bind_semantic_target(None).unwrap();
        let mut output = vec![];
        let mut host = OptOut;
        // Only this regression uses the configured inner shell: the public
        // borrowed entry point intentionally fixes the normal endpoint config.
        super::run_shell_borrowed_with_sources(
            &mut app,
            &mut InteractiveSession::new(),
            crate::agent::AgentConfig::local("fixture", "not a valid URL"),
            Cursor::new(RUN),
            &mut output,
            false,
            None,
            if has_host {
                Some(&mut host as &mut dyn ReadOnlyCommand)
            } else {
                None
            },
            &mut sources,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("error:"), "{output}");
        assert!(
            !output.contains("requires a bound semantic target"),
            "{output}"
        );
        assert!(
            output.contains("chat") || output.contains("URL") || output.contains("url"),
            "{output}"
        );
        assert!(service.calls.lock().unwrap().is_empty());
        assert!(sources.semantic_target().unwrap().is_none());
    }
}

#[test]
fn a_live_unsaved_source_has_exact_host_attribution_and_no_mutation() {
    let (root, mut app, service, mut sources) = fixture();
    let before = sources.get(0).unwrap();
    let (id, version, text, dirty) = (
        before.evaluation_source_id().clone(),
        before.version(),
        before.text().to_owned(),
        before.is_dirty(),
    );
    let mut authorizations = 0;
    let mut observations = 0;
    let mut authorize = |a: SemanticAuthorization<'_>| {
        authorizations += 1;
        assert_eq!(a.source.id, crate::source_edit::SourceId::Scratch(0));
        assert_eq!(a.source.language, "retained-fixture");
        assert_eq!(a.source.text, text);
        assert_eq!(a.source.version, version);
        true
    };
    let mut observe = |r: &RetainedScratchAgentRun<'_>| {
        observations += 1;
        assert_eq!(r.source.id, crate::source_edit::SourceId::Scratch(0));
        assert_eq!(r.source.language, "retained-fixture");
        assert_eq!(r.source.text, text);
        assert_eq!(r.source.version, version);
        assert_eq!(r.run.stop, AgentStop::Completed);
        assert_eq!((r.run.decisions, r.run.tool_calls), (2, 1));
        let events = trace_events(r);
        assert_discovery_and_dispatch(&events);
        assert_eq!(events.last().unwrap()["event"], "outcome");
        assert_eq!(events.last().unwrap()["stop"], "Completed");
    };
    let mut host = host(
        service,
        Model::new([call("inspect", json!({}))]),
        &mut authorize,
        &mut observe,
    );
    shell(&mut app, &mut sources, &mut host, RUN);
    let initial = &host.model.conversations[0][1].content;
    assert!(initial.contains("language.retained-fixture.inspect"));
    assert!(!initial.contains("language.retained-fixture.edit"));
    assert!(!initial.contains("language.retained-fixture.selection"));
    assert_eq!(authorizations, 1);
    assert_eq!(observations, 1);
    let after = sources.get(0).unwrap();
    assert_eq!(after.evaluation_source_id(), &id);
    assert_eq!(
        (after.version(), after.text(), after.is_dirty()),
        (version, text.as_str(), dirty)
    );
    assert_eq!(app.open_document().unwrap().text(), "disk App draft");
    assert_eq!(root.read_file("sample.fixture"), "disk");
    assert_eq!(service.calls.lock().unwrap()[0]["text"], text);
}

#[test]
fn b_binding_is_independent_of_selection_and_survives_shell_handoffs() {
    let (_root, mut app, service, mut sources) = fixture();
    sources.create("retained-fixture", "selected decoy");
    let mut seen = vec![];
    let mut observe = |r: &RetainedScratchAgentRun<'_>| {
        seen.push((r.source.version, r.source.text.to_owned()));
    };
    let mut authorize = |_: SemanticAuthorization<'_>| true;
    let mut host = host(service, Model::new([]), &mut authorize, &mut observe);
    shell(&mut app, &mut sources, &mut host, RUN);
    sources
        .get_mut(0)
        .unwrap()
        .apply_edit(TextEdit::insert(Position::new(0, 0), "new "))
        .unwrap();
    host.model = Model::new([]);
    shell(&mut app, &mut sources, &mut host, RUN);
    assert_eq!(sources.selected().unwrap().text(), "selected decoy");
    assert_eq!(seen.len(), 2);
    assert!(seen[1].0 > seen[0].0);
    assert_eq!(seen[1].1, format!("new {}", seen[0].1));
}

#[test]
fn c_absent_closed_and_wrong_language_targets_fail_before_model() {
    for script in [
        ":scope semantic-target none\n:scope agent inspect\n:scope quit\n",
        ":scope discard\n:scope agent inspect\n:scope quit\n",
        ":scope scratch new text\n:scope semantic-target 1\n:scope agent inspect\n:scope quit\n",
    ] {
        let (_root, mut app, service, mut sources) = fixture();
        let mut authorize = |_: SemanticAuthorization<'_>| -> bool {
            panic!("authorization before target validation")
        };
        let mut observe = |_: &RetainedScratchAgentRun<'_>| panic!("invalid target observed");
        let mut host = host(service, Model::new([]), &mut authorize, &mut observe);
        let output = shell(&mut app, &mut sources, &mut host, script);
        assert!(output.contains("error:"), "{output}");
        assert!(host.model.conversations.is_empty());
        assert!(service.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn d_candidate_read_still_requires_authorization_and_is_not_installed() {
    let (_root, mut app, service, mut sources) = fixture();
    let before = sources.get(0).unwrap().text().to_owned();
    let mut count = 0;
    let mut authorize = |a: SemanticAuthorization<'_>| {
        count += 1;
        assert_eq!(a.operation.id, "candidate");
        count == 2
    };
    let mut observe = |r: &RetainedScratchAgentRun<'_>| {
        assert_eq!(r.run.tool_calls, 2);
        let events = trace_events(r);
        let results: Vec<_> = events
            .iter()
            .filter(|event| {
                event["event"] == "feedback" && event["envelope"]["kind"] == "tool_result"
            })
            .collect();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["envelope"]["value"]["status"], "unauthorized");
        assert_eq!(results[1]["envelope"]["ok"], true);
        assert_eq!(events.last().unwrap()["stop"], "Completed");
    };
    let mut host = host(
        service,
        Model::new([
            call("candidate", json!({"candidate":"denied replacement"})),
            call(
                "candidate",
                json!({"candidate":"authorized disposable replacement"}),
            ),
        ]),
        &mut authorize,
        &mut observe,
    );
    shell(&mut app, &mut sources, &mut host, RUN);
    assert_eq!(count, 2);
    let calls = service.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0]["arguments"]["candidate"],
        "authorized disposable replacement"
    );
    assert_eq!(sources.get(0).unwrap().text(), before);
}

#[test]
fn e_hidden_and_generic_tools_cannot_dispatch_or_authorize() {
    let (_root, mut app, service, mut sources) = fixture();
    let mut authorize =
        |_: SemanticAuthorization<'_>| -> bool { panic!("hidden operation authorized") };
    let mut observe = |r: &RetainedScratchAgentRun<'_>| assert_eq!(r.run.tool_calls, 3);
    let mut host = host(
        service,
        Model::new([
            call("edit", json!({})),
            call("selection", json!({})),
            json!({"type":"tool", "name":"scope.read", "arguments":{}}),
        ]),
        &mut authorize,
        &mut observe,
    );
    shell(&mut app, &mut sources, &mut host, RUN);
    assert!(service.calls.lock().unwrap().is_empty());
}

#[test]
fn f_invalid_arguments_recover_without_semantic_dispatch() {
    let (_root, mut app, service, mut sources) = fixture();
    let mut count = 0;
    let mut authorize = |_: SemanticAuthorization<'_>| {
        count += 1;
        true
    };
    let mut observe = |r: &RetainedScratchAgentRun<'_>| assert_eq!(r.run.tool_calls, 2);
    let mut host = host(
        service,
        Model::new([call("candidate", json!({})), call("inspect", json!({}))]),
        &mut authorize,
        &mut observe,
    );
    shell(&mut app, &mut sources, &mut host, RUN);
    assert_eq!(service.calls.lock().unwrap().len(), 1);
    assert!(
        host.model
            .conversations
            .last()
            .unwrap()
            .iter()
            .any(|m| m.content.contains("tool_result"))
    );
    assert_eq!(count, 1);
}

#[test]
fn g_empty_allowlist_never_falls_back_to_generic_tools() {
    let (_root, mut app, service, mut sources) = fixture();
    let mut authorize = |_: SemanticAuthorization<'_>| -> bool { panic!("empty allowlist") };
    let mut observe =
        |r: &RetainedScratchAgentRun<'_>| assert_eq!(r.run.stop, AgentStop::Completed);
    let mut host = host(
        service,
        Model::new([call("inspect", json!({}))]),
        &mut authorize,
        &mut observe,
    );
    host.ids.clear();
    shell(&mut app, &mut sources, &mut host, RUN);
    assert!(service.calls.lock().unwrap().is_empty());
    assert!(
        !host.model.conversations[0][1]
            .content
            .contains("language.retained-fixture.inspect")
    );
}

#[test]
fn h_budget_stop_is_observed_as_a_stop_not_completion() {
    let (_root, mut app, service, mut sources) = fixture();
    let mut observed = 0;
    let mut authorize = |_: SemanticAuthorization<'_>| -> bool { panic!("zero budget dispatch") };
    let mut observe = |r: &RetainedScratchAgentRun<'_>| {
        observed += 1;
        assert_eq!(r.run.stop, AgentStop::DecisionBudget);
        assert_eq!((r.run.decisions, r.run.tool_calls), (0, 0));
        let events = trace_events(r);
        assert!(!events.iter().any(|event| event["event"] == "request"));
        assert_eq!(events.last().unwrap()["event"], "outcome");
        assert_eq!(events.last().unwrap()["stop"], "DecisionBudget");
    };
    let mut host = host(service, Model::new([]), &mut authorize, &mut observe);
    host.limits.max_decisions = 0;
    shell(&mut app, &mut sources, &mut host, RUN);
    assert!(host.model.conversations.is_empty());
    assert_eq!(observed, 1);
}

#[test]
fn i_transport_failure_reports_error_and_keeps_source_and_owner() {
    let (_root, mut app, service, mut sources) = fixture();
    let mut authorize =
        |_: SemanticAuthorization<'_>| -> bool { panic!("transport failure dispatch") };
    let mut observe = |_: &RetainedScratchAgentRun<'_>| panic!("failed run observed");
    let mut host = host(service, Model::new([]), &mut authorize, &mut observe);
    host.model.replies = [Err("fixture offline".into())].into();
    let output = shell(&mut app, &mut sources, &mut host, RUN);
    assert!(output.contains("fixture offline"), "{output}");
    assert_eq!(
        sources.semantic_target().unwrap().unwrap().text(),
        "unsaved café 🦀\r\n"
    );
    assert!(service.calls.lock().unwrap().is_empty());
}
