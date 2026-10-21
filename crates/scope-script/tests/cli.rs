use std::io::Write;
use std::process::{Command, Output, Stdio};

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_scope-script"));
    command.stdin(Stdio::null());
    command
}

fn script(source: impl AsRef<[u8]>) -> Output {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("script.scope");
    std::fs::write(&path, source).unwrap();
    cli().arg(path).output().unwrap()
}

fn script_with_output(mode: &str, source: &str) -> (Output, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("script.scope");
    std::fs::write(&path, source).unwrap();
    let output = cli()
        .current_dir(directory.path())
        .args(["--output", mode])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    let entries: Vec<_> = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(
        entries,
        vec![path.clone()],
        "execution must not write files"
    );
    (output, path)
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}

fn stderr(output: &Output) -> &str {
    std::str::from_utf8(&output.stderr).unwrap()
}

#[test]
fn help() {
    let output = cli().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(stdout(&output).contains("Usage: scope-script [--output prints|summary|json] FILE"));
    for mode in ["prints", "summary", "json", "scope-script.execution"] {
        assert!(stdout(&output).contains(mode));
    }
    assert!(stdout(&output).contains("REPL"));
    assert!(output.stderr.is_empty());
}

#[test]
fn output_arguments_are_validated() {
    for args in [
        vec!["--output"],
        vec!["--output", "invalid", "script.scope"],
        vec!["--output", "json"],
        vec!["--output", "json", "--output", "prints", "script.scope"],
        vec!["--help", "extra"],
    ] {
        let output = cli().args(&args).output().unwrap();
        assert!(!output.status.success(), "accepted {args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn explicit_prints_matches_default() {
    let source = "print({answer: [42, true, null]}); 99";
    let default = script(source);
    let (explicit, _) = script_with_output("prints", source);
    assert!(default.status.success(), "{}", stderr(&default));
    assert!(explicit.status.success(), "{}", stderr(&explicit));
    assert_eq!(explicit.stdout, default.stdout);
    assert!(explicit.stderr.is_empty());
}

#[test]
fn summary_success_labels_final_value_and_status() {
    let (output, _) = script_with_output("summary", "print(7); 42");
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "7\nFinal value: 42\nInterpreter status: success\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn summary_error_preserves_prints_and_reports_failure() {
    let (output, path) = script_with_output("summary", "print(7); missing");
    assert!(!output.status.success());
    assert_eq!(
        stdout(&output),
        "7\nFinal value: unavailable (evaluation failed)\nInterpreter status: error\n"
    );
    assert!(stderr(&output).contains(&format!("{}:1:11:", path.display())));
    assert!(stderr(&output).contains("unknown binding"));
}

#[test]
fn json_success_is_one_standalone_full_fidelity_record() {
    let long = "é".repeat(2_000);
    let source = format!("print(\"{long}\"); print([1, true, null]); {{answer: 42}}");
    let (output, path) = script_with_output("json", &source);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty());
    let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["schema"], "scope-script.execution");
    assert_eq!(record["version"], 1);
    assert_eq!(record["source"]["path"], path.to_str().unwrap());
    assert_eq!(record["source"]["text"], source);
    assert_eq!(
        record["printed"],
        serde_json::json!([long, [1, true, null]])
    );
    assert_eq!(
        record["outcome"],
        serde_json::json!({"status": "success", "value": {"answer": 42}})
    );
    assert_eq!(
        record["host"],
        serde_json::json!({"mode": "no-host", "invocations": []})
    );
}

#[test]
fn json_errors_are_stdout_only_with_source_chain_and_location() {
    for (source, printed, message, offset, line, column) in [
        (
            "print(\"é\"); missing",
            serde_json::json!(["é"]),
            "runtime error: unknown binding",
            13,
            1,
            13,
        ),
        (
            "print(7);\n@",
            serde_json::json!([]),
            "syntax error:",
            10,
            2,
            1,
        ),
    ] {
        let (output, path) = script_with_output("json", source);
        assert!(!output.status.success());
        assert!(output.stderr.is_empty(), "{}", stderr(&output));
        let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            record["source"],
            serde_json::json!({"path": path, "text": source})
        );
        assert_eq!(record["printed"], printed);
        assert_eq!(record["outcome"]["status"], "error");
        let error = &record["outcome"]["error"];
        assert_eq!(error["offset"], offset);
        assert_eq!(error["line"], line);
        assert_eq!(error["column"], column);
        assert!(error["message"].as_str().unwrap().starts_with(message));
        let chain = error["source_chain"].as_array().unwrap();
        assert_eq!(chain.len(), 1);
        assert!(chain[0].as_str().unwrap().starts_with(message));
        assert_eq!(
            record["host"],
            serde_json::json!({"mode": "no-host", "invocations": []})
        );
    }
}

#[test]
fn pure_file_prints_without_repl_or_implicit_result() {
    let output = script("let x = 5\nprint(x * 2)\n99\n");
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "10\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn rerun_reads_edits_with_fresh_bindings_and_no_writes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("daily.scope");
    let original = include_str!("../examples/daily.scope");
    std::fs::write(&path, original).unwrap();
    let first = cli().arg(&path).output().unwrap();
    let second = cli().arg(&path).output().unwrap();
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(second.status.success(), "{}", stderr(&second));
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let edited = "let experiment = 9\nprint(experiment)\n";
    std::fs::write(&path, edited).unwrap();
    let third = cli().arg(&path).output().unwrap();
    assert!(third.status.success(), "{}", stderr(&third));
    assert_eq!(stdout(&third), "9\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn diagnostics_identify_file_line_and_unicode_scalar_column() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("diagnostic.scope");
    std::fs::write(&path, "print(\"é\")\nmissing\n").unwrap();
    let output = cli().arg(&path).output().unwrap();
    assert!(!output.status.success());
    assert_eq!(stdout(&output), "\"é\"\n");
    assert!(stderr(&output).contains(&format!("{}:2:1:", path.display())));
    assert!(stderr(&output).contains("at byte"));
}

#[test]
fn missing_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing.scope");
    let output = cli().arg(&path).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains(path.to_str().unwrap()));
    assert!(stderr(&output).contains("scope-script:"));
}

#[test]
fn syntax_error_executes_nothing() {
    let output = script("print(1); let x = [");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains("syntax error:"));
    assert!(stderr(&output).contains("at byte"));
}

#[test]
fn runtime_error_preserves_printed_output() {
    let output = script("print(7); 1 / 0; print(8)");
    assert!(!output.status.success());
    assert_eq!(stdout(&output), "7\n");
    assert!(stderr(&output).contains("runtime error:"));
    assert!(stderr(&output).contains("at byte"));
}

#[test]
fn files_have_no_host_authority() {
    let output = script("print(scope.operations()); scope.invoke(\"p\", \"a\", {})");
    assert!(!output.status.success());
    assert_eq!(stdout(&output), "[]\n");
    assert!(stderr(&output).contains("at byte"));
    assert!(!output.stderr.is_empty());
}

#[test]
fn source_size_limit() {
    let output = script(" ".repeat(65_536));
    assert!(output.status.success(), "{}", stderr(&output));
    let output = script(" ".repeat(65_537));
    assert!(!output.status.success());
    assert!(stderr(&output).contains("source size limit exceeded"));
    assert!(output.stdout.is_empty());
}

#[test]
fn invalid_utf8() {
    let output = script([0xff]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("not UTF-8"));
}

#[test]
fn extra_arguments_are_rejected() {
    let output = cli().args(["first.scope", "extra.scope"]).output().unwrap();
    assert!(!output.status.success());
    assert!(stderr(&output).contains("expected a single script path"));
}

fn stdin_script(input: &[u8]) -> Output {
    let mut child = cli()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn redirected_input_preserves_partial_final_line_and_error_recovery() {
    let output = stdin_script(b"let x = 4\r\n1 / 0\nprint(x)\nx + 1");
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.starts_with("=> 4\n"));
    assert!(text.contains("runtime error:"));
    assert!(text.ends_with("4\n=> 5\n"));
    assert!(!text.contains("scope>"));
    assert!(!text.contains('\u{1b}'));
    assert!(output.stderr.is_empty());
}

#[test]
fn redirected_input_retains_validation_and_line_bound() {
    let output = stdin_script(b"\xff\n2\n");
    assert_eq!(stdout(&output), "input is not UTF-8\n=> 2\n");
    let output = stdin_script(&vec![b' '; 65_537]);
    assert_eq!(stdout(&output), "input line too large; session ended\n");
    assert!(output.status.success());
}

#[test]
fn no_arguments_preserves_repl() {
    let mut child = cli()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"1 + 2\n:quit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(stdout(&output), "=> 3\n");
    assert!(output.stderr.is_empty());
}
