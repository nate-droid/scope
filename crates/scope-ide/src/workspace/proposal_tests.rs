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
    for name in ["binary", "target/new", ".git/config", "missing/new"] {
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
