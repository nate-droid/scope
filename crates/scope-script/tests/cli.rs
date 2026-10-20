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
    assert!(stdout(&output).contains("Usage: scope-script [FILE]"));
    assert!(stdout(&output).contains("REPL"));
    assert!(output.stderr.is_empty());
}

#[test]
fn pure_file_prints_without_repl_or_implicit_result() {
    let output = script("let x = 5\nprint(x * 2)\n99\n");
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Integer(10)\n");
    assert!(output.stderr.is_empty());
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
    assert_eq!(stdout(&output), "Integer(7)\n");
    assert!(stderr(&output).contains("runtime error:"));
    assert!(stderr(&output).contains("at byte"));
}

#[test]
fn files_have_no_host_authority() {
    let output = script("print(scope.operations()); scope.invoke(\"p\", \"a\", {})");
    assert!(!output.status.success());
    assert_eq!(stdout(&output), "Array([])\n");
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
    let output = cli().args(["--help", "extra"]).output().unwrap();
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
    assert!(text.starts_with("=> Integer(4)\n"));
    assert!(text.contains("runtime error:"));
    assert!(text.ends_with("Integer(4)\n=> Integer(5)\n"));
    assert!(!text.contains("scope>"));
    assert!(!text.contains('\u{1b}'));
    assert!(output.stderr.is_empty());
}

#[test]
fn redirected_input_retains_validation_and_line_bound() {
    let output = stdin_script(b"\xff\n2\n");
    assert_eq!(stdout(&output), "input is not UTF-8\n=> Integer(2)\n");
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
    assert_eq!(stdout(&output), "=> Integer(3)\n");
    assert!(output.stderr.is_empty());
}
