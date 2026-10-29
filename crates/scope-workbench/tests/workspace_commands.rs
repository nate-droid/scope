use scope_ide::{
    app::App,
    cli::workspace_command::WorkspaceCommandOutput,
    language::InteractiveSession,
    scratch::ScratchBuffers,
    text::{Position, TextEdit},
    workspace::Workspace,
};
use scope_script::{Value, workbench::WorkbenchRunner};

#[test]
fn workspace_commands_use_existing_runner_and_retain_revision_output_and_sources() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("target.txt");
    std::fs::write(&path, "disk unchanged").unwrap();
    let mut app = App::new(Workspace::read_only(root.path().to_str().unwrap()).unwrap()).unwrap();
    let mut session = InteractiveSession::new();
    let mut sources = ScratchBuffers::default();
    let target = sources.create("text", "independent target");
    let script = sources.create("script", "print(7); 42");
    sources.bind_semantic_target(Some(target)).unwrap();
    let mut runner = WorkbenchRunner::default();
    let mut output = WorkspaceCommandOutput::default();
    output
        .submit(
            ":scope script run",
            &mut app,
            &mut session,
            Some(&mut runner),
            &mut sources,
        )
        .unwrap();
    assert_eq!(runner.records.len(), 1);
    assert_eq!(runner.records[0].printed, [Value::Integer(7)]);
    assert_eq!(
        runner.records[0].outcome.as_ref().unwrap(),
        &Value::Integer(42)
    );
    let original_revision = runner.records[0].source.revision;
    sources
        .selected_mut()
        .unwrap()
        .apply_edit(TextEdit::insert(Position::new(0, 0), "print(8); "))
        .unwrap();
    output
        .submit(
            ":scope script run",
            &mut app,
            &mut session,
            Some(&mut runner),
            &mut sources,
        )
        .unwrap();
    assert_ne!(runner.records[1].source.revision, original_revision);
    assert_eq!(runner.records[0].source.text, "print(7); 42");
    assert_eq!(sources.selected().unwrap().id(), script);
    assert_eq!(sources.semantic_target().unwrap().unwrap().id(), target);
    let transcript = String::from_utf8_lossy(output.bytes());
    assert!(transcript.contains("print: 7"));
    assert!(transcript.contains("print: 8"));
    assert!(transcript.contains("final: 42"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "disk unchanged");
    assert!(app.open_document().is_none());
    assert!(session.language_id().is_none());
}

#[cfg(feature = "tui")]
#[test]
fn normal_executable_advertises_workspace() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_scope"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("workspace [ROOT]"));
}
