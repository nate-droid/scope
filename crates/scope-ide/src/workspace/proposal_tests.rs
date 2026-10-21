use super::*;
use crate::test_support::TempWorkspace;

fn path(value: &str) -> ScopePath {
    ScopePath::new(value).unwrap()
}
fn snapshot(fixture: &TempWorkspace, paths: &[&str]) -> Proposal {
    Proposal::new(
        &fixture.workspace(),
        &paths.iter().map(|value| path(value)).collect::<Vec<_>>(),
        ProposalLimits::default(),
    )
    .unwrap()
}

#[test]
fn structured_read_errors_distinguish_cell_scope_and_missing() {
    use std::error::Error;
    let fixture = TempWorkspace::new("proposal-structured-errors");
    fixture.write_bytes("invalid", &[0xff]);
    fixture.write_file("large", "abcd");
    fixture.write_file("nul", "\0");
    fixture.write_file("a", "abc");
    fixture.write_file("b", "def");
    fixture.create_dir("directory");
    let workspace = fixture.workspace();
    let limits = ProposalLimits {
        max_files: 2,
        max_file_bytes: 3,
        max_total_bytes: 5,
    };
    for (name, expected) in [
        ("invalid", FsErrorKind::InvalidTextEncoding),
        ("large", FsErrorKind::ContentTooLarge),
        ("missing/child", FsErrorKind::NotFound),
    ] {
        let error = Proposal::new(&workspace, &[path(name)], limits).unwrap_err();
        let source = error.fs_error().unwrap();
        assert_eq!(source.kind, expected);
        assert!(source.path.starts_with("project/"));
        assert!(!source.operation.is_empty());
        assert_eq!(
            error.source().unwrap().downcast_ref::<FsError>(),
            Some(source)
        );
        assert_eq!(error.to_string(), error.0);
    }
    for (paths, kind) in [
        (vec![path("target/new")], ProposalErrorKind::Policy),
        (vec![path("directory")], ProposalErrorKind::NonRegularFile),
        (vec![path("nul")], ProposalErrorKind::NulText),
        (vec![path("a"), path("b")], ProposalErrorKind::Budget),
    ] {
        let error = Proposal::new(&workspace, &paths, limits).unwrap_err();
        assert_eq!(error.kind(), &kind);
        assert!(error.fs_error().is_none());
        assert!(error.source().is_none());
    }
    let mut proposal = Proposal::new(&workspace, &[path("new")], limits).unwrap();
    assert_eq!(
        proposal.read(&path("new")).unwrap_err().kind(),
        &ProposalErrorKind::Missing
    );
    assert_eq!(
        proposal
            .create(&path("new"), "four".into())
            .unwrap_err()
            .kind(),
        &ProposalErrorKind::Budget
    );
    assert_eq!(
        proposal
            .create(&path("new"), "\0".into())
            .unwrap_err()
            .kind(),
        &ProposalErrorKind::NulText
    );
}

#[test]
fn no_grant_never_falls_back_to_host_root_or_installed_cell() {
    let fixture = TempWorkspace::new("proposal-no-grant");
    fixture.write_file("a", "host content");
    let mut workspace = fixture.workspace();
    // Keep the descriptive root and installed Cell, but remove the filesystem
    // receiver's grants. Neither may be used as an alternate authority.
    workspace.fs = cell_capability_fs::Workspace::new();
    let error = Proposal::new(&workspace, &[path("a")], ProposalLimits::default()).unwrap_err();
    assert!(error.fs_error().is_some());
    let mut proposal = snapshot(&fixture, &["new"]);
    proposal.create(&path("new"), "proposed".into()).unwrap();
    proposal.workspace.fs = cell_capability_fs::Workspace::new();
    let result = proposal.accept();
    assert!(result.error.is_some());
    assert!(result.applied.is_empty());
    assert!(!fixture.host_path_exists("new"));
    assert_eq!(fixture.read_file("a"), "host content");
}

#[cfg(unix)]
#[test]
fn concurrent_cell_creates_have_exactly_one_complete_publication() {
    use std::sync::{Arc, Barrier};
    let fixture = TempWorkspace::new("proposal-concurrent-create");
    let workspace = fixture.workspace();
    let barrier = Arc::new(Barrier::new(2));
    let results = std::thread::scope(|threads| {
        let handles: Vec<_> = ["first complete content", "second complete content"]
            .into_iter()
            .map(|text| {
                let workspace = workspace.clone();
                let barrier = barrier.clone();
                threads.spawn(move || {
                    let mut proposal =
                        Proposal::new(&workspace, &[path("new")], ProposalLimits::default())
                            .unwrap();
                    proposal.create(&path("new"), text.into()).unwrap();
                    let change = proposal.changes().unwrap().pop().unwrap();
                    let checked = proposal.check_baseline(&change).unwrap();
                    // Both contenders have observed absence before either publishes.
                    // Exercise the real Cell publication primitive, without sleeps
                    // or mocks, at the exact post-recheck boundary used by accept.
                    barrier.wait();
                    (text, workspace.fs.create_text(checked, text))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        results.iter().filter(|(_, result)| result.is_ok()).count(),
        1
    );
    let (winner, revision) = results.iter().find(|(_, result)| result.is_ok()).unwrap();
    let loser = results.iter().find(|(_, result)| result.is_err()).unwrap();
    assert_eq!(
        loser.1.as_ref().unwrap_err().kind,
        FsErrorKind::AlreadyExists
    );
    let published = workspace.load_file_revision("new").unwrap();
    assert_eq!(published.contents, *winner);
    assert_eq!(&published.revision, revision.as_ref().unwrap());
}

#[test]
fn isolated_create_replace_review_accept() {
    let fixture = TempWorkspace::new("proposal-happy");
    fixture.write_file("a", "user edits");
    let mut proposal = snapshot(&fixture, &["a", "new"]);
    proposal.replace(&path("a"), "proposed".into(), 0).unwrap();
    proposal.create(&path("new"), "created".into()).unwrap();
    assert_eq!(fixture.read_file("a"), "user edits");
    assert!(!fixture.host_path_exists("new"));
    assert_eq!(proposal.changes().unwrap().len(), 2);
    assert!(proposal.create(&path("new"), "overwrite".into()).is_err());
    assert!(proposal.create(&path("a"), "overwrite".into()).is_err());
    assert!(proposal.replace(&path("a"), "stale".into(), 0).is_err());
    let result = proposal.accept();
    assert!(result.error.is_none());
    assert_eq!(result.applied, vec![path("a"), path("new")]);
    assert_eq!(fixture.read_file("a"), "proposed");
    assert_eq!(fixture.read_file("new"), "created");
    assert!(proposal.changes().is_err());
}

#[test]
fn preflight_all_paths_before_writing_and_creation_conflict() {
    let fixture = TempWorkspace::new("proposal-conflict");
    fixture.write_file("a", "original");
    let mut proposal = snapshot(&fixture, &["a", "z"]);
    proposal.replace(&path("a"), "proposed".into(), 0).unwrap();
    proposal.create(&path("z"), "created".into()).unwrap();
    fixture.write_file("z", "external new file");
    let result = proposal.accept();
    assert!(result.error.is_some());
    assert!(result.applied.is_empty());
    assert_eq!(fixture.read_file("a"), "original");
    assert_eq!(fixture.read_file("z"), "external new file");
    assert_eq!(proposal.changes().unwrap().len(), 2);
}

#[test]
fn replacement_uses_retained_revision_and_blocks_missing_token() {
    let fixture = TempWorkspace::new("proposal-retained-revision");
    fixture.write_file("a", "original");
    let mut proposal = snapshot(&fixture, &["a"]);
    let revision = proposal.files[&path("a")].revision.clone().unwrap();
    assert_eq!(
        revision,
        fixture
            .workspace()
            .load_file_revision("a")
            .unwrap()
            .revision
    );
    proposal.replace(&path("a"), "proposed".into(), 0).unwrap();
    // Bytes still pass the bounded preflight. Only Cell's save-time expected
    // revision check can reject this deliberately mismatched retained token.
    proposal.files.get_mut(&path("a")).unwrap().revision = Some("invalid-token".into());
    let result = proposal.accept();
    assert!(
        result
            .error
            .as_deref()
            .unwrap()
            .contains("baseline conflict")
    );
    assert!(result.applied.is_empty());
    assert_eq!(fixture.read_file("a"), "original");
    proposal.files.get_mut(&path("a")).unwrap().revision = None;
    let result = proposal.accept();
    assert!(
        result
            .error
            .as_deref()
            .unwrap()
            .contains("replacement blocked")
    );
    assert!(result.applied.is_empty());
    assert_eq!(fixture.read_file("a"), "original");
    proposal.files.get_mut(&path("a")).unwrap().revision = Some(revision);
    let result = proposal.accept();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.applied, vec![path("a")]);
    assert_eq!(fixture.read_file("a"), "proposed");
}

#[test]
fn stale_baseline_and_discard_preserve_external_edits() {
    let fixture = TempWorkspace::new("proposal-stale");
    fixture.write_file("a", "original");
    let mut proposal = snapshot(&fixture, &["a"]);
    proposal.replace(&path("a"), "proposed".into(), 0).unwrap();
    fixture.write_file("a", "external edits");
    assert!(proposal.accept().error.is_some());
    proposal.discard();
    assert!(proposal.read(&path("a")).is_err());
    assert!(proposal.create(&path("a"), "bad".into()).is_err());
    assert_eq!(fixture.read_file("a"), "external edits");
}

#[test]
fn exact_approval_exclusions_binary_and_budgets() {
    let fixture = TempWorkspace::new("proposal-bounds");
    fixture.write_file("binary", "\0");
    fixture.create_dir("target");
    fixture.create_dir(".git");
    for name in [
        ".",
        "binary",
        "target/new",
        ".git/config",
        "build/new",
        "node_modules/new",
        "missing/new",
        "binary/child",
        "target",
    ] {
        assert!(
            Proposal::new(
                &fixture.workspace(),
                &[path(name)],
                ProposalLimits::default()
            )
            .is_err()
        );
    }
    for name in ["../escape", "/absolute", "dir\\file"] {
        assert!(ScopePath::new(name).is_err());
    }
    let mut proposal = Proposal::new(
        &fixture.workspace(),
        &[path("new")],
        ProposalLimits {
            max_files: 1,
            max_file_bytes: 3,
            max_total_bytes: 3,
        },
    )
    .unwrap();
    assert!(proposal.create(&path("other"), "x".into()).is_err());
    assert!(proposal.create(&path("new"), "four".into()).is_err());
    assert!(proposal.create(&path("new"), "\0".into()).is_err());
    assert!(proposal.changes().unwrap().is_empty());
}

#[test]
fn snapshot_limits_and_preflight_growth_are_bounded() {
    let fixture = TempWorkspace::new("proposal-read-bounds");
    fixture.write_file("a", "abc");
    fixture.write_file("b", "def");
    fixture.write_bytes("invalid", &[0xff]);
    let limits = ProposalLimits {
        max_files: 2,
        max_file_bytes: 3,
        max_total_bytes: 5,
    };
    let workspace = fixture.workspace();
    for paths in [
        vec![path("a"), path("b")],
        vec![path("a"), path("a")],
        vec![path("invalid")],
    ] {
        assert!(Proposal::new(&workspace, &paths, limits).is_err());
    }
    assert!(
        Proposal::new(
            &workspace,
            &[path("a")],
            ProposalLimits {
                max_files: 0,
                ..limits
            }
        )
        .is_err()
    );
    fixture.write_file("large", "abcd");
    assert!(Proposal::new(&workspace, &[path("large")], limits).is_err());
    let mut proposal = Proposal::new(&workspace, &[path("a"), path("new")], limits).unwrap();
    proposal.replace(&path("a"), "xyz".into(), 0).unwrap();
    assert!(proposal.create(&path("new"), "abc".into()).is_err());
    proposal.create(&path("new"), "de".into()).unwrap();
    fixture.write_file("a", "grown beyond budget");
    let result = proposal.accept();
    assert!(result.error.is_some());
    assert!(result.applied.is_empty());
    assert!(!fixture.host_path_exists("new"));
}

#[cfg(unix)]
#[test]
fn retained_grant_survives_root_replacement_before_snapshot_and_acceptance() {
    let fixture = TempWorkspace::new("proposal-retained");
    fixture.write_file("grant/a", "original");
    let workspace = Workspace::new(fixture.root().join("grant").to_string_lossy()).unwrap();
    std::fs::rename(fixture.root().join("grant"), fixture.root().join("moved")).unwrap();
    fixture.write_file("grant/a", "decoy");
    let mut proposal = Proposal::new(
        &workspace,
        &[path("a"), path("new")],
        ProposalLimits::default(),
    )
    .unwrap();
    assert_eq!(proposal.read(&path("a")).unwrap().0, "original");
    drop(workspace);
    std::fs::rename(
        fixture.root().join("moved"),
        fixture.root().join("moved-again"),
    )
    .unwrap();
    proposal.replace(&path("a"), "saved".into(), 0).unwrap();
    proposal.create(&path("new"), "created".into()).unwrap();
    let result = proposal.accept();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.applied, vec![path("a"), path("new")]);
    assert_eq!(fixture.read_file("grant/a"), "decoy");
    assert!(!fixture.host_path_exists("grant/new"));
    assert_eq!(fixture.read_file("moved-again/a"), "saved");
    assert_eq!(fixture.read_file("moved-again/new"), "created");
}

#[cfg(unix)]
#[test]
fn replacement_does_not_truncate_other_hardlinks() {
    let fixture = TempWorkspace::new("proposal-hardlink");
    fixture.write_file("a", "original");
    std::fs::hard_link(fixture.root().join("a"), fixture.root().join("alias")).unwrap();
    let mut proposal = snapshot(&fixture, &["a"]);
    proposal
        .replace(&path("a"), "replacement".into(), 0)
        .unwrap();
    let result = proposal.accept();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(fixture.read_file("a"), "replacement");
    assert_eq!(fixture.read_file("alias"), "original");
}

#[cfg(unix)]
#[test]
fn publication_failure_reports_only_successes_and_can_retry() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = TempWorkspace::new("proposal-publication");
    fixture.write_file("z", "original");
    std::fs::set_permissions(
        fixture.root().join("z"),
        std::fs::Permissions::from_mode(0o444),
    )
    .unwrap();
    let mut proposal = snapshot(&fixture, &["a", "z"]);
    proposal.create(&path("a"), "created".into()).unwrap();
    proposal.replace(&path("z"), "saved".into(), 0).unwrap();
    let result = proposal.accept();
    assert!(result.error.is_some());
    assert_eq!(result.applied, vec![path("a")]);
    assert_eq!(fixture.read_file("a"), "created");
    assert_eq!(fixture.read_file("z"), "original");
    assert_eq!(
        proposal
            .changes()
            .unwrap()
            .iter()
            .map(|change| change.path.clone())
            .collect::<Vec<_>>(),
        vec![path("z")]
    );
    std::fs::set_permissions(
        fixture.root().join("z"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let result = proposal.accept();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.applied, vec![path("z")]);
    assert_eq!(fixture.read_file("z"), "saved");
}

#[test]
fn app_acceptance_preserves_dirty_buffer() {
    use crate::{
        app::App,
        text::{Position, TextEdit, TextRange},
    };
    let fixture = TempWorkspace::new("proposal-dirty");
    fixture.write_file("a", "original");
    let mut app = App::new(fixture.workspace()).unwrap();
    app.open_path("a").unwrap();
    app.open_document_mut()
        .unwrap()
        .apply_edit(TextEdit::replace(
            TextRange::new(Position::new(0, 0), Position::new(0, 0)),
            "unsaved ",
        ))
        .unwrap();
    let mut proposal = app
        .proposal(&[path("a")], ProposalLimits::default())
        .unwrap();
    assert_eq!(proposal.read(&path("a")).unwrap().0, "original");
    proposal.replace(&path("a"), "proposed".into(), 0).unwrap();
    assert!(app.accept_proposal(&mut proposal).error.is_some());
    assert_eq!(app.open_document().unwrap().text(), "unsaved original");
    assert_eq!(fixture.read_file("a"), "original");
}

#[cfg(feature = "agent")]
#[test]
fn bounded_loop_changes_only_proposal_and_cannot_accept() {
    use crate::agent::{AgentLimits, AgentStop, DecisionMessage, DecisionModel, run_session};
    use crate::tool::{AgentSession, ProposalToolProvider};
    struct Model(std::collections::VecDeque<String>);
    impl DecisionModel for Model {
        fn decide(&mut self, _: &[DecisionMessage], _: u32) -> Result<String, String> {
            self.0
                .pop_front()
                .ok_or_else(|| "unexpected decision".into())
        }
    }
    let fixture = TempWorkspace::new("proposal-loop");
    let mut proposal = snapshot(&fixture, &["new"]);
    let mut model = Model([
        r#"{"type":"tool","name":"scope.document.create","arguments":{"path":"new","text":"created"}}"#.into(),
        r#"{"type":"tool","name":"scope.proposal.accept","arguments":{}}"#.into(),
        r#"{"type":"complete","content":"Ready for human review"}"#.into(),
    ].into());
    {
        let mut session = AgentSession::builder()
            .provider(ProposalToolProvider::new(&mut proposal))
            .build()
            .unwrap();
        let result = run_session(
            &mut model,
            &mut session,
            "Create new",
            AgentLimits::default(),
            |_, _| true,
        )
        .unwrap();
        assert_eq!(result.stop, AgentStop::Completed);
        assert_eq!(result.tool_calls, 2);
    }
    assert!(!fixture.host_path_exists("new"));
    assert_eq!(proposal.changes().unwrap().len(), 1);
}

#[test]
fn read_only_baseline_cannot_be_accepted() {
    let fixture = TempWorkspace::new("proposal-readonly");
    let workspace = Workspace::read_only(fixture.root().to_string_lossy()).unwrap();
    let mut proposal =
        Proposal::new(&workspace, &[path("new")], ProposalLimits::default()).unwrap();
    proposal.create(&path("new"), "text".into()).unwrap();
    assert!(proposal.accept().error.is_some());
    assert!(!fixture.host_path_exists("new"));
}

#[cfg(unix)]
#[test]
fn rejects_leaf_parent_and_late_symlinks() {
    let fixture = TempWorkspace::new("proposal-symlink");
    fixture.write_file("real/a", "original");
    fixture.symlink("real/a", "leaf");
    fixture.symlink("real", "parent");
    fixture.symlink("missing", "dangling");
    for name in ["leaf", "parent/a", "parent/new", "dangling"] {
        assert!(
            Proposal::new(
                &fixture.workspace(),
                &[path(name)],
                ProposalLimits::default()
            )
            .is_err()
        );
    }
    let mut proposal = snapshot(&fixture, &["new"]);
    proposal.create(&path("new"), "proposed".into()).unwrap();
    fixture.symlink("real/a", "new");
    assert!(proposal.accept().error.is_some());
    assert_eq!(fixture.read_file("real/a"), "original");
}

#[cfg(feature = "tools")]
#[test]
fn proposal_provider_checked_creation_replacement_and_no_accept_tool() {
    use crate::tool::{AgentSession, ProposalToolProvider, ToolCall, ToolValue};
    let fixture = TempWorkspace::new("proposal-tools");
    let mut proposal = snapshot(&fixture, &["new"]);
    {
        let mut session = AgentSession::builder()
            .provider(ProposalToolProvider::new(&mut proposal))
            .build()
            .unwrap();
        assert!(session.catalog().lookup("scope.proposal.accept").is_none());
        let create = || {
            ToolCall::new("scope.document.create")
                .with_argument("path", "new".into())
                .with_argument("text", "old".into())
        };
        assert!(session.invoke(create()).is_ok());
        assert!(session.invoke(create()).is_err());
        assert!(
            session
                .invoke(
                    ToolCall::new("scope.document.create")
                        .with_argument("path", "../escape".into())
                        .with_argument("text", "x".into())
                )
                .is_err()
        );
        let replace = ToolCall::new("scope.document.replace")
            .with_argument("path", "new".into())
            .with_argument("expected_revision", ToolValue::Integer(1))
            .with_argument("start_line", ToolValue::Integer(0))
            .with_argument("start_column", ToolValue::Integer(0))
            .with_argument("end_line", ToolValue::Integer(0))
            .with_argument("end_column", ToolValue::Integer(3))
            .with_argument("replacement", "new text".into());
        session
            .invoke(replace)
            .expect("replace the full zero-based range of 'old'");
        assert!(
            session
                .invoke(ToolCall::new("scope.proposal.accept"))
                .is_err()
        );
    }
    assert!(!fixture.host_path_exists("new"));
    assert_eq!(proposal.read(&path("new")).unwrap().0, "new text");
}
