use std::process::Command;

#[test]
fn practical_examples_run_deterministically_without_a_host() {
    for (name, expected) in [
        ("collection-processing.scope", "\"flagged\": 1"),
        ("reusable-computations.scope", "\"total_cents\": 5022"),
        ("release-readiness.scope", "\"review_minutes\": 35"),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(name);
        let execute = || {
            Command::new(env!("CARGO_BIN_EXE_scope-script"))
                .arg(&path)
                .output()
                .expect("stock CLI starts")
        };
        let first = execute();
        let second = execute();
        assert!(first.status.success(), "{name}: {:?}", first.stderr);
        assert!(second.status.success(), "{name}: {:?}", second.stderr);
        assert_eq!(first.stdout, second.stdout, "{name}");
        let text = String::from_utf8(first.stdout).unwrap();
        assert!(text.contains(expected), "{name}: {text}");
    }
}
