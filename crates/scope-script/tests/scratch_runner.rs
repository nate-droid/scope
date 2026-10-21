#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod language;
use scope_ide::{
    app::App,
    cli,
    document::{Document, DocumentId},
    language::InteractiveSession,
    scratch::ScratchBuffers,
    text::{Position, TextEdit, TextRange},
    workspace::Workspace,
};
use scope_script::{ErrorKind, Value, runner::ScriptRunner};
use std::sync::atomic::Ordering;

const ACTION: &str = "language.example.declarations.describe_selection";
const CALL: &str =
    "scope.invoke(\"semantic\", \"language.example.declarations.describe_selection\", {})";
fn run_command() -> String {
    format!(":scope run {ACTION} --selection 0:0..0:15\n")
}
fn scratch(text: &str) -> String {
    format!(":scope scratch new scope-script\n:scope scratch replace 1 1 1 1 {text}\n")
}
fn shell(runner: &mut ScriptRunner<'_>, commands: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("example.decl"), language::SOURCE).unwrap();
    let mut output = vec![];
    cli::run_with_session_and_command(
        App::new(Workspace::read_only(dir.path().to_string_lossy()).unwrap()).unwrap(),
        InteractiveSession::new(),
        runner,
        commands.as_bytes(),
        &mut output,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("example.decl")).unwrap(),
        language::SOURCE
    );
    String::from_utf8(output).unwrap()
}
#[test]
fn authoring_completion_is_metadata_only_and_does_not_invent_bindings() {
    use scope_ide::cli::ReadOnlyCommand;
    let service = language::DeclarationLanguage::default();
    let runner = ScriptRunner::new(&service, "describe_selection", None);
    let mut buffers = ScratchBuffers::default();
    buffers.create("scope-script", "");
    let source = buffers.selected().unwrap();
    let complete = |draft: &str| runner.complete_source(source, draft, draft.len());
    let names = complete("scope.");
    assert!(
        names
            .iter()
            .any(|item| item.insert_text == "scope.operations")
    );
    assert!(names.iter().any(|item| item.insert_text == "scope.invoke"));
    assert!(
        complete("let invented = scope.invoke(\"semantic\", \"bad\", {}); invented").is_empty()
    );
    assert_eq!(
        complete("scope.invoke(\"sem")[0].insert_text,
        "\"semantic\""
    );
    assert_eq!(
        complete("scope.invoke(\"semantic\", \"language.")[0].insert_text,
        format!("\"{ACTION}\"")
    );
    assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    assert!(runner.records.is_empty());
}

#[test]
fn unicode_error_report_uses_one_based_scalar_columns_and_keeps_prints() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", None);
    let program = "print(\"é\"); missing";
    let output = shell(
        &mut runner,
        &format!(
            "open example.decl\n{}{}quit\n",
            scratch(program),
            run_command()
        ),
    );
    let column = program[..program.find("missing").unwrap()].chars().count() + 1;
    assert!(
        output.contains(&format!("1:{column} (Unicode scalars; byte offset")),
        "{output}"
    );
    assert!(output.contains("status: interpreter failure"), "{output}");
    assert!(output.find("print:").unwrap() < output.find("diagnostic:").unwrap());
}

#[test]
fn cooked_multiline_edit_runs_twice_then_reruns_edited_source() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let program = format!("let result = {CALL};\nprint(result);\nresult.value.name\n");
    let commands = format!(
        "open example.decl\n:scope scratch new scope-script\n:scope edit\n{program}:done\n{}{}:scope edit\n:at 3 1\n:delete 17\n:insert result.value.kind\n:done\n{}show\ndirty\nquit\n",
        run_command(),
        run_command(),
        run_command(),
    );
    let output = shell(&mut runner, &commands);
    assert!(!output.contains("error:"), "{output}");
    assert_eq!(runner.records.len(), 3, "{output}");
    assert_eq!(service.calls.load(Ordering::SeqCst), 3);
    let first = &runner.records[0];
    let second = &runner.records[1];
    let edited = &runner.records[2];
    assert_eq!(first.source.text, program);
    assert_eq!(second.source.text, program);
    assert_eq!(first.source.identity, second.source.identity);
    assert_eq!(first.source.revision, second.source.revision);
    assert_eq!(first.source.identity, edited.source.identity);
    assert_ne!(first.source.revision, edited.source.revision);
    assert_eq!(
        edited.source.text,
        program.replace("result.value.name", "result.value.kind")
    );
    for record in [first, second] {
        assert_eq!(
            record.outcome.as_ref().unwrap(),
            &Value::String("answer".into())
        );
    }
    assert_eq!(
        edited.outcome.as_ref().unwrap(),
        &Value::String("integer_literal_binding".into())
    );
    for record in &runner.records {
        assert_eq!(record.results.len(), 1);
        assert_eq!(record.printed, record.results);
        assert_eq!(record.target.text, language::SOURCE);
        assert_eq!(record.target.identity, first.target.identity);
        assert_eq!(record.target.revision, first.target.revision);
    }
    assert_eq!(output.matches("final: \"answer\"").count(), 2, "{output}");
    assert!(
        output.contains("final: \"integer_literal_binding\""),
        "{output}"
    );
}

#[test]
fn ss2_cooked_newline_statements_preserve_semantic_result() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let program = format!("let result = {CALL}\nprint(result)\nresult.value.kind\n");
    let output = shell(
        &mut runner,
        &format!(
            "open example.decl\n:scope scratch new scope-script\n:scope edit\n{program}:done\n{}quit\n",
            run_command(),
        ),
    );
    assert_eq!(runner.records.len(), 1, "{output}");
    let record = &runner.records[0];
    assert_eq!(record.source.text, program);
    assert_eq!(
        record.outcome.as_ref().unwrap(),
        &Value::String("integer_literal_binding".into())
    );
    assert_eq!(record.printed, record.results);
    assert_eq!(record.results.len(), 1);
    assert_eq!(service.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn ss3_cooked_multiline_example_preserves_envelope_and_provenance() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let program = include_str!("../examples/conditional-declaration.scope");
    let output = shell(
        &mut runner,
        &format!(
            "open example.decl\n:scope scratch new scope-script\n:scope edit\n{program}:done\n{}{}quit\n",
            run_command(),
            run_command(),
        ),
    );
    assert_eq!(runner.records.len(), 2, "{output}");
    assert_eq!(service.calls.load(Ordering::SeqCst), 2);
    for record in &runner.records {
        assert_eq!(record.source.text, program);
        assert_eq!(
            &record.printed[..2],
            [Value::String("answer".into()), Value::String("42".into())]
        );
        assert_eq!(record.printed.len(), 3);
        let Value::Object(report) = &record.printed[2] else {
            panic!("expected structured report")
        };
        assert_eq!(report["evaluation"], record.results[0]);
        assert_eq!(record.results, [record.outcome.as_ref().unwrap().clone()]);
        assert_eq!(record.target.text, language::SOURCE);
        for field in ["partial", "unsupported", "rationale", "unknown"] {
            assert!(format!("{:?}", record.results).contains(field));
        }
        assert!(format!("{:?}", record.catalog).contains("constraints"));
    }
    assert_eq!(
        runner.records[0].source.identity,
        runner.records[1].source.identity
    );
    assert_eq!(
        runner.records[0].source.revision,
        runner.records[1].source.revision
    );
    assert_eq!(
        runner.records[0].target.revision,
        runner.records[1].target.revision
    );
    assert_eq!(output.matches("print: \"answer\"").count(), 2, "{output}");
    assert_eq!(output.matches("print: \"42\"").count(), 2, "{output}");
}

#[test]
fn cooked_open_saved_script_executes_unsaved_editor_changes_without_saving() {
    let dir = tempfile::tempdir().unwrap();
    let program = format!("let result = {CALL};\nprint(result);\nresult.value.name\n");
    let script_path = dir.path().join("inspect.scope");
    let target_path = dir.path().join("example.decl");
    std::fs::write(&script_path, &program).unwrap();
    std::fs::write(&target_path, language::SOURCE).unwrap();
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let commands = format!(
        "open example.decl\n:scope open inspect.scope\n{}:scope edit\n:at 3 1\n:delete 17\n:insert result.value.literal\n:done\n{}quit\n",
        run_command(),
        run_command(),
    );
    let mut output = vec![];
    cli::run_with_session_and_command(
        App::new(Workspace::read_only(dir.path().to_string_lossy()).unwrap()).unwrap(),
        InteractiveSession::new(),
        &mut runner,
        commands.as_bytes(),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(!output.contains("error:"), "{output}");
    assert_eq!(runner.records.len(), 2, "{output}");
    assert_eq!(service.calls.load(Ordering::SeqCst), 2);
    let saved = &runner.records[0];
    let unsaved = &runner.records[1];
    assert_eq!(saved.source.text, program);
    assert_eq!(
        unsaved.source.text,
        program.replace("result.value.name", "result.value.literal")
    );
    assert_eq!(saved.source.identity, unsaved.source.identity);
    assert_ne!(saved.source.revision, unsaved.source.revision);
    assert_eq!(
        saved.outcome.as_ref().unwrap(),
        &Value::String("answer".into())
    );
    assert_eq!(
        unsaved.outcome.as_ref().unwrap(),
        &Value::String("42".into())
    );
    for record in &runner.records {
        assert_eq!(record.results.len(), 1);
        assert_eq!(record.printed, record.results);
        assert_eq!(record.target.text, language::SOURCE);
        assert_eq!(record.target.identity, saved.target.identity);
        assert_eq!(record.target.revision, saved.target.revision);
    }
    assert!(output.contains("final: \"42\""), "{output}");
    assert_eq!(std::fs::read_to_string(script_path).unwrap(), program);
    assert_eq!(
        std::fs::read_to_string(target_path).unwrap(),
        language::SOURCE
    );
}

#[test]
fn shell_repeated_runs_edit_and_failure_keep_owned_records() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let program = format!("let x = {CALL}; print(\"é\"); 7");
    let commands = format!(
        "open example.decl\n{}{}{}:scope scratch replace 1 {} 1 {} ; missing\n{}show\ndirty\nquit\n",
        scratch(&program),
        run_command(),
        run_command(),
        program.len() + 1,
        program.len() + 1,
        run_command()
    );
    let output = shell(&mut runner, &commands);
    assert_eq!(runner.records.len(), 3, "{output}");
    assert_eq!(service.calls.load(Ordering::SeqCst), 3);
    let first = &runner.records[0];
    assert_eq!(first.outcome.as_ref().unwrap(), &Value::Integer(7));
    assert_eq!(first.printed, [Value::String("é".into())]);
    assert_eq!(first.results.len(), 1);
    let envelope = format!("{:?}", first.results);
    for field in ["partial", "unsupported", "rationale", "unknown"] {
        assert!(envelope.contains(field), "{envelope}");
    }
    assert!(format!("{:?}", first.catalog).contains("constraints"));
    assert_eq!(first.source.identity, runner.records[2].source.identity);
    assert_ne!(first.source.revision, runner.records[2].source.revision);
    assert_eq!(first.source.text, program);
    assert_eq!(first.target.text, language::SOURCE);
    let failed = &runner.records[2];
    let error = failed.outcome.as_ref().unwrap_err();
    assert_eq!(error.offset, failed.source.text.find("missing").unwrap());
    assert!(error.offset > failed.source.text[..error.offset].chars().count());
    assert!(matches!(error.kind, ErrorKind::Runtime(_)));
    assert_eq!(failed.results.len(), 1);
    assert_eq!(failed.printed, first.printed);
    assert!(output.contains("final: 7"), "{output}");
    assert!(output.contains("at byte"), "{output}");
}
#[test]
fn shell_default_deny_discovery_and_syntax_failure_do_not_dispatch() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", None);
    let commands = format!(
        "open example.decl\n{}{}{}{}{}{}quit\n",
        scratch("scope.operations()"),
        run_command(),
        scratch(CALL),
        run_command(),
        scratch(&format!("{CALL}; [")),
        run_command()
    );
    let output = shell(&mut runner, &commands);
    assert_eq!(runner.records.len(), 3, "{output}");
    assert!(runner.records[0].outcome.is_ok());
    assert!(matches!(
        runner.records[1].outcome.as_ref().unwrap_err().kind,
        ErrorKind::Tool(_)
    ));
    assert!(matches!(
        runner.records[2].outcome.as_ref().unwrap_err().kind,
        ErrorKind::Syntax(_)
    ));
    assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    assert!(runner.records.iter().all(|r| r.results.is_empty()));
}
#[test]
fn shell_missing_inputs_unknown_name_and_fresh_bindings() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let commands = format!(
        "{}open example.decl\n{}{}:scope run unknown --selection 0:0..0:15\n{}{}{}{}quit\n",
        run_command(),
        run_command(),
        scratch("let x = 5; x + 0"),
        run_command(),
        scratch("unknown_binding"),
        run_command(),
        run_command()
    );
    let output = shell(&mut runner, &commands);
    assert_eq!(runner.records.len(), 3, "{output}");
    assert_eq!(
        runner.records[0].outcome.as_ref().unwrap(),
        &Value::Integer(5)
    );
    assert!(runner.records[1..].iter().all(|r| r.outcome.is_err()));
    assert!(output.contains("unavailable script operation"), "{output}");
    assert_eq!(service.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn direct_runner_uses_unsaved_target_and_target_range_not_script_range() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let mut document = Document::new(DocumentId::new(91), "example.decl", language::SOURCE);
    document
        .apply_edit(TextEdit::replace(
            TextRange::new(Position::new(0, 4), Position::new(0, 10)),
            "renamed",
        ))
        .unwrap();
    let before = document.clone();
    let mut buffers = ScratchBuffers::default();
    buffers.create("scope-script", CALL);
    let range = TextRange::new(Position::new(0, 0), Position::new(0, 16));
    let record = runner
        .run(buffers.selected().unwrap(), &document, range, ACTION)
        .unwrap();
    assert!(record.outcome.is_ok());
    assert!(format!("{:?}", record.results).contains("renamed"));
    assert_eq!(document, before);
    buffers.create("scope-script", "1");
    assert!(
        runner
            .run(buffers.selected().unwrap(), &document, range, ACTION)
            .unwrap()
            .outcome
            .is_ok()
    );
    assert_eq!(document, before);
}

#[test]
fn shell_preserves_runtime_ownership_and_observes_target_edits() {
    use scope_language::{EvaluationResult, InteractiveLanguageSession, SessionError};
    use std::{cell::RefCell, rc::Rc};
    struct Runtime(Rc<RefCell<Vec<&'static str>>>);
    impl InteractiveLanguageSession for Runtime {
        fn language_id(&self) -> &str {
            "runtime"
        }
        fn evaluate(&mut self, _: &str) -> Result<EvaluationResult, SessionError> {
            panic!("script commands must never evaluate language source")
        }
        fn reset(&mut self) -> Result<(), SessionError> {
            self.0.borrow_mut().push("reset");
            Ok(())
        }
        fn close(&mut self) -> Result<(), SessionError> {
            self.0.borrow_mut().push("close");
            Ok(())
        }
    }
    impl Drop for Runtime {
        fn drop(&mut self) {
            self.0.borrow_mut().push("drop");
        }
    }
    let events = Rc::new(RefCell::new(vec![]));
    let mut session = InteractiveSession::new();
    session
        .install("runtime", Box::new(Runtime(events.clone())))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("example.decl"), language::SOURCE).unwrap();
    let mut app = App::new(Workspace::read_only(dir.path().to_string_lossy()).unwrap()).unwrap();
    app.open_path("example.decl").unwrap();
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let commands = format!(
        "{}{}:scope replace 1 5 1 11 amount\n{}:scope session\n:scope quit\n",
        scratch(CALL),
        run_command(),
        run_command()
    );
    let mut output = vec![];
    cli::run_with_session_and_command(app, session, &mut runner, commands.as_bytes(), &mut output)
        .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert_eq!(runner.records.len(), 2, "{output}");
    assert!(runner.records.iter().all(|r| r.outcome.is_ok()), "{output}");
    assert_eq!(runner.records[0].target.text, language::SOURCE);
    assert!(runner.records[1].target.text.starts_with("let amount = 42"));
    assert_eq!(
        runner.records[0].target.identity,
        runner.records[1].target.identity
    );
    assert_ne!(
        runner.records[0].target.revision,
        runner.records[1].target.revision
    );
    assert!(format!("{:?}", runner.records[1].results).contains("amount"));
    assert_eq!(*events.borrow(), ["close", "drop"]);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("example.decl")).unwrap(),
        language::SOURCE
    );
}

#[test]
fn scratch_callback_uses_live_target_and_fresh_revisions() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let mut scripts = ScratchBuffers::default();
    scripts.create("scope-script", CALL);
    let source = scripts.selected().unwrap();
    let mut targets = ScratchBuffers::default();
    targets.create("example.declarations", "let answer = 42");
    let mut output = Vec::new();
    runner
        .invoke_script_with_target(source, Some(targets.selected().unwrap()), &mut output)
        .unwrap();
    assert!(runner.records[0].outcome.is_ok());
    let revision = runner.records[0].target.revision.clone();
    targets
        .selected_mut()
        .unwrap()
        .apply_edit(TextEdit::replace(
            TextRange::new(Position::new(0, 4), Position::new(0, 10)),
            "renamed",
        ))
        .unwrap();
    runner
        .invoke_script_with_target(source, Some(targets.selected().unwrap()), &mut output)
        .unwrap();
    assert!(runner.records[1].outcome.is_ok());
    assert_ne!(runner.records[1].target.revision, revision);
    assert_eq!(runner.records[0].target.text, "let answer = 42");
    assert_eq!(runner.records[1].target.text, "let renamed = 42");
    assert!(format!("{:?}", runner.records[1].results).contains("renamed"));
    assert_eq!(service.calls.load(Ordering::SeqCst), 2);
    assert!(
        runner
            .invoke_script_with_target(source, None, &mut output)
            .is_err()
    );
    assert_eq!(runner.records.len(), 2);
}

#[test]
fn scratch_callback_denies_without_authority_and_rejects_wrong_language() {
    let service = language::DeclarationLanguage::default();
    let mut scripts = ScratchBuffers::default();
    scripts.create("scope-script", CALL);
    let mut targets = ScratchBuffers::default();
    targets.create("example.declarations", "let answer = 42");
    for authorization in [None, Some(false)] {
        let mut runner = ScriptRunner::new(&service, "describe_selection", authorization);
        runner
            .invoke_script_with_target(
                scripts.selected().unwrap(),
                Some(targets.selected().unwrap()),
                &mut Vec::new(),
            )
            .unwrap();
        assert!(runner.records[0].outcome.is_err());
    }
    targets.create("wrong-language", "let answer = 42");
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    assert!(
        runner
            .invoke_script_with_target(
                scripts.selected().unwrap(),
                Some(targets.selected().unwrap()),
                &mut Vec::new()
            )
            .is_err()
    );
    assert!(runner.records.is_empty());
    assert_eq!(service.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn semantic_target_shell_switches_active_source_without_rebinding_target() {
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", Some(true));
    let commands = format!(
        ":scope scratch new example.declarations\n\
         :scope scratch replace 1 1 1 1 let answer = 42\n\
         :scope semantic-target 0\n\
         :scope scratch new scope-script\n\
         :scope scratch replace 1 1 1 1 {CALL}.value.name\n\
         :scope script run\n\
         :scope scratch select 0\n\
         :scope scratch replace 1 5 1 11 renamed\n\
         :scope scratch select 1\n\
         :scope script run\n\
         :scope semantic-target none\n\
         :scope script run\nquit\n"
    );
    let output = shell(&mut runner, &commands);
    assert!(output.contains("final: \"answer\""), "{output}");
    assert!(output.contains("final: \"renamed\""), "{output}");
    assert!(output.contains("unsupported"), "{output}");
    assert_eq!(runner.records.len(), 2);
    assert_ne!(
        runner.records[0].target.revision,
        runner.records[1].target.revision
    );
    assert_eq!(
        runner.records[0].target.identity,
        runner.records[1].target.identity
    );
    assert_eq!(service.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn semantic_target_shell_denial_and_wrong_language_do_not_dispatch() {
    for authorization in [None, Some(false), Some(true)] {
        let service = language::DeclarationLanguage::default();
        let mut runner = ScriptRunner::new(&service, "describe_selection", authorization);
        let language = if authorization == Some(true) {
            "wrong-language"
        } else {
            "example.declarations"
        };
        let output = shell(
            &mut runner,
            &format!(
                ":scope scratch new {language}\n\
             :scope scratch replace 1 1 1 1 let answer = 42\n\
             :scope semantic-target 0\n{}:scope script run\nquit\n",
                scratch(CALL)
            ),
        );
        assert_eq!(service.calls.load(Ordering::SeqCst), 0, "{output}");
        if authorization != Some(true) {
            assert_eq!(runner.records.len(), 1, "{output}");
            assert!(runner.records[0].outcome.is_err());
        } else {
            assert!(runner.records.is_empty());
        }
    }
}

#[test]
fn scratch_semantic_preconditions_reject_stale_revision_and_text_before_dispatch() {
    use scope_ide::{
        source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
        tool::{AgentSession, semantic::SemanticToolProvider},
    };
    use scope_script::{Interpreter, semantic::SemanticHost};

    let service = language::DeclarationLanguage::default();
    let mut buffers = ScratchBuffers::default();
    buffers.create("example.declarations", "let answer = 42");
    let old_text = buffers.selected().unwrap().text().to_owned();
    let old_version = buffers.selected().unwrap().version();
    buffers
        .selected_mut()
        .unwrap()
        .apply_edit(TextEdit::replace(
            TextRange::new(Position::new(0, 4), Position::new(0, 10)),
            "renamed",
        ))
        .unwrap();
    let target = buffers.selected().unwrap();
    // Isolate revision and text preconditions; neither may reach the language service.
    for (text, version) in [
        (target.text(), old_version),
        (old_text.as_str(), target.version()),
    ] {
        let mut controller = SourceEditController::default();
        let provider = SemanticToolProvider::new(
            &service,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(TextRange::new(
                Position::new(0, 0),
                Position::new(0, target.text().len()),
            )),
            |_| true,
        )
        .unwrap();
        let session = AgentSession::builder().provider(provider).build().unwrap();
        let mut host = SemanticHost::new(session, ACTION, text, version, Some(true));
        let result = Interpreter::new().eval(CALL, &mut host).unwrap();
        let Value::Object(fields) = result else {
            panic!("expected semantic envelope");
        };
        assert_eq!(fields["status"].as_str(), Some("revision_conflict"));
    }
    assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    assert_eq!(target.text(), "let renamed = 42");
}

#[test]
fn opened_scratch_target_uses_unsaved_edits_without_changing_persisted_file() {
    use scope_ide::language::LanguageRegistry;
    use std::sync::LazyLock;
    // Registry services must live for 'static; keep this test's service isolated.
    static SERVICE: LazyLock<language::DeclarationLanguage> = LazyLock::new(Default::default);
    let service = &*SERVICE;
    let registry = LanguageRegistry::builtins().with_language(service);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("example.decl");
    let persisted = "let answer = 42";
    std::fs::write(&path, persisted).unwrap();
    let app = App::builder(Workspace::read_only(dir.path().to_string_lossy()).unwrap())
        .language_registry(registry)
        .build()
        .unwrap();
    let mut runner = ScriptRunner::new(service, "describe_selection", Some(true));
    let commands = format!(
        ":scope open example.decl\n\
         :scope scratch replace 1 5 1 11 unsaved\n\
         :scope semantic-target 0\n\
         :scope scratch new scope-script\n\
         :scope scratch replace 1 1 1 1 {CALL}.value.name\n\
         :scope script run\n\
         :scope scratch select 0\n\
         :scope scratch replace 1 5 1 12 changed\n\
         :scope scratch select 1\n\
         :scope script run\nquit\n"
    );
    let mut output = Vec::new();
    cli::run_with_session_and_command(
        app,
        InteractiveSession::new(),
        &mut runner,
        commands.as_bytes(),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("final: \"unsaved\""), "{output}");
    assert!(output.contains("final: \"changed\""), "{output}");
    assert_eq!(runner.records.len(), 2, "{output}");
    let first = &runner.records[0];
    let second = &runner.records[1];
    assert!(first.outcome.is_ok() && second.outcome.is_ok());
    assert_eq!(first.source.identity, second.source.identity);
    assert_eq!(first.source.revision, second.source.revision);
    assert_eq!(first.target.identity, second.target.identity);
    assert_ne!(first.source.identity, first.target.identity);
    assert_ne!(first.target.revision, second.target.revision);
    assert_eq!(first.target.text, "let unsaved = 42");
    assert_eq!(second.target.text, "let changed = 42");
    assert_eq!(service.calls.load(Ordering::SeqCst), 2);
    assert_eq!(std::fs::read(&path).unwrap(), persisted.as_bytes());
}
