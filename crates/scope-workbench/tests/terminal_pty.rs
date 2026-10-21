//! Actual-binary Unix terminal coverage; requires Python 3 (stdlib only).
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

fn python_suite(script: &Path) {
    let output = Command::new("python3")
        .arg("-B")
        .arg(script)
        .args(["--binary", env!("CARGO_BIN_EXE_scope")])
        .output()
        .expect("PTY integration tests require python3 on PATH");
    assert!(
        output.status.success(),
        "{} failed ({}):\n{}\n{}",
        script.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn workbench_end_user_pty() {
    python_suite(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pty_workbench.py"));
}

#[test]
fn existing_terminal_suite_against_workbench_binary() {
    python_suite(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../scope-ide/tests/pty_shell.py"));
}
