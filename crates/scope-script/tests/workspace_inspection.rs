#[path = "../examples/workspace_support/mod.rs"]
mod support;
use scope_ide::{tool::operations::OperationError, workspace::WorkspaceError};
use scope_script::{ErrorKind, Host, Interpreter, Value};
use support::{InspectionError, PROVIDER, SCRIPT, WorkspaceHost};

fn open(dir: &tempfile::TempDir) -> WorkspaceHost {
    WorkspaceHost::open(dir.path().to_str().unwrap(), true).unwrap()
}
fn invoke(host: &mut WorkspaceHost, action: &str) -> Result<Value, ErrorKind> {
    host.invoke(PROVIDER, action, Value::Object(Default::default()))
}
#[test]
fn script_matches_direct_rust_and_does_not_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    let text = "# Agent handoff 🦀\n## Context\nTODO: verify café\n- [ ] run tests\n";
    std::fs::write(dir.path().join("docs/handoff.md"), text).unwrap();
    std::fs::write(dir.path().join("ignore.txt"), "TODO ignored").unwrap();
    let actual = Interpreter::new().eval(SCRIPT, &mut open(&dir)).unwrap();
    assert_eq!(actual, support::direct(&mut open(&dir)).unwrap());
    let Value::Object(report) = actual else {
        panic!()
    };
    assert_eq!(report["document_count"], Value::Integer(1));
    assert_eq!(report["todo_count"], Value::Integer(2));
    assert_eq!(report["needs_follow_up"], Value::Boolean(true));
    let Value::Array(documents) = &report["documents"] else {
        panic!()
    };
    let Value::Object(document) = &documents[0] else {
        panic!()
    };
    assert_eq!(document["text"], text.into());
    assert_eq!(
        document["headings"],
        Value::Array(vec!["# Agent handoff 🦀".into(), "## Context".into()])
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("docs/handoff.md")).unwrap(),
        text
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}
#[test]
fn daily_use_examples_are_read_only_and_handle_empty_workspaces() {
    let inventory = include_str!("../examples/markdown-inventory.scope");
    let pending = include_str!("../examples/pending-review.scope");
    for source in [inventory, pending] {
        for text in [None, Some("# Review\nTODO: check build\n- [ ] test\n")] {
            let dir = tempfile::tempdir().unwrap();
            if let Some(text) = text {
                std::fs::write(dir.path().join("review.md"), text).unwrap();
            }
            let mut interpreter = Interpreter::new();
            let report = interpreter.eval(source, &mut open(&dir)).unwrap();
            assert_eq!(interpreter.take_output(), vec![report.clone()]);
            let Value::Object(fields) = report else {
                panic!()
            };
            let Value::Array(documents) = &fields["documents"] else {
                panic!()
            };
            assert_eq!(documents.len(), usize::from(text.is_some()));
            if source == pending {
                assert_eq!(
                    fields["pending_count"],
                    Value::Integer(if text.is_some() { 2 } else { 0 })
                );
                assert_eq!(fields["needs_follow_up"], Value::Boolean(text.is_some()));
            }
            assert_eq!(
                std::fs::read_dir(dir.path()).unwrap().count(),
                usize::from(text.is_some())
            );
            if let Some(text) = text {
                assert_eq!(
                    std::fs::read_to_string(dir.path().join("review.md")).unwrap(),
                    text
                );
            }
        }
    }
}

#[test]
fn empty_workspace_is_a_valid_report() {
    let dir = tempfile::tempdir().unwrap();
    let report = Interpreter::new().eval(SCRIPT, &mut open(&dir)).unwrap();
    assert_eq!(report, support::direct(&mut open(&dir)).unwrap());
}
#[test]
fn deny_before_open_and_reject_script_authority() {
    assert!(matches!(
        WorkspaceHost::open("/does/not/exist", false),
        Err(InspectionError::Unauthorized)
    ));
    let dir = tempfile::tempdir().unwrap();
    let mut host = open(&dir);
    assert!(matches!(
        invoke(&mut host, "save"),
        Err(ErrorKind::Operation(OperationError::UnknownOperation))
    ));
    assert!(
        host.invoke(
            PROVIDER,
            "discover_markdown",
            Value::object([("path", "../secret".into())])
        )
        .is_err()
    );
    assert!(matches!(
        invoke(&mut host, "inspect_discovered"),
        Err(ErrorKind::Runtime(_))
    ));
    assert!(matches!(
        host.last_error,
        Some(InspectionError::DiscoveryRequired)
    ));
}
#[test]
fn revisions_and_workspace_errors_remain_structured() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("handoff.md");
    std::fs::write(&path, "# Before").unwrap();
    let mut host = open(&dir);
    invoke(&mut host, "discover_markdown").unwrap();
    std::fs::write(&path, "# After").unwrap();
    assert!(matches!(
        invoke(&mut host, "inspect_discovered"),
        Err(ErrorKind::Operation(OperationError::StaleRevision))
    ));
    assert!(
        matches!(&host.last_error, Some(InspectionError::Stale { path }) if path == "handoff.md")
    );
    std::fs::remove_file(path).unwrap();
    assert!(invoke(&mut host, "inspect_discovered").is_err());
    assert!(matches!(
        host.last_error,
        Some(InspectionError::Workspace(WorkspaceError::ReadFile { .. }))
    ));
}
#[test]
fn file_content_and_call_limits_fail_without_truncation() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..17 {
        std::fs::write(dir.path().join(format!("{i}.md")), "# Doc").unwrap();
    }
    assert!(matches!(open(&dir).discover(), Err(InspectionError::Limit)));
    let large = tempfile::tempdir().unwrap();
    std::fs::write(large.path().join("large.md"), "x".repeat(32_769)).unwrap();
    assert!(matches!(
        open(&large).discover(),
        Err(InspectionError::Limit)
    ));
    let empty = tempfile::tempdir().unwrap();
    let mut host = open(&empty);
    for _ in 0..8 {
        host.operations().unwrap();
    }
    assert!(matches!(host.operations(), Err(ErrorKind::LimitExceeded)));
    let many = tempfile::tempdir().unwrap();
    for i in 0..129 {
        std::fs::write(many.path().join(format!("{i}.txt")), "").unwrap();
    }
    assert!(matches!(
        open(&many).discover(),
        Err(InspectionError::Limit)
    ));
}
#[cfg(unix)]
#[test]
fn symlinks_are_not_discovered() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.md"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.md"), dir.path().join("link.md"))
        .unwrap();
    assert_eq!(open(&dir).discover().unwrap(), Value::Array(vec![]));
}
