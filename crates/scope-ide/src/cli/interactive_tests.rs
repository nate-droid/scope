use super::*;
use crate::test_support::TempWorkspace;
use scope_language::{
    Diagnostic, EvaluationResult, EvaluationStatus, InteractiveLanguageSession,
    InteractiveSessionProvider, SessionError,
};
use std::sync::Mutex;

#[test]
fn evaluation_diagnostics_reuse_attribution_without_changing_status_or_payload() {
    for (source, code, label) in [
        (None, None, "error"),
        (Some("parser"), None, "parser: error"),
        (None, Some("E001"), "error[E001]"),
        (Some("parser"), Some("E001"), "parser[E001]: error"),
    ] {
        for (status, suffix) in [
            (EvaluationStatus::Success, ""),
            (EvaluationStatus::Failed, "evaluation failed\n"),
            (
                EvaluationStatus::Cancelled {
                    requires_reset: false,
                },
                "evaluation cancelled\n",
            ),
            (
                EvaluationStatus::Cancelled {
                    requires_reset: true,
                },
                "evaluation cancelled; session reset required\n",
            ),
        ] {
            let mut diagnostic = Diagnostic::error(
                scope_language::TextRange::new(
                    scope_language::Position::new(1, 2),
                    scope_language::Position::new(1, 3),
                ),
                "unexpected token",
            );
            diagnostic.source = source.map(str::to_owned);
            diagnostic.code = code.map(str::to_owned);
            let result = EvaluationResult {
                output: vec!["before diagnostic".into()],
                diagnostics: vec![diagnostic],
                status,
            };
            let retained = result.clone();
            let mut output = Vec::new();
            print_evaluation(result.clone(), &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                format!("before diagnostic\n{label} 2:3: unexpected token\n{suffix}")
            );
            assert_eq!(result, retained);
        }
    }
}

#[test]
fn evaluation_diagnostic_output_failure_is_propagated() {
    let result = EvaluationResult {
        output: vec![],
        diagnostics: vec![Diagnostic::error(
            scope_language::TextRange::new(
                scope_language::Position::new(0, 0),
                scope_language::Position::new(0, 1),
            ),
            "unexpected token",
        )],
        status: EvaluationStatus::Failed,
    };
    let error = print_evaluation(result, &mut std::io::Cursor::new(&mut [][..])).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::WriteZero);
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Create,
    Evaluate(String, usize),
    Reset,
    Close,
}

struct Provider(Mutex<Vec<Event>>, &'static str);

struct MockSession {
    provider: &'static Provider,
    count: usize,
}

// The registry requires static providers; each test leaks its own independent
// provider and registration wrapper so parallel tests never share event state.
impl InteractiveSessionProvider for &'static Provider {
    fn language_id(&self) -> &'static str {
        self.1
    }

    fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
        self.0.lock().unwrap().push(Event::Create);
        Ok(Box::new(MockSession {
            provider: self,
            count: 0,
        }))
    }
}

impl InteractiveLanguageSession for MockSession {
    fn language_id(&self) -> &str {
        self.provider.1
    }

    fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
        self.count += 1;
        self.provider
            .0
            .lock()
            .unwrap()
            .push(Event::Evaluate(source.into(), self.count));
        let mut result = EvaluationResult {
            output: vec![format!("{}[{}]: {source}", self.language_id(), self.count)],
            diagnostics: vec![],
            status: EvaluationStatus::Success,
        };
        match source {
            "reset-required" => {
                result.status = EvaluationStatus::Cancelled {
                    requires_reset: true,
                };
            }
            "fail" => {
                result.output = vec!["before diagnostic".into(), "already newline\n".into()];
                result.diagnostics = vec![Diagnostic::error(
                    scope_language::TextRange::new(
                        scope_language::Position::new(1, 2),
                        scope_language::Position::new(1, 3),
                    ),
                    "mock syntax failure",
                )];
                result.status = EvaluationStatus::Failed;
            }
            "auth" => {
                return Err(SessionError::Authorization {
                    code: "mock_denied".into(),
                    message: "mock permission denied".into(),
                });
            }
            "infra" => {
                return Err(SessionError::Infrastructure {
                    code: "mock_unavailable".into(),
                    message: "mock transport unavailable".into(),
                });
            }
            "empty" => result.output.clear(),
            _ => {}
        }
        Ok(result)
    }

    fn inspect(
        &self,
        name: &str,
    ) -> Result<Option<scope_language::SymbolDescription>, scope_language::InspectionError> {
        let events = self.provider.0.lock().unwrap();
        let definition = events
            .iter()
            .rev()
            .take_while(|event| !matches!(event, Event::Reset | Event::Create))
            .find_map(|event| match event {
                Event::Evaluate(source, _) if source == "define answer" && name == "answer" => {
                    Some(source.clone())
                }
                _ => None,
            });
        Ok(
            definition.map(|definition| scope_language::SymbolDescription {
                name: name.into(),
                kind: scope_language::SymbolKind::Function,
                signature: Some("( -- value )".into()),
                documentation: Some("mock evaluated definition".into()),
                origin: Some(scope_language::SymbolOrigin::Interactive),
                definition: Some(definition),
                location: None,
            }),
        )
    }

    fn reset(&mut self) -> Result<(), SessionError> {
        self.count = 0;
        self.provider.0.lock().unwrap().push(Event::Reset);
        Ok(())
    }

    fn close(&mut self) -> Result<(), SessionError> {
        self.provider.0.lock().unwrap().push(Event::Close);
        Ok(())
    }
}

fn setup() -> (&'static Provider, LanguageRegistry) {
    let provider = provider("mock");
    let registration = Box::leak(Box::new(provider));
    (
        provider,
        LanguageRegistry::empty().with_interactive_provider(registration),
    )
}

fn provider(id: &'static str) -> &'static Provider {
    Box::leak(Box::new(Provider(Mutex::new(Vec::new()), id)))
}

#[cfg(not(feature = "agent"))]
fn agent_config() -> AgentConfig {
    AgentConfig
}

#[cfg(feature = "agent")]
fn agent_config() -> AgentConfig {
    AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL)
}

fn shell(input: &str) -> (String, Vec<Event>) {
    let root = TempWorkspace::new("cli_interactive");
    let (provider, registry) = setup();
    let app = App::builder(root.workspace())
        .language_registry(registry)
        .build()
        .unwrap();
    let mut output = Vec::new();
    run_shell(app, agent_config(), input.as_bytes(), &mut output).unwrap();
    let events = provider.0.lock().unwrap().drain(..).collect();
    (String::from_utf8(output).unwrap(), events)
}

#[test]
fn line_edit_evaluates_selected_unsaved_source_in_active_session() {
    let (output, events) = shell(concat!(
        ":scope session mock\n",
        ":scope scratch new mock\n",
        ":scope line 1 \"  edited source\"\n",
        ":scope show\n",
        ":scope buffer eval\n",
        ":scope quit\n",
    ));
    assert!(output.contains("  edited source"), "{output}");
    assert!(
        events.contains(&Event::Evaluate("  edited source".into(), 1)),
        "{events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Evaluate(_, _)))
            .count(),
        1
    );
}

fn evaluated(source: &str, count: usize) -> Event {
    Event::Evaluate(source.into(), count)
}

#[test]
fn context_reports_reset_block_without_resetting_or_evaluating() {
    let (output, events) = shell(
        ":scope scratch new mock\n:scope session mock\nreset-required\n:scope context\n:scope context\n:scope session-reset\n:scope context\n:scope quit\n",
    );
    assert_eq!(
        output
            .matches("Buffer evaluation target: unavailable (session reset required)")
            .count(),
        2
    );
    assert!(output.contains("Buffer evaluation target: Scratch(0) @0"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("reset-required", 1),
            Event::Reset,
            Event::Close
        ]
    );
}

#[test]
fn context_is_observational_and_namespace_controls_session_routing() {
    let (output, events) = shell(
        ":scope scratch new mock\n:scope context\n:scope session mock\ncontext\n:scope context\n:scope buffer eval\n:scope scratch new other\n:scope context\n:scope session none\ncontext\nquit\n",
    );
    assert!(output.contains("Interactive session: mock"));
    assert!(output.contains("Buffer evaluation target: Scratch(0) @0"));
    assert!(
        output.contains("Buffer evaluation target: unavailable (buffer/session language mismatch)")
    );
    assert!(output.contains("Buffer evaluation target: unavailable (no interactive session)"));
    assert!(output.contains("Shell buffer target: Scratch(0) @0"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("context", 1),
            evaluated("", 2),
            Event::Close
        ]
    );
}

#[test]
fn explicit_selection_routes_source_and_namespaced_commands() {
    let (output, events) = shell(
        ":scope session mock\nfirst\n  second  \n:scope nonexistent\nhelp\n:scope help\n:scope session\n:scope quit\nignored\n",
    );
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("first", 1),
            evaluated("  second  ", 2),
            evaluated("help", 3),
            Event::Close,
        ]
    );
    assert!(output.contains("Active language: mock"));
    assert!(output.contains("mock> mock[1]: first\n"));
    assert!(output.contains("mock[2]:   second  \n"));
    assert!(output.contains("error: unknown command: nonexistent"));
    assert!(output.contains("mock[3]: help\n"));
    assert_eq!(output.matches("Interactive sessions:\n").count(), 1);
    assert_eq!(output.matches("Active language: mock").count(), 2);
    assert!(!output.contains("ignored"));
}

#[test]
fn session_selection_requires_explicit_namespace() {
    let (output, events) = shell("session mock\n:scope session\nquit\n");
    assert!(events.is_empty());
    assert!(output.contains("error: unknown command: session"));
    assert!(output.contains("Active language: none"));
}

#[test]
fn reset_clears_state_and_none_returns_to_legacy_shell() {
    let (output, events) = shell(
        ":scope session mock\none\ntwo\n:scope session-reset\nthree\n:scope session none\nhelp\nquit\n",
    );
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("one", 1),
            evaluated("two", 2),
            Event::Reset,
            evaluated("three", 1),
            Event::Close,
        ]
    );
    assert!(output.contains("session reset\n"));
    assert!(output.contains("mock[1]: three\n"));
    assert!(output.contains("Active language: none\nscope> Discovery:"));
}

#[test]
fn language_discovery_in_active_session_is_observational() {
    let (output, events) =
        shell(":scope session mock\n:scope languages\n:scope session\n:scope quit\n");
    assert!(
        output.contains("Interactive evaluators:\n  mock"),
        "{output}"
    );
    assert!(output.contains("Active session: mock"), "{output}");
    assert!(output.contains("Active language: mock"), "{output}");
    assert_eq!(events, vec![Event::Create, Event::Close]);
}

#[test]
fn unsupported_selection_preserves_active_session_and_state() {
    let (output, events) = shell(
        ":scope session mock\nbefore\n:scope session missing\n:scope session\nafter\n:scope quit\n",
    );
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("before", 1),
            evaluated("after", 2),
            Event::Close,
        ]
    );
    assert!(output.contains("error: unsupported language: missing\n"));
    assert_eq!(output.matches("Active language: mock").count(), 2);
    assert!(output.contains("mock[2]: after\n"));
}

#[test]
fn selecting_same_language_recreates_session() {
    let (output, events) =
        shell(":scope session mock\nbefore\n:scope session mock\nafter\n:scope quit\n");
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("before", 1),
            Event::Create,
            Event::Close,
            evaluated("after", 1),
            Event::Close,
        ]
    );
    assert!(output.contains("mock[1]: after\n"));
}

#[test]
fn eof_and_explicit_quit_close_active_resources_once() {
    for ending in ["", ":scope quit\nnot evaluated\n"] {
        let (_, events) = shell(&format!(":scope session mock\nsource\n{ending}"));
        assert_eq!(
            events,
            vec![Event::Create, evaluated("source", 1), Event::Close]
        );
    }
}

#[test]
fn structured_language_failure_is_rendered_and_recoverable() {
    let (output, events) = shell(":scope session mock\nfail\nrecovered\n:scope quit\n");
    assert!(output.contains(
        "before diagnostic\nalready newline\nerror 2:3: mock syntax failure\nevaluation failed\n"
    ));
    assert!(output.contains("mock[2]: recovered\n"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("fail", 1),
            evaluated("recovered", 2),
            Event::Close,
        ]
    );
}

#[test]
fn authorization_and_infrastructure_errors_do_not_end_shell() {
    let (output, events) = shell(":scope session mock\nauth\ninfra\nrecovered\n:scope quit\n");
    assert!(output.contains("error: authorization error (mock_denied): mock permission denied\n"));
    assert!(
        output.contains(
            "error: infrastructure error (mock_unavailable): mock transport unavailable\n"
        )
    );
    assert!(!output.contains("evaluation failed"));
    assert!(output.contains("mock[3]: recovered\n"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("auth", 1),
            evaluated("infra", 2),
            evaluated("recovered", 3),
            Event::Close,
        ]
    );
}

#[test]
fn empty_success_emits_no_output_or_failure_and_preserves_session() {
    let (output, events) = shell(":scope session mock\nempty\nafter\n:scope quit\n");
    assert!(output.contains("Active language: mock\nmock> mock> mock[2]: after\n"));
    assert!(!output.contains("evaluation failed"));
    assert!(!output.contains("error:"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("empty", 1),
            evaluated("after", 2),
            Event::Close,
        ]
    );
}

#[test]
fn history_records_verbatim_input_before_routing_in_both_modes() {
    let (_, registry) = setup();
    let mut session = InteractiveSession::new();
    let mut history = CommandHistory::new(10);
    for (input, expected) in [
        ("help", InputRoute::ScopeCommand("help")),
        (
            ":scope session mock",
            InputRoute::ScopeCommand("session mock"),
        ),
    ] {
        assert!(history.record(input));
        assert_eq!(session.route(input), expected);
    }
    session.select(&registry, "mock").unwrap();
    for (input, expected) in [
        ("  source  ", InputRoute::Source("  source  ")),
        ("help", InputRoute::Source("help")),
        (":scope help", InputRoute::ScopeCommand("help")),
        (
            ":scope nonexistent",
            InputRoute::ScopeCommand("nonexistent"),
        ),
        (
            ":scope session none",
            InputRoute::ScopeCommand("session none"),
        ),
    ] {
        assert!(history.record(input));
        assert_eq!(session.route(input), expected);
    }
    session.clear().unwrap();
    assert!(history.record("help"));
    assert_eq!(session.route("help"), InputRoute::ScopeCommand("help"));
    assert_eq!(
        history.entries(),
        [
            "help",
            ":scope session mock",
            "  source  ",
            "help",
            ":scope help",
            ":scope nonexistent",
            ":scope session none",
            "help",
        ]
    );
    assert!(!history.record("help"));
    assert!(!history.record(" \t "));
    assert_eq!(history.previous("draft"), Some("help".into()));
    assert_eq!(history.previous("help"), Some(":scope session none".into()));
    assert_eq!(history.next(), Some("help".into()));
    assert_eq!(history.next(), Some("draft".into()));
}

#[test]
fn switching_distinct_languages_closes_old_and_recreates_on_return() {
    let root = TempWorkspace::new("cli_interactive_switch");
    let first = provider("mock");
    let second = provider("other");
    let registry = LanguageRegistry::empty()
        .with_interactive_provider(Box::leak(Box::new(first)))
        .with_interactive_provider(Box::leak(Box::new(second)));

    // Observe cleanup at each switch, not merely after final shell shutdown.
    let mut session = InteractiveSession::new();
    session.select(&registry, "mock").unwrap();
    session.evaluate("one").unwrap();
    session.evaluate("two").unwrap();
    session.select(&registry, "other").unwrap();
    assert_eq!(first.0.lock().unwrap().last(), Some(&Event::Close));
    assert_eq!(
        session.evaluate("fresh").unwrap().output,
        ["other[1]: fresh"]
    );
    session.select(&registry, "mock").unwrap();
    assert_eq!(second.0.lock().unwrap().last(), Some(&Event::Close));
    assert_eq!(
        session.evaluate("returned").unwrap().output,
        ["mock[1]: returned"]
    );
    session.clear().unwrap();
    first.0.lock().unwrap().clear();
    second.0.lock().unwrap().clear();

    let app = App::builder(root.workspace())
        .language_registry(registry)
        .build()
        .unwrap();
    let mut output = Vec::new();
    run_shell(
        app,
        agent_config(),
        &b":scope session mock\none\ntwo\n:scope session other\nfresh\n:scope session mock\nreturned\n:scope quit\n"[..],
        &mut output,
    )
    .unwrap();
    assert_eq!(
        *first.0.lock().unwrap(),
        [
            Event::Create,
            evaluated("one", 1),
            evaluated("two", 2),
            Event::Close,
            Event::Create,
            evaluated("returned", 1),
            Event::Close,
        ]
    );
    assert_eq!(
        *second.0.lock().unwrap(),
        [Event::Create, evaluated("fresh", 1), Event::Close]
    );
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("Active language: other\nother> other[1]: fresh\n"));
    assert!(output.contains("Active language: mock\nmock> mock[1]: returned\n"));
}

#[test]
fn selection_and_evaluation_leave_capability_metadata_and_workspace_unchanged() {
    let root = TempWorkspace::new("cli_interactive_capabilities");
    root.write_file("sentinel.txt", "unchanged\n");
    let workspace = root.workspace();
    let before_capabilities = workspace.capabilities().to_vec();
    let mut metadata = Vec::new();
    print_capabilities(&workspace, &mut metadata).unwrap();
    let metadata = String::from_utf8(metadata).unwrap();
    assert!(!metadata.is_empty());
    let before_files: Vec<_> = std::fs::read_dir(root.root())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    let (provider, registry) = setup();
    let app = App::builder(workspace.clone())
        .language_registry(registry)
        .build()
        .unwrap();
    let mut output = Vec::new();
    run_shell(
        app,
        agent_config(),
        &b"capabilities\n:scope session mock\n:scope capabilities\nsave sentinel.txt\ncreate unexpected.txt\n:scope capabilities\n:scope session none\ncapabilities\nquit\n"[..],
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    // Compare the exact rendered metadata before selection, after selection,
    // after evaluation, and after clearing the session.
    assert_eq!(output.matches(&metadata).count(), 4);
    assert_eq!(workspace.capabilities(), before_capabilities);
    assert_eq!(
        *provider.0.lock().unwrap(),
        [
            Event::Create,
            evaluated("save sentinel.txt", 1),
            evaluated("create unexpected.txt", 2),
            Event::Close,
        ]
    );
    let after_files: Vec<_> = std::fs::read_dir(root.root())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(after_files, before_files);
    assert_eq!(root.read_file("sentinel.txt"), "unchanged\n");
}

#[test]
fn scratch_capture_is_exact_and_independent_of_session_and_history() {
    let (output, events) = shell(
        ":scope scratch capture\n:scope session mock\n  source  \n:scope help\n:scope session none\n:scope scratch capture\n:scope scratch eval\n:scope scratch new other\n:scope scratch select 0\n:scope session mock\n:scope scratch eval\n:scope scratch list\n:scope scratch discard\n:scope scratch show\n:scope quit\n",
    );
    assert!(output.contains("no REPL submission to capture"));
    assert!(output.contains("scratch 0 captured (mock)"));
    assert!(output.contains("scratch language mock does not match session None"));
    assert!(output.contains("* scratch 0 language=mock dirty version=0 destination=[none]"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("  source  ", 1),
            Event::Close,
            Event::Create,
            evaluated("  source  ", 1),
            Event::Close
        ]
    );
}

#[test]
fn scratch_replace_and_eval_reuse_literal_source_and_failure_rendering() {
    let (output, events) = shell(
        ":scope scratch new mock\n:scope scratch replace 1 1 1 1 fail\n:scope session mock\n:scope scratch eval\n:scope scratch capture\n:scope scratch replace 1 1 1 5 :scope help\\n\\t\\\\  \n:scope scratch eval\n:scope scratch list\n:scope quit\n",
    );
    assert!(output.contains(
        "before diagnostic\nalready newline\nerror 2:3: mock syntax failure\nevaluation failed\n"
    ));
    assert!(output.contains("no REPL submission to capture"));
    assert!(output.contains("version=2"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("fail", 1),
            evaluated(":scope help\n\t\\  ", 2),
            Event::Close
        ]
    );
}

#[test]
fn scratch_commands_reject_bad_arguments_without_mutation_or_dispatch() {
    let mut input = String::from(":scope scratch new mock\n");
    for command in [
        "",
        "unknown",
        "new",
        "new mock extra",
        "select",
        "select nope",
        "select 99",
        "list extra",
        "show extra",
        "capture extra",
        "eval extra",
        "destination",
        "destination a b",
        "save extra",
        "promote",
        "promote a b",
        "discard extra",
        "open",
        "open a b",
        "save-as",
        "save-as a b",
        "close extra",
        "reload extra",
        "reload-discard extra",
        "replace",
        "replace 0 1 1 1 x",
        "replace 1 1 1 1 \\q",
    ] {
        input.push_str(&format!(":scope scratch {command}\n"));
    }
    input.push_str(":scope scratch list\n:scope scratch discard\n:scope scratch show\nquit\n");
    let (output, events) = shell(&input);
    assert!(events.is_empty());
    assert_eq!(output.matches("error:").count(), 28);
    assert!(output.contains("* scratch 0 language=mock dirty version=0 destination=[none]"));
    assert!(output.contains("no scratch buffer selected"));
}

#[test]
fn scratch_persistence_is_explicit_and_existing_destinations_are_required() {
    let root = TempWorkspace::new("cli_scratch_save");
    root.write_file("saved.txt", "baseline");
    root.write_file("promoted.txt", "other baseline");
    let (provider, registry) = setup();
    let app = App::builder(root.workspace())
        .language_registry(registry)
        .build()
        .unwrap();
    let mut output = Vec::new();
    run_shell(app, agent_config(), &b":scope scratch new mock\n:scope scratch replace 1 1 1 1 exact\\n\n:scope scratch save\n:scope scratch destination missing.txt\n:scope scratch destination saved.txt\n:scope scratch save\n:scope scratch list\n:scope scratch promote promoted.txt\n:scope scratch replace 1 1 1 1 unsaved\n:scope scratch discard\nquit\n"[..], &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("scratch buffer has no destination"));
    assert!(output.contains("new files are unsupported"));
    assert!(output.contains("clean version=1 destination=saved.txt"));
    assert_eq!(root.read_file("saved.txt"), "exact\n");
    assert_eq!(root.read_file("promoted.txt"), "exact\n");
    assert!(!root.root().join("missing.txt").exists());
    assert!(provider.0.lock().unwrap().is_empty());
}

#[test]
fn scratch_file_lifecycle_uses_same_collection_and_explicit_discard() {
    let root = TempWorkspace::new("cli_scratch_lifecycle");
    root.write_file("source.txt", "baseline");
    root.write_file("copy.txt", "other");
    let app = App::new(root.workspace()).unwrap();
    let mut output = Vec::new();
    run_shell(app, agent_config(), &b":scope scratch new mock\n:scope scratch open source.txt\n:scope scratch replace 1 1 1 9 changed\n:scope scratch close\n:scope scratch reload\n:scope scratch show\n:scope scratch reload-discard\n:scope scratch show\n:scope scratch replace 1 1 1 9 saved\n:scope scratch save-as missing.txt\n:scope scratch save-as copy.txt\n:scope scratch close\n:scope scratch list\n:scope scratch discard\n:scope scratch show\nquit\n"[..], &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert_eq!(output.matches("error:").count(), 4);
    assert!(output.contains("scratch 1 opened: source.txt"));
    assert!(output.contains("changed"));
    assert!(output.contains("baseline"));
    assert!(output.contains("scratch saved as: copy.txt"));
    assert!(output.contains("scratch closed (no write)"));
    assert!(output.contains("* scratch 0 language=mock dirty version=0 destination=[none]"));
    assert!(output.contains("no scratch buffer selected"));
    assert_eq!(root.read_file("source.txt"), "baseline");
    assert_eq!(root.read_file("copy.txt"), "saved");
    assert!(!root.root().join("missing.txt").exists());
}

#[test]
fn scratch_binding_discard_and_eof_never_write() {
    let root = TempWorkspace::new("cli_scratch_no_write");
    root.write_file("sentinel.txt", "baseline");
    let app = App::new(root.workspace()).unwrap();
    let mut output = Vec::new();
    run_shell(app, agent_config(), &b":scope scratch new mock\n:scope scratch replace 1 1 1 1 changed\n:scope scratch destination sentinel.txt\n:scope scratch discard\n:scope scratch new mock\n:scope scratch destination sentinel.txt\n"[..], &mut output).unwrap();
    assert_eq!(root.read_file("sentinel.txt"), "baseline");
    assert_eq!(
        complete_commands(":scope scr", 10, false).unwrap()[0].insert_text,
        "scratch"
    );
}

#[test]
fn buffer_editor_commits_unicode_without_evaluating_or_resetting_session() {
    let (output, events) = shell(
        ":scope session mock\nbefore\n:scope scratch new mock\n:scope edit\né猫\n🙂x\n:at 1 2\n:delete 2\n:insert Ω\\n\\t\\\\:literal\n:done\n:scope show\n:scope buffers\nafter\n:scope quit\n",
    );
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("before", 1),
            evaluated("after", 2),
            Event::Close
        ]
    );
    assert!(!output.contains("error:"), "{output}");
    assert!(output.contains("    1 | éΩ\n    2 | \\t\\\\:literal🙂x\n    3 | \n"));
    assert!(output.contains("dirty version=1 destination=[none]"));
    assert!(output.contains("Returned to REPL (no save or evaluation)."));
}

#[test]
fn buffer_editor_cancel_preserves_committed_draft_and_selection() {
    let (output, events) = shell(
        ":scope scratch new mock\n:scope edit\n  猫  \n\n:insert :colon\n:done\n:scope scratch new other\n:scope buffer 0\n:scope edit\n:delete 2\n:insert discarded\n:cancel\n:scope show\n:scope buffers\n:scope buffer 99\n:scope show\n:scope discard\n:scope buffers\n:scope discard\n:scope show\nquit\n",
    );
    assert!(events.is_empty());
    assert_eq!(
        output
            .matches("    1 |   猫  \n    2 | \n    3 | :colon\n")
            .count(),
        2
    );
    assert!(output.contains("* scratch 0 language=mock dirty version=1 destination=[none]"));
    assert!(output.contains("error: unknown scratch buffer 99"));
    assert!(output.contains("* scratch 1 language=other dirty version=0 destination=[none]"));
    assert!(output.contains("error: no scratch buffer selected"));
    assert!(!output.contains("| discarded"));
}

#[test]
fn buffer_alias_lifecycle_protects_dirty_edits_and_failed_save_as() {
    let root = TempWorkspace::new("cli_buffer_aliases");
    root.write_file("source.txt", "baseline");
    root.write_file("copy.txt", "other");
    let (provider, registry) = setup();
    let app = App::builder(root.workspace())
        .language_registry(registry)
        .build()
        .unwrap();
    let mut output = Vec::new();
    run_shell(app, agent_config(), &b":scope session mock\nbefore\n:scope open source.txt\n:scope edit\n:delete 8\n:insert changed\n:done\n:scope reload\n:scope close\n:scope save-as missing.txt\n:scope show\n:scope buffers\n:scope save\n:scope reload\n:scope edit\n:delete 7\n:insert copied\n:done\n:scope save-as copy.txt\n:scope buffers\n:scope close\n:scope show\nafter\n:scope quit\n"[..], &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert_eq!(
        output
            .matches("buffer 0 has unsaved changes; save or explicitly discard")
            .count(),
        2
    );
    assert!(output.contains("new files are unsupported"));
    assert!(output.contains("buffer 0 source.txt dirty\n    1 | changed\n"));
    assert!(output.contains("dirty version=1 destination=source.txt"));
    assert!(output.contains("clean version=3 destination=copy.txt"));
    assert!(output.contains("scratch reloaded (no write)"));
    assert!(output.contains("scratch closed (no write)"));
    assert!(output.contains("no scratch buffer selected"));
    assert_eq!(root.read_file("source.txt"), "changed");
    assert_eq!(root.read_file("copy.txt"), "copied");
    assert!(!root.host_path_exists("missing.txt"));
    assert_eq!(
        *provider.0.lock().unwrap(),
        vec![
            Event::Create,
            evaluated("before", 1),
            evaluated("after", 2),
            Event::Close
        ]
    );
}

#[test]
fn buffer_alias_readonly_save_denial_preserves_edits_and_destination() {
    let root = TempWorkspace::new("cli_buffer_readonly");
    root.write_file("source.txt", "baseline");
    root.write_file("copy.txt", "other");
    let app =
        App::new(crate::workspace::Workspace::read_only(root.root().to_string_lossy()).unwrap())
            .unwrap();
    let mut output = Vec::new();
    run_shell(app, agent_config(), &b":scope open source.txt\n:scope edit\n:delete 8\n:insert changed\n:done\n:scope save\n:scope save-as copy.txt\n:scope buffers\n:scope show\n:scope close\n:scope discard\nquit\n"[..], &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert_eq!(output.matches("error:").count(), 3, "{output}");
    assert_eq!(output.matches("PermissionDenied").count(), 2, "{output}");
    assert!(output.contains("dirty version=1 destination=source.txt"));
    assert!(output.contains("buffer 0 source.txt dirty\n    1 | changed\n"));
    assert_eq!(root.read_file("source.txt"), "baseline");
    assert_eq!(root.read_file("copy.txt"), "other");
}

#[test]
fn buffer_eval_uses_unsaved_editor_source_and_shares_repl_session() {
    let (output, events) = shell(
        ":scope session mock\nbefore\n:scope scratch new mock\n:scope edit\n  猫  \n:insert :scope help\n:done\n:scope buffer eval\n:scope show\n:scope buffers\nafter\n:scope quit\n",
    );
    assert!(!output.contains("error:"), "{output}");
    assert!(
        output.contains("mock[2]:   猫  \n:scope help\n"),
        "{output}"
    );
    assert!(output.contains("    1 |   猫  \n    2 | :scope help\n"));
    assert!(output.contains("dirty version=1 destination=[none]"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("before", 1),
            evaluated("  猫  \n:scope help", 2),
            evaluated("after", 3),
            Event::Close
        ]
    );
}

#[test]
fn buffer_eval_open_file_edit_and_save_workflow() {
    let root = TempWorkspace::new("buffer_eval_open");
    root.write_file("source.scope", "baseline");
    let provider = provider("scope.simple");
    let registry =
        LanguageRegistry::builtins().with_interactive_provider(Box::leak(Box::new(provider)));
    let app = App::builder(root.workspace())
        .language_registry(registry)
        .build()
        .unwrap();
    let mut output = Vec::new();
    run_shell(
        app,
        agent_config(),
        &b":scope open source.scope\n:scope session scope.simple\n:scope edit\n:delete 8\n:insert unsaved\n:done\n:scope buffer eval\n:scope buffers\nrepl\n:scope save\n:scope buffers\n:scope quit\n"[..],
        &mut output,
    ).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(!output.contains("error:"), "{output}");
    assert!(output.contains("dirty version=1 destination=source.scope"));
    assert!(output.contains("clean version=1 destination=source.scope"));
    assert_eq!(root.read_file("source.scope"), "unsaved");
    assert_eq!(
        *provider.0.lock().unwrap(),
        vec![
            Event::Create,
            evaluated("unsaved", 1),
            evaluated("repl", 2),
            Event::Close,
        ]
    );
}

#[test]
fn buffer_eval_exposes_definition_metadata_without_inspection_evaluation() {
    let (output, events) = shell(
        ":scope session mock\n:scope scratch new mock\n:scope scratch replace 1 1 1 1 define answer\n:scope describe answer\n:scope buffer eval\n:scope describe answer\n:scope effect answer\n:scope where answer\n:scope buffers\nafter\n:scope quit\n",
    );
    assert!(!output.contains("error:"), "{output}");
    assert!(output.contains("unknown symbol: answer\n"));
    assert_eq!(
        output
            .lines()
            .filter(|line| *line == "mock> symbol: answer")
            .count(),
        3,
        "{output}"
    );
    assert!(output.contains("documentation: mock evaluated definition\n"));
    assert!(output.contains("signature: ( -- value )\n"));
    assert!(output.contains("definition: define answer\n"));
    assert!(output.contains("dirty version=1 destination=[none]"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("define answer", 1),
            evaluated("after", 2),
            Event::Close
        ]
    );
}

#[test]
fn buffer_eval_reset_preserves_source_and_numeric_buffer_selection() {
    let (output, events) = shell(
        ":scope session mock\n:scope scratch new mock\n:scope scratch replace 1 1 1 1 first\n:scope scratch new mock\n:scope scratch replace 1 1 1 1 second\n:scope buffer 0\n:scope buffer eval\n:scope session-reset\n:scope buffer eval\n:scope show\n:scope buffers\n:scope buffer 1\n:scope buffer eval\n:scope quit\n",
    );
    assert!(!output.contains("error:"), "{output}");
    assert!(output.contains("session reset\n"));
    assert!(output.contains("    1 | first\n"));
    assert!(output.contains("* scratch 0 language=mock dirty version=1 destination=[none]"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("first", 1),
            Event::Reset,
            evaluated("first", 1),
            evaluated("second", 2),
            Event::Close
        ]
    );
}

#[test]
fn buffer_eval_rejects_missing_buffer_inactive_session_and_language_mismatch() {
    let (output, events) = shell(
        ":scope buffer eval\n:scope scratch new mock\n:scope scratch replace 1 1 1 1 retained\n:scope buffer eval\n:scope session mock\n:scope scratch new other\n:scope buffer eval\n:scope buffer 0\n:scope session missing\n:scope buffer eval\n:scope buffers\n:scope show\n:scope quit\n",
    );
    assert!(output.contains("no scratch buffer selected"), "{output}");
    assert!(
        output.contains("no interactive language selected"),
        "{output}"
    );
    assert!(
        output.contains("scratch language other does not match session"),
        "{output}"
    );
    assert!(output.contains("unsupported language: missing"), "{output}");
    assert!(output.contains("* scratch 0 language=mock dirty version=1 destination=[none]"));
    assert!(output.contains("    1 | retained\n"));
    assert_eq!(
        events,
        vec![Event::Create, evaluated("retained", 1), Event::Close]
    );
}

#[test]
fn buffer_eval_requires_namespace_and_rejects_extra_arguments_without_dispatch() {
    let (output, events) = shell(
        ":scope scratch new mock\n:scope scratch replace 1 1 1 1 retained\nbuffer eval\n:scope session mock\n:scope buffer eval extra\nbuffer eval\n:scope buffer eval\n:scope buffers\n:scope quit\n",
    );
    assert!(
        output.contains("error: unknown command: buffer"),
        "{output}"
    );
    assert_eq!(output.matches("error:").count(), 2, "{output}");
    assert!(output.contains("dirty version=1 destination=[none]"));
    assert_eq!(
        events,
        vec![
            Event::Create,
            evaluated("buffer eval", 1),
            evaluated("retained", 2),
            Event::Close
        ]
    );
}

#[test]
fn buffer_eval_failures_preserve_edits_and_session_mutations_without_writes() {
    for (source, message) in [
        (
            "fail",
            "before diagnostic\nalready newline\nerror 2:3: mock syntax failure\nevaluation failed\n",
        ),
        (
            "auth",
            "error: authorization error (mock_denied): mock permission denied\n",
        ),
        (
            "infra",
            "error: infrastructure error (mock_unavailable): mock transport unavailable\n",
        ),
    ] {
        let root = TempWorkspace::new("cli_buffer_eval_failure");
        root.write_file("destination.txt", "baseline");
        let (provider, registry) = setup();
        let app = App::builder(root.workspace())
            .language_registry(registry)
            .build()
            .unwrap();
        let input = format!(
            ":scope session mock\n:scope scratch new mock\n:scope scratch destination destination.txt\n:scope edit\n:insert {source}\n:done\n:scope buffer eval\n:scope show\n:scope buffers\n:scope buffer eval\nrecovered\n:scope quit\n"
        );
        let mut output = Vec::new();
        run_shell(app, agent_config(), input.as_bytes(), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches(message).count(), 2, "{output}");
        assert!(output.contains(&format!("    1 | {source}\n")), "{output}");
        assert!(output.contains("dirty version=1 destination=destination.txt"));
        assert_eq!(root.read_file("destination.txt"), "baseline");
        assert_eq!(
            *provider.0.lock().unwrap(),
            vec![
                Event::Create,
                evaluated(source, 1),
                evaluated(source, 2),
                evaluated("recovered", 3),
                Event::Close
            ]
        );
    }
}

#[test]
fn buffer_eval_and_save_are_independent_and_eval_does_not_mark_clean() {
    let root = TempWorkspace::new("cli_buffer_eval_save");
    root.write_file("destination.txt", "baseline");
    let (provider, registry) = setup();
    let app = App::builder(root.workspace())
        .language_registry(registry)
        .build()
        .unwrap();
    let mut output = Vec::new();
    run_shell(app, agent_config(), &b":scope scratch new mock\n:scope scratch destination destination.txt\n:scope edit\n:insert saved\n:done\n:scope save\n:scope session mock\n:scope buffer eval\n:scope buffers\n:scope edit\n:insert unsaved\n:done\n:scope buffer eval\n:scope buffers\n:scope quit\n"[..], &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(!output.contains("error:"), "{output}");
    assert!(output.contains("clean version=1 destination=destination.txt"));
    assert!(output.contains("dirty version=2 destination=destination.txt"));
    assert_eq!(root.read_file("destination.txt"), "saved");
    assert_eq!(
        *provider.0.lock().unwrap(),
        vec![
            Event::Create,
            evaluated("saved", 1),
            evaluated("unsavedsaved", 2),
            Event::Close
        ]
    );
}

#[test]
fn buffer_alias_save_conflict_is_presented_and_reload_discard_recovers() {
    struct ChangingReader<'a> {
        input: std::io::Cursor<&'a [u8]>,
        root: &'a TempWorkspace,
        changed: bool,
    }
    impl std::io::Read for ChangingReader<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.input, bytes)
        }
    }
    impl std::io::BufRead for ChangingReader<'_> {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            std::io::BufRead::fill_buf(&mut self.input)
        }
        fn consume(&mut self, amount: usize) {
            std::io::BufRead::consume(&mut self.input, amount);
        }
        fn read_line(&mut self, line: &mut String) -> std::io::Result<usize> {
            let read = std::io::BufRead::read_line(&mut self.input, line)?;
            if !self.changed && line == ":scope save\n" {
                self.root.write_file("source.txt", "external");
                self.changed = true;
            }
            Ok(read)
        }
    }
    let root = TempWorkspace::new("cli_buffer_conflict");
    root.write_file("source.txt", "baseline");
    let input = ChangingReader {
        input: std::io::Cursor::new(&b":scope open source.txt\n:scope edit\n:delete 8\n:insert changed\n:done\n:scope save\n:scope show\n:scope reload\n:scope reload-discard\n:scope show\n:scope close\nquit\n"[..]),
        root: &root,
        changed: false,
    };
    let mut output = Vec::new();
    run_shell(
        App::new(root.workspace()).unwrap(),
        agent_config(),
        input,
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("conflict"), "{output}");
    assert!(output.contains("buffer 0 source.txt dirty\n    1 | changed\n"));
    assert!(output.contains("buffer 0 has unsaved changes"));
    assert!(output.contains("buffer 0 source.txt clean\n    1 | external\n"));
    assert!(output.contains("scratch closed (no write)"));
    assert_eq!(root.read_file("source.txt"), "external");
}
