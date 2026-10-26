//! W8b: compile and exercise configured editing exclusively through public APIs.
#![cfg(feature = "agent")]

#[path = "support/configured_editing.rs"]
mod configured_editing;

use scope_ide::{
    agent::{DecisionMessage, DecisionModel, EditOutcome, run_semantic_edit},
    app::App,
    cli::{self, ReadOnlyCommand, workspace_command::WorkspaceCommandOutput},
    document::Document,
    language::{InteractiveSession, LanguageRegistry},
    proposal::{ProposalId, ProposalStatus},
    scratch::ScratchBuffers,
    source_edit::AgentEditRequest,
    text::{Position, TextEdit, TextRange},
    workspace::Workspace,
};
use std::{error::Error, io::Write};

struct UnexpectedHost;
impl ReadOnlyCommand for UnexpectedHost {
    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn Error>> {
        panic!("configured startup must not invoke the host")
    }
}

#[test]
fn combined_launcher_preserves_help_and_argument_errors() {
    cli::run_with_registry_and_command_and_editing(
        ["--workspace", "", "--help"].map(String::from),
        LanguageRegistry::builtins(),
        &mut UnexpectedHost,
        configured_editing::config(),
    )
    .unwrap();
    let args = ["--workspace"].map(String::from);
    let existing = cli::run_with_registry_and_command(
        args.clone(),
        LanguageRegistry::builtins(),
        &mut UnexpectedHost,
    )
    .unwrap_err();
    let configured = cli::run_with_registry_and_command_and_editing(
        args,
        LanguageRegistry::builtins(),
        &mut UnexpectedHost,
        configured_editing::config(),
    )
    .unwrap_err();
    assert_eq!(configured.to_string(), existing.to_string());
}

#[test]
fn retained_constructor_rejects_unaccepted_host_contract() {
    let mut editing = configured_editing::config();
    editing
        .task
        .criteria
        .get_mut("exact-source")
        .unwrap()
        .acceptance = scope_ide::task_contract::Acceptance::Provisional;
    assert!(WorkspaceCommandOutput::with_editing(editing).is_err());
}

struct ReplaceOnce(usize);
impl DecisionModel for ReplaceOnce {
    fn decide(&mut self, _: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.0 += 1;
        assert_eq!(self.0, 1, "no default generator or correction is needed");
        Ok(r#"{"type":"tool","name":"language.configured-fixture.replace","arguments":{"candidate":"new"}}"#.into())
    }
}

#[test]
fn retained_configured_consumer_reviews_accepts_and_rejects_stale_contracted_edits() {
    for stale in [false, true] {
        let editing = configured_editing::config();
        let service = editing.service;
        let options = editing.options;
        let task = editing.task.clone();
        let mut authorize = editing.authorize;
        // Use an independent config so its authorization closure remains owned by the session.
        let mut output =
            WorkspaceCommandOutput::with_editing(configured_editing::config()).unwrap();
        let mut sources = ScratchBuffers::default();
        sources.create("configured-fixture", "old");
        let source = sources.selected().unwrap();
        let mut model = ReplaceOnce(0);
        let run = run_semantic_edit(
            &mut model,
            output.source_edit_controller(),
            source,
            service,
            AgentEditRequest::new(source, "replace old with new"),
            &options,
            &mut authorize,
        );
        assert_eq!(run.outcome, EditOutcome::ProposalReady);
        assert_eq!(model.0, 1);
        let id = run.proposal.unwrap();
        assert_eq!(id, ProposalId(1));
        let proposal = output.source_edit_controller().get(id).unwrap();
        assert_eq!(proposal.status(), ProposalStatus::Pending);
        assert_eq!(proposal.task_contract(), Some(&task));
        let evidence = proposal
            .evidence()
            .expect("host contract must produce retained evidence");
        assert_eq!(
            (evidence.original.as_str(), evidence.candidate.as_str()),
            ("old", "new")
        );
        assert_eq!(
            evidence.observations.checks["exact-source"].outcome,
            scope_language::CheckOutcome::Passed
        );
        assert_eq!(sources.selected().unwrap().text(), "old");
        let mut app = App::new(Workspace::read_only(env!("CARGO_MANIFEST_DIR")).unwrap()).unwrap();
        let mut session = InteractiveSession::new();
        for command in [":scope source-proposal 1", ":scope source-diff 1"] {
            output
                .submit(command, &mut app, &mut session, None, &mut sources)
                .unwrap();
        }
        assert_eq!(sources.selected().unwrap().version(), 0);
        output.clear();
        assert!(output.bytes().is_empty());
        assert_eq!(
            output.source_edit_controller().get(id).unwrap().status(),
            ProposalStatus::Pending
        );
        if stale {
            sources
                .selected_mut()
                .unwrap()
                .apply_edit(TextEdit::insert(Position::new(0, 3), "!"))
                .unwrap();
        }
        output
            .submit(
                ":scope source-accept 1",
                &mut app,
                &mut session,
                None,
                &mut sources,
            )
            .unwrap();
        let source = sources.selected().unwrap();
        assert_eq!(source.text(), if stale { "old!" } else { "new" });
        assert_eq!(source.version(), 1);
        assert!(source.destination_path().is_none());
        let proposal = output.source_edit_controller().get(id).unwrap();
        assert_eq!(
            proposal.status(),
            if stale {
                ProposalStatus::Pending
            } else {
                ProposalStatus::Applied
            }
        );
        assert!(proposal.evidence().is_some());
        let transcript = String::from_utf8_lossy(output.bytes());
        assert!(
            transcript.contains(if stale {
                "RevisionConflict"
            } else {
                "not re-evaluated on acceptance"
            }),
            "{transcript}"
        );
    }
}
