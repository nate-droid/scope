use scope_ide::{
    app::App,
    cli::{self, ReadOnlyCommand},
    language::InteractiveSession,
    workspace::Workspace,
};
use scope_script::{Value, runner::ScriptRunner, workbench::WorkbenchRunner};

#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
#[allow(dead_code)]
mod language;

fn shell(host: &mut dyn ReadOnlyCommand, input: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut output = Vec::new();
    cli::run_with_session_and_command(
        App::new(Workspace::read_only(dir.path().to_string_lossy()).unwrap()).unwrap(),
        InteractiveSession::new(),
        host,
        input.as_bytes(),
        &mut output,
    )
    .unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn reduction_hooks_preserve_snapshot_and_byte_ranges_without_execution() {
    use scope_ide::text::TextBuffer;
    use scope_language::{RevisionId, SourceId, SourceSnapshot};
    let service = language::DeclarationLanguage::default();
    let semantic = ScriptRunner::new(&service, "describe_selection", None);
    let workbench = WorkbenchRunner::default();
    for host in [&semantic as &dyn ReadOnlyCommand, &workbench] {
        for text in [
            "# λ😀\r\nlet mut n = 6; # 雪\r\nn = 12; n",
            "let mut n = 1; n = 22 # λ; comment\nn",
        ] {
            let snapshot = SourceSnapshot {
                source_id: SourceId::fresh(),
                revision_id: RevisionId::from_version(7),
                text: text.into(),
            };
            let expected = scope_script::reduction::reduce(&scope_script::runner::Snapshot {
                identity: snapshot.source_id.clone(),
                revision: snapshot.revision_id,
                text: text.into(),
            })
            .unwrap();
            let result = host.reduce_script_snapshot(snapshot.clone()).unwrap();
            assert_eq!(result.original, snapshot);
            assert_eq!(result.rule, expected.rule);
            assert_eq!(result.checked, expected.checked);
            let buffer = TextBuffer::new(text);
            let mut replay = text.to_owned();
            for (edit, raw) in result.edits.iter().zip(&expected.edits).rev() {
                let span = buffer.span_for_range(edit.range).unwrap();
                assert_eq!(span.into_range(), raw.range);
                replay.replace_range(raw.range.clone(), &edit.replacement);
            }
            assert_eq!(replay, expected.text);
        }
        for (text, reason) in [
            ("let mut =", "invalid source"),
            ("print(9)", "unsupported source"),
            ("scope.operations()", "unsupported source"),
        ] {
            let error = host
                .reduce_script_snapshot(SourceSnapshot {
                    source_id: SourceId::fresh(),
                    revision_id: RevisionId::from_version(1),
                    text: text.into(),
                })
                .unwrap_err();
            assert!(error.contains(reason), "{error}");
        }
    }
    assert!(semantic.records.is_empty());
    assert!(workbench.records.is_empty());
    assert_eq!(service.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn writable_save_as_matches_execution_snapshot_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("saved.scope");
    std::fs::write(&destination, "previous contents\n").unwrap();
    let mut host = WorkbenchRunner::default();
    let mut output = Vec::new();
    cli::run_with_session_and_command(
        App::new(Workspace::new(dir.path().to_string_lossy()).unwrap()).unwrap(),
        InteractiveSession::new(),
        &mut host,
        concat!(
            ":scope script enter\n",
            "fn twice(n) {\n  n * 2 # retained body comment\n}\n",
            "twice(4) # retained trailing comment\n",
            "(9)\n",
            ":scope scratch save-as saved.scope\n",
            ":run\n:script leave\nquit\n",
        )
        .as_bytes(),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(!output.contains("error:"), "{output}");
    assert!(output.contains("scratch saved as: saved.scope"), "{output}");
    assert_eq!(host.records.len(), 4, "{output}");
    let expected = concat!(
        "fn twice(n) {\n  n * 2 # retained body comment\n};\n",
        "twice(4) # retained trailing comment\n;\n(9)",
    );
    let before_save = &host.records[2];
    let after_save = &host.records[3];
    assert_eq!(before_save.source.text, expected);
    assert_eq!(after_save.source.text, expected);
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        before_save.source.text.as_bytes()
    );
    assert_eq!(before_save.outcome.as_ref().unwrap(), &Value::Integer(9));
    assert_eq!(
        after_save.outcome.as_ref().unwrap(),
        before_save.outcome.as_ref().unwrap()
    );
}

#[test]
fn multiline_blocks_comments_and_expression_prefixes_keep_hard_boundaries() {
    let mut host = WorkbenchRunner::default();
    let output = shell(
        &mut host,
        concat!(
            ":scope script enter\n",
            "if true {\n  7 # inside block\n}\n",
            "[10, 20] # trailing comment\n",
            "[1]\n",
            "(42)\n",
            ":script leave\nquit\n",
        ),
    );
    assert!(!output.contains("error:"), "{output}");
    assert_eq!(host.records.len(), 4, "{output}");
    let block = "if true {\n  7 # inside block\n}";
    let array = format!("{block};\n[10, 20] # trailing comment");
    let separate_array = format!("{array}\n;\n[1]");
    let separate_group = format!("{separate_array};\n(42)");
    for (record, expected) in host.records.iter().zip([
        block,
        array.as_str(),
        separate_array.as_str(),
        separate_group.as_str(),
    ]) {
        assert_eq!(record.source.text, expected);
        assert!(record.outcome.is_ok(), "{:?}", record.outcome);
    }
    assert_eq!(
        host.records[2].outcome.as_ref().unwrap(),
        &Value::Array(vec![Value::Integer(1)])
    );
    assert_eq!(
        host.records[3].outcome.as_ref().unwrap(),
        &Value::Integer(42)
    );
}

#[test]
fn existing_terminators_and_following_whitespace_do_not_add_separators() {
    for declaration in ["let n=8;", "let n=8;   ", "let n=8; # retained"] {
        let mut host = WorkbenchRunner::default();
        let input =
            format!(":scope script enter\n{declaration}\n   \nn + 1\n:show\n:script leave\nquit\n");
        let output = shell(&mut host, &input);
        assert!(!output.contains("error:"), "{output}");
        assert_eq!(host.records.len(), 2, "{output}");
        let expected = format!("{declaration}\nn + 1");
        let record = host.records.last().unwrap();
        assert_eq!(record.source.text, expected);
        assert_eq!(record.outcome.as_ref().unwrap(), &Value::Integer(9));
        assert!(output.contains(&format!("1 | {declaration}")), "{output}");
        assert!(output.contains("2 | n + 1"), "{output}");
    }
}

#[test]
fn expressions_variables_multiline_and_syntax_recovery() {
    let mut host = WorkbenchRunner::default();
    let output = shell(
        &mut host,
        concat!(
            ":scope script enter\n",
            "1 + 2\nlet answer = 40\nanswer + 2\n",
            "(\nanswer + 3\n)\n",
            "let = ;\nanswer + 4\n",
            ":scope script leave\nquit\n",
        ),
    );
    assert_eq!(host.records.len(), 5, "{output}");
    for (index, value) in [(0, 3), (2, 42), (3, 43), (4, 44)] {
        assert_eq!(
            host.records[index].outcome.as_ref().unwrap(),
            &Value::Integer(value)
        );
    }
    assert!(output.contains("incomplete"), "{output}");
    assert!(!host.records.last().unwrap().source.text.contains("let ="));
    assert!(host.records.iter().all(|record| record.outcome.is_ok()));
    assert!(
        host.records
            .windows(2)
            .all(|pair| pair[0].source.identity == pair[1].source.identity)
    );
}

#[test]
fn editor_commands_show_and_exit_reentry_preserve_source_not_bindings() {
    let mut host = WorkbenchRunner::default();
    let output = shell(
        &mut host,
        concat!(
            ":scope script enter\nlet answer = 40\nanswer + 2\n",
            ":commands\n:show\n:edit --commands\n:at 1 14\n:delete 2\n:insert 50\n:done\n",
            ":run\n:script leave\n:scope script enter\nanswer + 3\n",
            ":script leave\nquit\n",
        ),
    );
    assert_eq!(host.records.len(), 4, "{output}");
    assert_eq!(
        host.records[2].outcome.as_ref().unwrap(),
        &Value::Integer(52)
    );
    assert_eq!(
        host.records[3].outcome.as_ref().unwrap(),
        &Value::Integer(53)
    );
    assert_eq!(
        host.records[0].source.identity,
        host.records[3].source.identity
    );
    assert_ne!(
        host.records[1].source.revision,
        host.records[2].source.revision
    );
    assert!(output.contains("let answer = 40"), "{output}");
    assert!(output.contains("Left Script context"), "{output}");
    assert!(
        output.contains("error: unknown command: commands"),
        "{output}"
    );
    assert_eq!(output.matches("error:").count(), 1, "{output}");
}

#[test]
fn print_requires_explicit_repeated_runs_and_later_pure_input_never_replays() {
    let mut host = WorkbenchRunner::default();
    let output = shell(
        &mut host,
        concat!(
            ":scope script enter\nlet answer = 40\nprint(answer)\n",
            ":run\n:run\nanswer + 2\n:script leave\n",
            ":scope script enter\nanswer + 3\n:script leave\nquit\n",
        ),
    );
    assert_eq!(host.records.len(), 3, "{output}");
    assert!(host.records[0].printed.is_empty());
    for record in &host.records[1..] {
        assert_eq!(record.printed, [Value::Integer(40)]);
        assert!(record.outcome.is_ok());
    }
    assert_eq!(host.records[1].source.text, host.records[2].source.text);
    assert_eq!(
        host.records[1].source.revision,
        host.records[2].source.revision
    );
    assert_eq!(
        output
            .matches("Source updated. Run required (:run, full snapshot).")
            .count(),
        3,
        "{output}"
    );
}

#[test]
fn authorized_semantic_context_dispatches_only_on_explicit_runs() {
    let service = language::DeclarationLanguage::default();
    let mut host = ScriptRunner::new(&service, "describe_selection", Some(true));
    assert!(!host.supports_direct_script());
    let output = shell(
        &mut host,
        concat!(
            ":scope scratch new example.declarations\n",
            ":scope line 1 \"let answer = 42\"\n",
            ":scope semantic-target 0\n:scope scratch new scope-script\n",
            ":scope script enter\n",
            "let result = scope.invoke(\"semantic\", \"language.example.declarations.describe_selection\", {})\n",
            ":run\n:run\nresult.value.name\n:script leave\n",
            ":scope script enter\n42\n:script leave\nquit\n",
        ),
    );
    assert!(!output.contains("error:"), "{output}");
    assert_eq!(host.records.len(), 2, "{output}");
    assert_eq!(service.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    let first = &host.records[0];
    let second = &host.records[1];
    assert_eq!(first.source.identity, second.source.identity);
    assert_eq!(first.source.revision, second.source.revision);
    assert_eq!(first.source.text, second.source.text);
    assert_eq!(first.target.identity, second.target.identity);
    assert_eq!(first.target.revision, second.target.revision);
    assert_eq!(first.target.text, "let answer = 42");
    for record in &host.records {
        assert!(record.outcome.is_ok(), "{:?}", record.outcome);
        assert_eq!(record.results.len(), 1);
    }
    assert_eq!(
        output
            .matches("Source updated. Run required (:run, full snapshot).")
            .count(),
        3,
        "{output}"
    );
}

#[test]
fn semantic_admission_matches_workbench_without_execution_or_direct_support() {
    let service = language::DeclarationLanguage::default();
    let host = ScriptRunner::new(&service, "describe_selection", Some(true));
    let pure = WorkbenchRunner::default();
    for source in [
        "",
        "1 + 2",
        "(",
        "let = ;",
        "print(1)",
        "scope.invoke(\"semantic\", \"language.example.declarations.describe_selection\", {})",
        "fn effect() { print(1) }; effect()",
    ] {
        assert_eq!(
            host.admit_script_snapshot(source),
            pure.admit_script_snapshot(source),
            "{source}"
        );
    }
    assert!(!host.supports_direct_script());
    assert!(host.records.is_empty());
    assert_eq!(service.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn pending_commands_retain_draft_run_blocks_and_invalid_discards_whole_draft() {
    let mut host = WorkbenchRunner::default();
    let output = shell(
        &mut host,
        concat!(
            ":scope script enter\n(\n:show\n:run\n42)\n",
            "(\nlet = ;\n43\n(\n:cancel\n44\n:script leave\nquit\n",
        ),
    );
    assert_eq!(host.records.len(), 3, "{output}");
    for (record, value) in host.records.iter().zip([42, 43, 44]) {
        assert_eq!(record.outcome.as_ref().unwrap(), &Value::Integer(value));
    }
    assert!(
        output.contains("finish it or :cancel before :run"),
        "{output}"
    );
    assert!(output.contains("Committed Script source:\n\nPending Script snapshot (not committed; not evaluated):\n(\n"), "{output}");
    assert_eq!(host.records[0].source.text, "(\n42)");
    assert_eq!(host.records[1].source.text, "(\n42);\n43");
    assert_eq!(host.records[2].source.text, "(\n42);\n43;\n44");
    assert!(output.contains("...> "), "{output}");
    assert!(output.contains("script> "), "{output}");
    assert!(!host.records.last().unwrap().source.text.contains("let ="));
}

#[test]
fn concise_active_output_and_evidence_inspection_do_not_reexecute() {
    let mut host = WorkbenchRunner::default();
    let output = shell(
        &mut host,
        concat!(
            ":scope script enter\n42\nprint(7)\n:run\n:script evidence\n",
            ":script leave\nquit\n",
        ),
    );
    assert_eq!(
        host.records.len(),
        2,
        "inspection must not execute: {output}"
    );
    let (concise, evidence) = output.split_once("script: SourceId").unwrap();
    assert!(concise.contains("script> 42\n"), "{output}");
    assert!(concise.contains("script> 7\n"), "{output}");
    assert!(concise.contains("Source updated. Run required (:run, full snapshot)."));
    assert!(!concise.contains("final:"), "{output}");
    assert!(!concise.contains("status: interpreter"), "{output}");
    assert!(evidence.contains("source snapshot:"), "{output}");
    assert!(evidence.contains("print: 7"), "{output}");
    assert!(evidence.contains("status: interpreter success"), "{output}");
}
