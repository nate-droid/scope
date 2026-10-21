use super::*;
use crate::source_edit::SourceId;
use crate::task_contract::{Acceptance, AssurancePolicy, TaskContract, TaskCriterion};
use scope_language::*;
use std::{cell::Cell, collections::VecDeque, sync::Mutex};

struct Target;
impl SourceTarget for Target {
    fn source_id(&self) -> SourceId {
        SourceId::Scratch(42)
    }
    fn text(&self) -> &str {
        "old"
    }
    fn version(&self) -> u64 {
        7
    }
    fn language_id(&self) -> &str {
        "intent"
    }
    fn apply_source_edit(
        &mut self,
        _: crate::text::TextEdit,
    ) -> Result<(), crate::proposal::ProposalError> {
        panic!("semantic lifecycle must not apply a proposal")
    }
}

#[derive(Default)]
struct Service {
    dispatches: Mutex<Vec<String>>,
    validations: Mutex<Vec<(String, ValidationContract)>>,
}
impl LanguageService for Service {
    fn language_id(&self) -> &'static str {
        "intent"
    }
    fn language_name(&self) -> &'static str {
        "Intent fixture"
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
            description: "Propose candidate source".into(),
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
        let candidate = request.arguments["candidate"].as_str().unwrap();
        self.dispatches.lock().unwrap().push(candidate.into());
        Ok(SemanticOperationResult::Changes {
            edits: vec![SemanticEdit {
                range: crate::text::TextRange::new(
                    crate::text::Position::new(0, 0),
                    crate::text::Position::new(0, 3),
                ),
                replacement: candidate.into(),
            }],
            rationale: "candidate from operation arguments".into(),
        })
    }
    fn validate_semantic_contract(
        &self,
        request: SemanticValidationRequest<'_>,
        contract: &ValidationContract,
    ) -> Result<ContractValidation, SemanticOperationError> {
        assert_eq!(request.operation_id, "replace");
        assert_eq!(request.document_id, "Scratch(42)");
        assert_eq!(request.document.text(), "old");
        assert_eq!(request.document.version(), 7);
        assert_eq!(request.selection, None);
        assert_eq!(request.edits.len(), 1);
        assert_eq!(
            request.candidate_source,
            request.arguments["candidate"].as_str().unwrap()
        );
        assert_eq!(request.edits[0].replacement, request.candidate_source);
        self.validations
            .lock()
            .unwrap()
            .push((request.candidate_source.into(), contract.clone()));
        if request.candidate_source == "evaluation-error" {
            return Err(SemanticOperationError::InternalFailure(
                "fixture evaluator unavailable".into(),
            ));
        }
        Ok(ContractValidation {
            checks: contract
                .checks
                .iter()
                .map(|(id, expected)| {
                    let actual = json!(request.candidate_source);
                    let outcome = if request.candidate_source.starts_with("inconclusive") {
                        CheckOutcome::Inconclusive
                    } else if expected == &actual {
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

#[derive(Default)]
struct Model {
    decisions: VecDeque<String>,
    contexts: Vec<serde_json::Value>,
}
impl DecisionModel for Model {
    fn decide(
        &mut self,
        messages: &[crate::agent::DecisionMessage],
        _: u32,
    ) -> Result<String, String> {
        self.contexts.push(serde_json::Value::Array(
            messages
                .iter()
                .filter_map(|message| serde_json::from_str(&message.content).ok())
                .collect(),
        ));
        Ok(self
            .decisions
            .pop_front()
            .expect("unexpected model invocation"))
    }
}
fn propose(candidate: &str) -> String {
    json!({"type":"tool","name":"language.intent.replace","arguments":{
        "expected_version":"7","expected_source":"old","arguments":{"candidate":candidate}
    }})
    .to_string()
}
fn task(policy: AssurancePolicy, accepted: bool) -> TaskContract {
    TaskContract {
        original_request: "replace old with new".into(),
        goal: "new source".into(),
        policy,
        criteria: std::collections::BTreeMap::from([(
            "exact-source".into(),
            TaskCriterion {
                expectation: json!("new"),
                origin: ExpectationOrigin::Agent("author".into()),
                acceptance: if accepted {
                    Acceptance::Accepted {
                        consumer: "reviewer".into(),
                    }
                } else {
                    Acceptance::Provisional
                },
                mandatory: true,
            },
        )]),
    }
}
struct ResultSet {
    run: EditRun,
    controller: SourceEditController,
    model: Model,
    service: Service,
    policies: usize,
}
fn execute(task: TaskContract, decisions: Vec<String>, deny_at: Option<usize>) -> ResultSet {
    let target = Target;
    let service = Service::default();
    let mut model = Model {
        decisions: decisions.into(),
        ..Default::default()
    };
    let mut controller = SourceEditController::with_task_contract(task.clone());
    let policies = Cell::new(0);
    let options = SemanticEditOptions {
        capabilities: EditCapabilities {
            propose: vec!["language.intent.replace".into()],
            inspect_first: false,
            ..Default::default()
        },
        budgets: EditBudgets {
            corrections: 1,
            ..Default::default()
        },
    };
    let run = run_semantic_edit(
        &mut model,
        &mut controller,
        &target,
        &service,
        AgentEditRequest::new(&target, &task.original_request),
        &options,
        |_| {
            policies.set(policies.get() + 1);
            deny_at != Some(policies.get())
        },
    );
    assert_eq!(target.text(), "old");
    assert_eq!(target.version(), 7);
    assert_eq!(controller.task_contract(), Some(&task));
    if let Some(id) = run.proposal {
        assert_eq!(controller.list().count(), 1);
        let proposal = controller.get(id).unwrap();
        assert_eq!(proposal.status(), crate::proposal::ProposalStatus::Pending);
        assert_eq!(proposal.task_contract(), Some(&task));
        let evidence = proposal.evidence().unwrap();
        assert_eq!(evidence.contract, task.validation_contract());
        assert_eq!(
            controller.preview(id, &target).unwrap(),
            service.dispatches.lock().unwrap().last().unwrap().as_str()
        );
    } else {
        assert_eq!(controller.list().count(), 0);
    }
    ResultSet {
        run,
        controller,
        model,
        service,
        policies: policies.get(),
    }
}
fn outcome(result: &ResultSet) -> CheckOutcome {
    result
        .controller
        .get(result.run.proposal.unwrap())
        .unwrap()
        .evidence()
        .unwrap()
        .observations
        .checks["exact-source"]
        .outcome
}

#[test]
fn accepted_first_pass_is_pending_with_bound_evidence() {
    let result = execute(
        task(AssurancePolicy::Strict, true),
        vec![propose("new")],
        None,
    );
    assert_eq!(result.run.outcome, EditOutcome::ProposalReady);
    assert_eq!(
        (
            result.run.model_invocations,
            result.run.proposal_attempts,
            result.run.corrections
        ),
        (1, 1, 0)
    );
    assert_eq!(result.policies, 3);
    assert_eq!(outcome(&result), CheckOutcome::Passed);
    assert_eq!(result.service.validations.lock().unwrap().len(), 1);
}

#[test]
fn failed_candidate_is_corrected_using_structured_continuation() {
    let result = execute(
        task(AssurancePolicy::Guided, true),
        vec![propose("bad"), propose("new")],
        None,
    );
    assert_eq!(result.run.outcome, EditOutcome::ProposalReady);
    assert_eq!(
        (result.run.proposal_attempts, result.run.corrections),
        (2, 1)
    );
    assert_eq!(result.run.proposal, Some(ProposalId(1)));
    assert_eq!(outcome(&result), CheckOutcome::Passed);
    let expected = json!({"id":"exact-source","observed":true,"outcome":"Failed","expected_json":json!("new").to_string(),"actual_json":json!("bad").to_string()});
    fn contains(value: &serde_json::Value, expected: &serde_json::Value) -> bool {
        value == expected
            || match value {
                serde_json::Value::Array(items) => items.iter().any(|v| contains(v, expected)),
                serde_json::Value::Object(fields) => fields.values().any(|v| contains(v, expected)),
                _ => false,
            }
    }
    assert!(
        contains(&result.model.contexts[1], &expected),
        "{}",
        result.model.contexts[1]
    );
    assert_eq!(
        result.service.dispatches.lock().unwrap().as_slice(),
        ["bad", "new"]
    );
    assert_eq!(result.service.validations.lock().unwrap().len(), 2);
}

#[test]
fn repeated_failures_exhaust_one_correction_without_registration() {
    let result = execute(
        task(AssurancePolicy::Strict, true),
        vec![propose("bad"), propose("still-bad"), propose("new")],
        None,
    );
    assert_eq!(result.run.outcome, EditOutcome::ValidationFailure);
    assert_eq!(
        (
            result.run.model_invocations,
            result.run.proposal_attempts,
            result.run.corrections
        ),
        (2, 2, 1)
    );
    assert_eq!(result.model.decisions.len(), 1);
    assert_eq!(result.service.validations.lock().unwrap().len(), 2);
    assert_eq!(
        field(result.run.last_result.as_ref().unwrap(), "status").and_then(ToolValue::as_str),
        Some("validation_failed")
    );
}

#[test]
fn no_criteria_does_not_invent_assurance_requirements() {
    let result = execute(
        TaskContract {
            policy: AssurancePolicy::Strict,
            original_request: "replace source without criteria".into(),
            ..Default::default()
        },
        vec![propose("anything")],
        None,
    );
    assert_eq!(result.run.outcome, EditOutcome::ProposalReady);
    let evidence = result
        .controller
        .get(result.run.proposal.unwrap())
        .unwrap()
        .evidence()
        .unwrap();
    assert!(evidence.observations.checks.is_empty());
}

#[test]
fn accepting_agent_criterion_changes_enforcement_not_provenance() {
    let provisional = task(AssurancePolicy::Strict, false);
    let result = execute(provisional.clone(), vec![propose("bad")], None);
    assert_eq!(result.run.outcome, EditOutcome::ProposalReady);
    assert_eq!(outcome(&result), CheckOutcome::Failed);
    let mut accepted = provisional.clone();
    accepted
        .criteria
        .get_mut("exact-source")
        .unwrap()
        .accept("reviewer");
    assert_eq!(
        accepted.criteria["exact-source"].origin,
        provisional.criteria["exact-source"].origin
    );
    assert_eq!(
        accepted.criteria["exact-source"].expectation,
        provisional.criteria["exact-source"].expectation
    );
    let result = execute(accepted, vec![propose("bad"), propose("new")], None);
    assert_eq!(result.run.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.run.corrections, 1);
    let snapshot = result
        .controller
        .get(result.run.proposal.unwrap())
        .unwrap()
        .task_contract()
        .unwrap();
    assert_eq!(
        snapshot.criteria["exact-source"].origin,
        ExpectationOrigin::Agent("author".into())
    );
    assert_eq!(
        snapshot.criteria["exact-source"].acceptance,
        Acceptance::Accepted {
            consumer: "reviewer".into()
        }
    );
}

#[test]
fn inconclusive_is_retained_in_guided_but_blocks_strict() {
    let guided = execute(
        task(AssurancePolicy::Guided, true),
        vec![propose("inconclusive")],
        None,
    );
    assert_eq!(guided.run.outcome, EditOutcome::ProposalReady);
    assert_eq!(outcome(&guided), CheckOutcome::Inconclusive);
    let strict = execute(
        task(AssurancePolicy::Strict, true),
        vec![propose("inconclusive"), propose("inconclusive-again")],
        None,
    );
    assert_eq!(strict.run.outcome, EditOutcome::ValidationFailure);
    assert_eq!(strict.run.corrections, 1);
}

#[test]
fn exploratory_retains_accepted_failed_observation() {
    let result = execute(
        task(AssurancePolicy::Exploratory, true),
        vec![propose("bad")],
        None,
    );
    assert_eq!(result.run.outcome, EditOutcome::ProposalReady);
    assert_eq!(outcome(&result), CheckOutcome::Failed);
    assert_eq!(result.run.corrections, 0);
}

#[test]
fn task_acceptance_does_not_grant_dispatch_or_completion_authorization() {
    for denied in [1, 2, 3] {
        let result = execute(
            task(AssurancePolicy::Strict, true),
            vec![propose("new")],
            Some(denied),
        );
        assert_eq!(result.run.outcome, EditOutcome::AuthorizationFailure);
        assert_eq!(result.policies, denied);
        assert_eq!(
            result.service.dispatches.lock().unwrap().len(),
            usize::from(denied > 1)
        );
        assert_eq!(
            result.service.validations.lock().unwrap().len(),
            usize::from(denied > 2)
        );
    }
}

#[test]
fn model_completion_is_not_a_proposal_or_application() {
    let result = execute(
        task(AssurancePolicy::Strict, true),
        vec![json!({"type":"complete","content":"done"}).to_string()],
        None,
    );
    assert_ne!(result.run.outcome, EditOutcome::ProposalReady);
    assert!(result.run.proposal.is_none());
    assert_eq!(result.run.model_invocations, 1);
    assert!(result.service.dispatches.lock().unwrap().is_empty());
    assert!(result.service.validations.lock().unwrap().is_empty());
    assert_eq!(result.policies, 0);
}

#[test]
fn evaluator_error_is_retained_as_error_not_behavioral_failure() {
    let result = execute(
        task(AssurancePolicy::Guided, true),
        vec![propose("evaluation-error")],
        None,
    );
    assert_eq!(outcome(&result), CheckOutcome::Error);
    let proposal = result.controller.get(result.run.proposal.unwrap()).unwrap();
    assert_eq!(proposal.unresolved_obligations(), vec!["exact-source"]);
    assert_eq!(
        result.model.contexts[0][0]["task_contract"],
        task(AssurancePolicy::Guided, true).context()
    );
}
