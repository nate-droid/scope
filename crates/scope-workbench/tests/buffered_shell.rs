use std::io::Write;
use std::process::{Command, Stdio};

/// Exercise the installed command loop, not the runner or an in-process shell.
fn shell(input: &str) -> String {
    let workspace = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_scope"))
        .current_dir(workspace.path())
        .env("XDG_STATE_HOME", workspace.path())
        .env_remove("SCOPE_HISTORY_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    assert_eq!(
        std::fs::read_dir(workspace.path()).unwrap().count(),
        0,
        "buffered editing and execution must not create files: {stdout}"
    );
    stdout
}

fn draft(source: &str) -> String {
    format!(":scope scratch new script\n:scope edit\n{source}\n:done\n")
}

fn runs(output: &str) -> Vec<&str> {
    output.split("script: ").skip(1).collect()
}

#[test]
fn literal_multiline_runs_repeat_then_edit_with_owned_revisions_and_fresh_bindings() {
    let source = "let answer = 40;\nif false { missing } else { print(\"雪😀\") };\nanswer + 2";
    let mut input = draft(source);
    input.push_str(":scope show\n:scope script run\n:scope script run\n");
    // Replace the complete draft through the cooked editor, not an escaped insert.
    input.push_str(&format!(":scope edit\n:at 1 1\n:delete {}\nlet answer = 7;\nif true {{ print(answer) }} else {{ missing }};\nanswer\n:done\n:scope script run\n", source.chars().count() + 1));
    input.push_str(&draft("answer"));
    input.push_str(":scope script run\n:scope quit\n");
    let output = shell(&input);
    let records = runs(&output);
    assert_eq!(records.len(), 4, "{output}");
    for record in &records[..2] {
        assert!(
            record.contains("print: \"雪😀\"\nfinal: 42\nstatus: interpreter success"),
            "{output}"
        );
    }
    let provenance = |record: &str| record.lines().next().unwrap().to_owned();
    assert_eq!(provenance(records[0]), provenance(records[1]));
    assert_ne!(provenance(records[1]), provenance(records[2]));
    assert!(
        records[2].contains("final: 7\nstatus: interpreter success"),
        "{output}"
    );
    assert!(records[3].contains("unknown binding"), "{output}");
    assert!(
        records[3].contains("status: interpreter failure"),
        "{output}"
    );
    assert!(!records[3].contains("print:"), "{output}");
    assert!(!output.contains("input line rejected"), "{output}");
}

#[test]
fn syntax_runtime_and_immutable_errors_preserve_ordered_unicode_prints_and_recover() {
    let runtime = "print(\"雪😀\"); print(2); missing";
    let mut input = draft(runtime);
    input.push_str(":scope script run\n");
    input.push_str(&draft("print(999); if true { print([) }"));
    input.push_str(":scope script run\n");
    input.push_str(&draft("print(1); let fixed = 1; fixed = print(999)"));
    input.push_str(":scope script run\n");
    input.push_str(&draft("let mut answer = 6; answer = answer * 7; answer"));
    input.push_str(":scope script run\n:scope quit\n");
    let output = shell(&input);
    let records = runs(&output);
    assert_eq!(records.len(), 4, "{output}");
    let record = records[0];
    assert!(record.find("print: \"雪😀\"").unwrap() < record.find("print: 2").unwrap());
    assert!(record.find("print: 2").unwrap() < record.find("diagnostic:").unwrap());
    let offset = runtime.find("missing").unwrap();
    let column = runtime[..offset].chars().count() + 1;
    assert!(
        record.contains(&format!(
            "1:{column} (Unicode scalars; byte offset {offset})"
        )),
        "{output}"
    );
    for record in &records[..3] {
        assert!(record.contains("diagnostic:"), "{output}");
        assert!(record.contains("status: interpreter failure"), "{output}");
        assert!(record.contains("error:"), "{output}");
    }
    assert!(records[1].contains("error: syntax error:"), "{output}");
    assert!(
        !records[1].contains("print:"),
        "syntax errors must not execute: {output}"
    );
    assert!(
        records[2].contains("error: runtime error: assignment requires a mutable binding"),
        "valid assignment syntax must enforce binding mutability at runtime: {output}"
    );
    assert!(
        records[2].find("print: 1").unwrap() < records[2].find("diagnostic:").unwrap(),
        "statements before the runtime failure must execute: {output}"
    );
    assert!(
        !records[2].contains("print: 999"),
        "immutable assignment must reject before evaluating its RHS: {output}"
    );
    assert!(
        records[3].contains("final: 42\nstatus: interpreter success"),
        "{output}"
    );
}

#[test]
fn script_execution_preserves_the_actual_registered_session_state() {
    let mut input =
        String::from(":scope languages\n:scope session\n:scope session scope\n:scope session\n");
    input.push_str(&draft("print(42); 7"));
    input.push_str(
        ":scope session\n:scope script run\n:scope session\n:scope context\n:scope quit\n",
    );
    let output = shell(&input);
    // The default unprivileged binary registers analysis services, not evaluators.
    assert!(
        output.contains("Interactive evaluators:\n  none registered"),
        "{output}"
    );
    assert!(output.contains("unsupported language: scope"), "{output}");
    assert_eq!(
        output.matches("Active language: none").count(),
        4,
        "{output}"
    );
    assert!(output.contains("Interactive session: none"), "{output}");
    assert!(
        output.contains("final: 7\nstatus: interpreter success"),
        "{output}"
    );
}
