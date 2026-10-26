#![cfg(feature = "agent")]

use scope_ide::{
    agent::{
        DecisionMessage, DecisionModel, EditCapabilities, EditOutcome, SemanticEditOptions,
        run_semantic_edit,
    },
    app::App,
    cli::workspace_command::WorkspaceCommandOutput,
    language::InteractiveSession,
    proposal::ProposalStatus,
    scratch::ScratchBuffers,
    source_edit::{AgentEditRequest, SourceId},
    text::{Position, TextRange},
    workspace::Workspace,
};
use scope_language::{
    LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, SemanticArgument,
    SemanticArgumentKind, SemanticEdit, SemanticOperationDescription, SemanticOperationEffect,
    SemanticOperationError, SemanticOperationRequest, SemanticOperationResult,
    SemanticTargetRequirement,
};
use serde_json::json;
use std::{
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};

const OPERATION: &str = "language.bridge.replace";

#[derive(Default)]
struct Service {
    dispatches: AtomicUsize,
}

impl LanguageService for Service {
    fn language_id(&self) -> &'static str {
        "bridge"
    }
    fn language_name(&self) -> &'static str {
        "Bridge fixture"
    }
    fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
        true
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        Default::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        vec![SemanticOperationDescription {
            id: "replace".into(),
            description: "Replace source".into(),
            arguments: vec![SemanticArgument {
                name: "candidate".into(),
                kind: SemanticArgumentKind::String,
                required: true,
                description: "Replacement source".into(),
            }],
            effect: SemanticOperationEffect::ProposeChanges,
            target: SemanticTargetRequirement::Document,
            constraints: None,
        }]
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        self.dispatches.fetch_add(1, Ordering::SeqCst);
        Ok(SemanticOperationResult::Changes {
            edits: vec![SemanticEdit {
                range: TextRange::new(Position::new(0, 0), Position::new(0, 3)),
                replacement: request.arguments["candidate"].as_str().unwrap().into(),
            }],
            rationale: "Requested replacement".into(),
        })
    }
}

#[derive(Default)]
struct Model {
    calls: usize,
}
impl DecisionModel for Model {
    fn decide(&mut self, _: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.calls += 1;
        assert_eq!(self.calls, 1, "unexpected additional model invocation");
        Ok(json!({"type":"tool", "name":OPERATION, "arguments":{"candidate":"new"}}).to_string())
    }
}

fn exercise_bridge(authorization_limit: usize) {
    let authorized = authorization_limit >= 2;
    let mut app = App::new(Workspace::read_only(env!("CARGO_MANIFEST_DIR")).unwrap()).unwrap();
    let mut session = InteractiveSession::new();
    let mut sources = ScratchBuffers::default();
    let target_id = sources.create("bridge", "old");
    let mut output = WorkspaceCommandOutput::default();
    let mut model = Model::default();
    let service = Service::default();
    let authorizations = Cell::new(0);
    let options = SemanticEditOptions {
        capabilities: EditCapabilities {
            propose: vec![OPERATION.into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let target = sources.get(target_id).unwrap();
    let run = run_semantic_edit(
        &mut model,
        output.source_edit_controller(),
        target,
        &service,
        AgentEditRequest::new(target, "Replace old with new"),
        &options,
        |authorization| {
            authorizations.set(authorizations.get() + 1);
            assert_eq!(authorization.operation.id, "replace");
            assert_eq!(
                authorization.operation.effect,
                SemanticOperationEffect::ProposeChanges
            );
            assert_eq!(authorization.arguments, &json!({"candidate":"new"}));
            authorizations.get() <= authorization_limit
        },
    );
    assert_eq!(model.calls, 1);
    assert_eq!(
        authorizations.get(),
        if authorization_limit > 0 { 2 } else { 1 }
    );
    assert_eq!(target.text(), "old");
    assert_eq!(target.version(), 0);

    if !authorized {
        assert_eq!(run.outcome, EditOutcome::AuthorizationFailure);
        assert!(run.proposal.is_none());
        assert_eq!(
            service.dispatches.load(Ordering::SeqCst),
            usize::from(authorization_limit > 0)
        );
        assert_eq!(output.source_edit_controller().list().count(), 0);
        output
            .submit(
                ":scope source-proposals",
                &mut app,
                &mut session,
                None,
                &mut sources,
            )
            .unwrap();
        assert!(String::from_utf8_lossy(output.bytes()).contains("0 pending source proposals"));
        assert_eq!(sources.get(target_id).unwrap().text(), "old");
        assert_eq!(sources.get(target_id).unwrap().version(), 0);
        return;
    }

    assert_eq!(run.outcome, EditOutcome::ProposalReady);
    assert_eq!(service.dispatches.load(Ordering::SeqCst), 1);
    let id = run.proposal.unwrap();
    let proposal = output.source_edit_controller().get(id).unwrap();
    assert_eq!(proposal.status(), ProposalStatus::Pending);
    assert_eq!(proposal.target(), SourceId::Scratch(target_id));
    assert_eq!(proposal.expected_source(), "old");
    assert_eq!(proposal.expected_version(), 0);
    let other_id = sources.create("bridge", "other");
    output.clear();
    output
        .submit(
            ":scope source-proposals",
            &mut app,
            &mut session,
            None,
            &mut sources,
        )
        .unwrap();
    assert!(String::from_utf8_lossy(output.bytes()).contains("1 pending source proposals"));
    output.clear();
    output
        .submit(
            &format!(":scope source-diff {}", id.0),
            &mut app,
            &mut session,
            None,
            &mut sources,
        )
        .unwrap();
    let diff = String::from_utf8_lossy(output.bytes());
    assert!(diff.contains("-old"), "{diff}");
    assert!(diff.contains("+new"), "{diff}");
    assert_eq!(sources.get(target_id).unwrap().text(), "old");
    assert_eq!(sources.get(target_id).unwrap().version(), 0);
    output.clear();
    output
        .submit(
            &format!(":scope source-accept {}", id.0),
            &mut app,
            &mut session,
            None,
            &mut sources,
        )
        .unwrap();
    assert_eq!(sources.get(target_id).unwrap().text(), "new");
    assert_eq!(sources.get(target_id).unwrap().version(), 1);
    assert!(sources.get(target_id).unwrap().is_dirty());
    assert_eq!(sources.selected().unwrap().id(), other_id);
    assert_eq!(sources.get(other_id).unwrap().text(), "other");
    assert_eq!(sources.get(other_id).unwrap().version(), 0);
    assert_eq!(
        output.source_edit_controller().get(id).unwrap().status(),
        ProposalStatus::Applied
    );
    assert_eq!(model.calls, 1);
    assert_eq!(service.dispatches.load(Ordering::SeqCst), 1);
}

#[test]
fn authorized_semantic_proposal_is_reviewed_and_accepted_by_retained_commands() {
    exercise_bridge(2);
}

#[test]
fn denied_concrete_semantic_authorization_leaves_no_pending_proposal_or_source_edit() {
    exercise_bridge(0);
}

#[test]
fn revoked_authorization_after_dispatch_prevents_proposal_registration() {
    exercise_bridge(1);
}
