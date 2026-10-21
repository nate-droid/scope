use std::io::Write;
use std::process::{Command, Stdio};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_scope-ide"))
}

#[test]
fn no_arguments_start_shell_without_writes() {
    let root = std::env::temp_dir().join(format!("scope-runtime-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let output = binary()
        .current_dir(&root)
        .env_remove("SCOPE_HISTORY_FILE")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let entries = std::fs::read_dir(&root).unwrap().count();
    std::fs::remove_dir(&root).unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("scope> "));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Workspace access: read-only"));
    assert_eq!(entries, 0);
}

#[test]
fn missing_workspace_is_not_created() {
    let root = std::env::temp_dir().join(format!("scope-runtime-missing-{}", std::process::id()));
    assert!(!root.exists());
    for command in ["shell", "list", "meta"] {
        let output = binary()
            .arg("--workspace")
            .arg(&root)
            .arg(command)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("existing directory"));
        assert!(!root.exists());
    }
}

#[test]
fn help_does_not_open_workspace_and_discovers_runtimes() {
    let output = binary()
        .args(["--workspace", "/nonexistent/scope-runtime", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("cargo run --"));
    assert!(help.contains("cargo run -p scope-script"));
}

#[test]
fn authority_and_save_require_explicit_write_grant() {
    let root = std::env::temp_dir().join(format!("scope-runtime-authority-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("sample.txt");
    for writable in [false, true] {
        std::fs::write(&path, "before\n").unwrap();
        let mut command = binary();
        command.arg("--workspace").arg(&root);
        if writable {
            command.arg("--writable");
        }
        let mut child = command
            .arg("shell")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"authority\nopen sample.txt\nreplace 1 1 1 7 after\nsave\nquit\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains(if writable {
                "write policy: WithinRoot"
            } else {
                "write policy: Denied"
            }),
            "{text}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            if writable { "after\n" } else { "before\n" },
            "{text}"
        );
        if !writable {
            assert!(text.contains("error:"), "{text}");
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn language_discovery_reports_analysis_separately_from_evaluation() {
    let mut child = binary()
        .args(["--workspace", env!("CARGO_MANIFEST_DIR"), "shell"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            b"languages\n:scope languages\n:scope session\n:scope context\nlanguages extra\nquit\n",
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        text.matches("Analysis services (not evaluators):").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("scope.simple — Scope Simple").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("semantic operations: none installed").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("Interactive evaluators:\n  none registered")
            .count(),
        2,
        "{text}"
    );
    assert!(text.contains("Active session: none"), "{text}");
    assert!(text.contains("Active language: none"), "{text}");
    assert!(text.contains("unexpected argument: extra"), "{text}");
    assert!(!text.contains("unknown command"), "{text}");
    assert!(text.contains("cargo run -p scope-script -- --help"));
}

#[test]
fn workspace_only_keeps_listing_compatibility() {
    let output = binary().arg(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("workspace:"));
    assert!(!text.contains("scope> "));
}
