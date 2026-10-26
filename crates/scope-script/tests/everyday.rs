use scope_ide::scratch::ScratchBuffers;
use scope_script::{
    Interpreter, host::NoHost, presentation::value_json, workbench::WorkbenchRunner,
};
use std::process::Command;

#[test]
fn examples_match_file_and_fresh_scratch_runs() {
    for (name, text, succeeds) in [
        ("hello.scope", include_str!("../examples/hello.scope"), true),
        (
            "checklist.scope",
            include_str!("../examples/checklist.scope"),
            true,
        ),
        (
            "expected-failure.scope",
            include_str!("../examples/expected-failure.scope"),
            false,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(name);
        std::fs::write(&path, text).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_scope-script"))
            .args(["--output", "json"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.success(), succeeds);
        assert!(output.stderr.is_empty());
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut buffers = ScratchBuffers::default();
        buffers.create("scope-script", text);
        let mut runner = WorkbenchRunner::default();
        for _ in 0..2 {
            let record = runner.run(buffers.selected().unwrap());
            assert_eq!(record.outcome.is_ok(), succeeds);
            assert_eq!(
                json["printed"],
                serde_json::json!(record.printed.iter().map(value_json).collect::<Vec<_>>())
            );
            match &record.outcome {
                Ok(value) => assert_eq!(json["outcome"]["value"], value_json(value)),
                Err(error) => {
                    assert_eq!(json["outcome"]["error"]["offset"], error.offset);
                    assert_eq!(json["outcome"]["error"]["message"], error.to_string());
                }
            }
        }
    }
}

#[test]
fn helpers_do_not_install_or_authorize_operations() {
    let mut interpreter = Interpreter::new();
    assert_eq!(
        interpreter
            .eval("len(scope.operations())", &mut NoHost)
            .unwrap(),
        scope_script::Value::Integer(0)
    );
    let error = interpreter
        .eval("scope.invoke(\"filesystem\", \"read\", {})", &mut NoHost)
        .unwrap_err();
    assert!(matches!(error.kind, scope_script::ErrorKind::Operation(_)));
}
