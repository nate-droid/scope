use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn downstream_runner_exposes_the_registered_language() -> Result<(), Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_calc-scope"))
        .arg(format!("{}/fixtures", env!("CARGO_MANIFEST_DIR")))
        .arg("open")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"open main.calc\nlanguage\noutline\ndiagnostics\nquit\n")?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("example.calc"), "{stdout}");
    assert!(stdout.contains("answer"), "{stdout}");
    assert!(
        stdout.contains("expected definition name after `def`"),
        "{stdout}"
    );
    Ok(())
}
