use super::*;
use crate::agent::{DecisionMessage, DecisionModel, EditCapabilities, SemanticEditOptions};
use crate::source_edit::SourceTarget;
use crate::task_contract::{Acceptance, AssurancePolicy, TaskContract, TaskCriterion};
use scope_language::*;
use serde_json::json;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct Service {
    dispatches: AtomicUsize,
    validations: AtomicUsize,
    omit_mandatory: bool,
}

impl LanguageService for Service {
    fn language_id(&self) -> &'static str {
        "configured-fixture"
    }
    fn language_name(&self) -> &'static str {
        "Configured fixture"
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
            description: "Replace the document".into(),
            effect: SemanticOperationEffect::ProposeChanges,
            target: SemanticTargetRequirement::Document,
            arguments: vec![SemanticArgument {
                name: "candidate".into(),
                kind: SemanticArgumentKind::String,
                required: true,
                description: "Candidate source".into(),
            }],
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
                range: crate::text::TextRange::new(Position::new(0, 0), Position::new(0, 3)),
                replacement: request.arguments["candidate"].as_str().unwrap().into(),
            }],
            rationale: "Fixture replacement".into(),
        })
    }
    fn validate_semantic_contract(
        &self,
        request: SemanticValidationRequest<'_>,
        contract: &ValidationContract,
    ) -> Result<ContractValidation, SemanticOperationError> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.document.text(), "old");
        assert_eq!(request.document.version(), 0);
        assert_eq!(request.operation_id, "replace");
        assert_eq!(
            request.candidate_source,
            request.arguments["candidate"].as_str().unwrap()
        );
        Ok(ContractValidation {
            checks: contract
                .checks
                .iter()
                .filter(|(id, _)| {
                    id.as_str() != "optional"
                        && !(self.omit_mandatory && id.as_str() == "exact-source")
                })
                .map(|(id, expected)| {
                    let actual = json!(request.candidate_source);
                    let outcome = if request.candidate_source == "inconclusive" {
                        CheckOutcome::Inconclusive
                    } else if actual == *expected {
                        CheckOutcome::Passed
                    } else {
                        CheckOutcome::Failed
                    };
                    (id.clone(), CheckObservation { outcome, actual })
                })
                .collect(),
        })
    }
}

struct Model {
    decisions: VecDeque<String>,
    calls: usize,
}
impl DecisionModel for Model {
    fn decide(&mut self, _: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.calls += 1;
        Ok(self
            .decisions
            .pop_front()
            .expect("unexpected model invocation"))
    }
}
fn model(buffers: &ScratchBuffers, candidates: &[&str]) -> Model {
    let buffer = buffers.selected().unwrap();
    let id = buffer.source_id();
    assert_eq!(buffer.version(), 0);
    Model {
        calls: 0,
        decisions: candidates
            .iter()
            .map(|candidate| {
                // The tool is document-bound by the host; its envelope has no target-ID field.
                assert_eq!(buffers.selected().unwrap().source_id(), id);
                json!({"type":"tool", "name":"language.configured-fixture.replace", "arguments":{
                "expected_version":buffer.version().to_string(), "expected_source":buffer.text(),
                "arguments":{"candidate":candidate}
            }}).to_string()
            })
            .collect(),
    }
}
fn buffers(language: &str) -> ScratchBuffers {
    let mut buffers = ScratchBuffers::default();
    buffers.create(language, "old");
    buffers
}
fn config() -> InteractiveEditConfig {
    InteractiveEditConfig {
        service: Box::leak(Box::new(Service::default())),
        task: TaskContract {
            original_request: "replace old with new".into(),
            goal: "new source".into(),
            policy: AssurancePolicy::Strict,
            criteria: [(
                "exact-source".into(),
                TaskCriterion {
                    expectation: json!("new"),
                    origin: ExpectationOrigin::Project("fixture-policy".into()),
                    acceptance: Acceptance::Accepted {
                        consumer: "host".into(),
                    },
                    mandatory: true,
                },
            )]
            .into(),
        },
        options: SemanticEditOptions {
            capabilities: EditCapabilities {
                propose: vec!["language.configured-fixture.replace".into()],
                inspect_first: false,
                ..Default::default()
            },
            ..Default::default()
        },
        authorize: Box::new(|_| true),
    }
}
fn unchanged(buffers: &ScratchBuffers) {
    let buffer = buffers.selected().unwrap();
    assert_eq!(buffer.text(), "old");
    assert_eq!(buffer.version(), 0);
    assert!(buffer.destination_path().is_none());
}

#[test]
fn configured_success_retains_pending_bound_evidence_and_host_provenance() {
    let buffers = buffers("configured-fixture");
    let config = config();
    let task = config.task.clone();
    let mut proposals = Proposals::configured(Some(config)).unwrap();
    let mut model = model(&buffers, &["new"]);
    let mut output = Vec::new();
    let super::super::ShellCommand::Source(Command::AgentEdit(request)) =
        super::super::ShellCommand::parse("agent edit \"replace\"").unwrap()
    else {
        panic!("expected the existing interactive editing command")
    };
    proposals
        .generate(&mut model, &request, &buffers, &mut output)
        .unwrap();
    assert_eq!(model.calls, 1);
    unchanged(&buffers);
    let proposal = proposals.controller.get(ProposalId(1)).unwrap();
    assert_eq!(proposal.status(), ProposalStatus::Pending);
    assert_eq!(proposal.task_contract(), Some(&task));
    let evidence = proposal.evidence().unwrap();
    assert_eq!(evidence.target, buffers.selected().unwrap().source_id());
    assert_eq!(evidence.version, 0);
    assert_eq!(evidence.original, "old");
    assert_eq!(evidence.candidate, "new");
    assert_eq!(evidence.operation, "replace");
    assert_eq!(evidence.arguments, json!({"candidate":"new"}));
    assert_eq!(evidence.contract, task.validation_contract());
    assert_eq!(
        evidence.observations.checks["exact-source"].outcome,
        CheckOutcome::Passed
    );
    assert_eq!(
        evidence.observations.checks["exact-source"].actual,
        json!("new")
    );
    assert!(proposal.unresolved_obligations().is_empty());
    let output = String::from_utf8(output).unwrap();
    for text in [
        "pending",
        "source unchanged",
        "not saved",
        "not universal correctness",
        "Passed",
    ] {
        assert!(output.contains(text), "{output}");
    }
}

#[test]
fn explicit_acceptance_retains_candidate_evidence_without_saving_or_reevaluating() {
    let mut buffers = buffers("configured-fixture");
    let mut proposals = Proposals::configured(Some(config())).unwrap();
    let mut model = model(&buffers, &["new"]);
    let mut output = Vec::new();
    proposals
        .generate(&mut model, "replace", &buffers, &mut output)
        .unwrap();
    unchanged(&buffers);
    proposals
        .run(Command::Review(ProposalId(1)), &mut buffers, &mut output)
        .unwrap();
    proposals
        .run(Command::Diff(ProposalId(1)), &mut buffers, &mut output)
        .unwrap();
    unchanged(&buffers);
    output.clear();
    proposals
        .run(Command::Accept(ProposalId(1)), &mut buffers, &mut output)
        .unwrap();
    assert_eq!(buffers.selected().unwrap().text(), "new");
    assert!(buffers.selected().unwrap().is_dirty());
    assert!(buffers.selected().unwrap().destination_path().is_none());
    assert_eq!(model.calls, 1);
    let proposal = proposals.controller.get(ProposalId(1)).unwrap();
    assert_eq!(proposal.status(), ProposalStatus::Applied);
    assert_eq!(proposal.evidence().unwrap().candidate, "new");
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("not saved; candidate observations retained"));
    assert!(output.contains("not re-evaluated on acceptance"));
}

#[test]
fn optional_missing_observations_are_reported_as_not_evaluated() {
    let buffers = buffers("configured-fixture");
    let mut config = config();
    config.task.criteria.insert(
        "optional".into(),
        TaskCriterion {
            expectation: json!("unimplemented"),
            origin: ExpectationOrigin::Project("fixture-policy".into()),
            acceptance: Acceptance::Provisional,
            mandatory: false,
        },
    );
    let mut proposals = Proposals::configured(Some(config)).unwrap();
    let mut model = model(&buffers, &["new"]);
    let mut output = Vec::new();
    proposals
        .generate(&mut model, "replace", &buffers, &mut output)
        .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(
        output.contains("\"optional\": NotEvaluated (accepted: false, mandatory: false)"),
        "{output}"
    );
}

#[test]
fn manual_unevaluated_proposal_cannot_bypass_configured_controller() {
    let mut buffers = buffers("configured-fixture");
    let mut proposals = Proposals::configured(Some(config())).unwrap();
    let mut output = Vec::new();
    let payload = json!({"edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":3}},"replacement":"new"}]}).to_string();
    assert!(
        proposals
            .run(Command::Propose(payload), &mut buffers, &mut output)
            .is_err()
    );
    assert!(proposals.controller.list().next().is_none());
    assert!(output.is_empty());
    unchanged(&buffers);
}

#[test]
fn authorization_denial_never_registers_or_applies() {
    let buffers = buffers("configured-fixture");
    let mut config = config();
    let service = Box::leak(Box::new(Service::default()));
    config.service = service;
    config.authorize = Box::new(|call| {
        assert_eq!(call.operation.id, "replace");
        assert_eq!(call.source.text, "old");
        assert_eq!(call.source.version, 0);
        assert_eq!(call.arguments, &json!({"candidate":"new"}));
        false
    });
    let mut proposals = Proposals::configured(Some(config)).unwrap();
    let mut model = model(&buffers, &["new"]);
    let mut output = Vec::new();
    let error = proposals
        .generate(&mut model, "replace", &buffers, &mut output)
        .unwrap_err();
    assert!(error.to_string().contains("AuthorizationFailure"));
    assert_eq!(service.dispatches.load(Ordering::SeqCst), 0);
    assert_eq!(service.validations.load(Ordering::SeqCst), 0);
    assert_eq!(proposals.controller.list().count(), 0);
    assert!(output.is_empty());
    unchanged(&buffers);
}

#[test]
fn missing_mandatory_evidence_is_not_equivalent_to_passing_checks() {
    for include_other_check in [false, true] {
        let buffers = buffers("configured-fixture");
        let mut config = config();
        config.service = Box::leak(Box::new(Service {
            omit_mandatory: true,
            ..Default::default()
        }));
        config.options.budgets.corrections = 0;
        if include_other_check {
            config.task.criteria.insert(
                "other".into(),
                TaskCriterion {
                    expectation: json!("new"),
                    origin: ExpectationOrigin::Project("fixture-policy".into()),
                    acceptance: Acceptance::Provisional,
                    mandatory: false,
                },
            );
        }
        let mut proposals = Proposals::configured(Some(config)).unwrap();
        let mut model = model(&buffers, &["new"]);
        let mut output = Vec::new();
        let error = proposals
            .generate(&mut model, "replace", &buffers, &mut output)
            .unwrap_err();
        assert!(error.to_string().contains("ValidationFailure"), "{error}");
        assert!(proposals.controller.list().next().is_none());
        assert!(output.is_empty());
        unchanged(&buffers);
    }
}

#[test]
fn strict_failed_and_inconclusive_checks_do_not_claim_readiness() {
    for candidate in ["bad", "inconclusive"] {
        let buffers = buffers("configured-fixture");
        let mut config = config();
        config.options.budgets.corrections = 0;
        let mut proposals = Proposals::configured(Some(config)).unwrap();
        let mut model = model(&buffers, &[candidate]);
        let mut output = Vec::new();
        let error = proposals
            .generate(&mut model, "replace", &buffers, &mut output)
            .unwrap_err();
        assert!(error.to_string().contains("ValidationFailure"), "{error}");
        assert_eq!(model.calls, 1);
        assert_eq!(proposals.controller.list().count(), 0);
        assert!(output.is_empty());
        unchanged(&buffers);
    }
}

#[test]
fn correction_budget_is_shared_and_exhaustion_never_falls_back() {
    for (second, succeeds) in [("new", true), ("still-wrong", false)] {
        let buffers = buffers("configured-fixture");
        let mut config = config();
        config.options.budgets.corrections = 1;
        let mut proposals = Proposals::configured(Some(config)).unwrap();
        let mut model = model(&buffers, &["bad", second]);
        let mut output = Vec::new();
        let result = proposals.generate(&mut model, "replace", &buffers, &mut output);
        assert_eq!(model.calls, 2);
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(proposals.controller.list().count(), usize::from(succeeds));
        if !succeeds {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("ValidationFailure"), "{error}");
            assert!(error.contains("1 corrections"), "{error}");
            assert!(output.is_empty());
        }
        unchanged(&buffers);
    }
}

#[test]
fn configured_shell_reports_incompatibility_without_connecting_to_model_or_saving() {
    let fixture = crate::test_support::TempWorkspace::new("configured-shell");
    fixture.write_file("sentinel", "old");
    let app = crate::app::App::new(fixture.workspace()).unwrap();
    let mut output = Vec::new();
    super::super::run_shell_configured(
        app,
        crate::agent::AgentConfig::local("unused", "not-a-model-url"),
        std::io::Cursor::new(concat!(
            ":scope scratch new other\n",
            ":scope agent edit \"replace\"\n",
            "source-proposals\nquit\n",
        )),
        &mut output,
        false,
        Some(config()),
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(
        output.contains("host-configured contract required"),
        "{output}"
    );
    assert!(
        output.contains("matching the host language service"),
        "{output}"
    );
    assert!(output.contains("source unchanged"), "{output}");
    assert!(output.contains("0 pending source proposals"), "{output}");
    assert!(!output.contains("ready; pending"), "{output}");
    assert_eq!(fixture.read_file("sentinel"), "old");
}

#[test]
fn model_budget_exhaustion_does_not_consume_a_decision_or_create_a_proposal() {
    let buffers = buffers("configured-fixture");
    let mut config = config();
    config.options.budgets.model_invocations = 0;
    let mut proposals = Proposals::configured(Some(config)).unwrap();
    let mut model = model(&buffers, &["new"]);
    let mut output = Vec::new();
    assert!(
        proposals
            .generate(&mut model, "replace", &buffers, &mut output)
            .is_err()
    );
    assert_eq!(model.calls, 0);
    assert_eq!(model.decisions.len(), 1);
    assert_eq!(proposals.controller.list().count(), 0);
    assert!(output.is_empty());
    unchanged(&buffers);
}

#[test]
fn stale_configured_proposal_cannot_overwrite_intervening_edit() {
    let mut buffers = buffers("configured-fixture");
    let mut proposals = Proposals::configured(Some(config())).unwrap();
    let mut model = model(&buffers, &["new"]);
    let mut output = Vec::new();
    proposals
        .generate(&mut model, "replace", &buffers, &mut output)
        .unwrap();
    buffers
        .selected_mut()
        .unwrap()
        .apply_edit(TextEdit::insert(Position::new(0, 3), "!"))
        .unwrap();
    output.clear();
    let error = proposals
        .run(Command::Accept(ProposalId(1)), &mut buffers, &mut output)
        .unwrap_err();
    assert!(error.to_string().contains("RevisionConflict"));
    assert_eq!(buffers.selected().unwrap().text(), "old!");
    assert_eq!(buffers.selected().unwrap().version(), 1);
    assert!(buffers.selected().unwrap().destination_path().is_none());
    assert_eq!(
        proposals.controller.get(ProposalId(1)).unwrap().status(),
        ProposalStatus::Pending
    );
    assert!(output.is_empty());
}

#[test]
fn incompatible_language_or_capabilities_fail_before_model_dispatch() {
    for case in 0..5 {
        let buffers = buffers(if case == 0 {
            "other"
        } else {
            "configured-fixture"
        });
        let mut config = config();
        match case {
            1 => config.options.capabilities.propose.clear(),
            2 => config
                .options
                .capabilities
                .propose
                .push("language.configured-fixture.missing".into()),
            3 => config
                .options
                .capabilities
                .inspect
                .push("language.configured-fixture.missing".into()),
            4 => config
                .options
                .capabilities
                .inspect
                .push("language.configured-fixture.replace".into()),
            _ => {}
        }
        let mut proposals = Proposals::configured(Some(config)).unwrap();
        assert!(proposals.validate_request("replace", &buffers).is_err());
        let mut model = model(&buffers, &["new"]);
        let mut output = Vec::new();
        assert!(
            proposals
                .generate(&mut model, "replace", &buffers, &mut output)
                .is_err()
        );
        assert_eq!(model.calls, 0);
        assert_eq!(proposals.controller.list().count(), 0);
        assert!(output.is_empty());
        unchanged(&buffers);
    }
}

#[test]
fn invalid_assurance_or_acceptance_configuration_is_rejected() {
    for case in 0..6 {
        let mut config = config();
        match case {
            0 => config.task.policy = AssurancePolicy::Guided,
            1 => config.task.policy = AssurancePolicy::Exploratory,
            2 => config.task.criteria.clear(),
            3 => {
                config
                    .task
                    .criteria
                    .get_mut("exact-source")
                    .unwrap()
                    .acceptance = Acceptance::Provisional
            }
            4 => {
                config
                    .task
                    .criteria
                    .get_mut("exact-source")
                    .unwrap()
                    .mandatory = false
            }
            5 => {
                config.task.criteria.get_mut("exact-source").unwrap().origin =
                    ExpectationOrigin::Agent("author".into())
            }
            _ => unreachable!(),
        }
        assert!(Proposals::configured(Some(config)).is_err());
    }
    assert!(Proposals::configured(None).is_ok());
}
