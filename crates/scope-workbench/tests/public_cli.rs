use std::process::Command;

#[test]
fn public_help_is_successful_without_a_workspace() {
    let root = tempfile::tempdir().unwrap();
    for flag in ["--help", "-h"] {
        let output = Command::new(env!("CARGO_BIN_EXE_scope"))
            .args([flag])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        for text in [
            "Usage: scope",
            "No arguments:",
            "shell",
            "piped input",
            "Developer commands",
            "--version",
            "--writable",
            "scope-script",
        ] {
            assert!(help.contains(text), "missing {text:?}: {help}");
        }
        #[cfg(feature = "tui")]
        for text in ["workspace editor", "Ctrl-S", "F5", "F2", "returns"] {
            assert!(help.contains(text), "missing {text:?}: {help}");
        }
        #[cfg(not(feature = "tui"))]
        assert!(help.contains("This build has no TUI"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn public_version_reports_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_scope"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("scope {}\n", env!("CARGO_PKG_VERSION"))
    );
}
