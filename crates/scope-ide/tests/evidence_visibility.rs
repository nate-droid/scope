#![cfg(feature = "tools")]

use scope_ide::{
    proposal::{ProposalError, ProposalId},
    source_edit::{PermittedEditScope, SourceEditController, SourceId, SourceTarget},
    task_contract::{Acceptance, AssurancePolicy, ExpectationOrigin, TaskContract, TaskCriterion},
    text::{Position, TextEdit, TextRange},
    tool::{AgentSession, ToolCall, ToolError, ToolValue, semantic::SemanticToolProvider},
};
use scope_language::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
};

const ORIGINAL: &str = "α old\n";
const CANDIDATE: &str = "α 新\n";

struct Target;
impl SourceTarget for Target {
    fn source_id(&self) -> SourceId {
        SourceId::Scratch(42)
    }
    fn text(&self) -> &str {
        ORIGINAL
    }
    fn version(&self) -> u64 {
        7
    }
    fn language_id(&self) -> &str {
        "evidence"
    }
    fn apply_source_edit(&mut self, _: TextEdit) -> Result<(), ProposalError> {
        panic!("tool invocation must not apply a proposal")
    }
}

struct Service {
    observations: ContractValidation,
    read_only: bool,
    calls: AtomicUsize,
    validations: AtomicUsize,
}
impl Service {
    fn new(outcome: Option<CheckOutcome>) -> Self {
        Self {
            observations: ContractValidation {
                checks: outcome
                    .into_iter()
                    .map(|outcome| {
                        (
                            "behavior".into(),
                            CheckObservation {
                                outcome,
                                actual: json!({"candidate": CANDIDATE, "native": [false, null, 3]}),
                            },
                        )
                    })
                    .collect(),
            },
            read_only: false,
            calls: AtomicUsize::new(0),
            validations: AtomicUsize::new(0),
        }
    }
}
impl LanguageService for Service {
    fn language_id(&self) -> &'static str {
        "evidence"
    }
    fn language_name(&self) -> &'static str {
        "Evidence fixture"
    }
    fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
        false
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        panic!("no implicit analysis or evaluation")
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        vec![SemanticOperationDescription {
            id: "inspect_or_replace".into(),
            description: "Deterministic evidence fixture".into(),
            arguments: vec![],
            effect: if self.read_only {
                SemanticOperationEffect::Read
            } else {
                SemanticOperationEffect::ProposeChanges
            },
            target: SemanticTargetRequirement::Document,
            constraints: None,
        }]
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.operation_id, "inspect_or_replace");
        assert_eq!(request.document.text(), ORIGINAL);
        assert_eq!(request.document.version(), 7);
        assert_eq!(request.arguments, json!({}));
        if self.read_only {
            Ok(SemanticOperationResult::ReadOnly {
                value: json!({"success": true, "native": [null, false]}),
                rationale: "inspection succeeded, not semantic satisfaction".into(),
            })
        } else {
            Ok(SemanticOperationResult::Changes {
                edits: vec![SemanticEdit {
                    range: TextRange::new(Position::new(0, 3), Position::new(0, 6)),
                    replacement: "新".into(),
                }],
                rationale: "replace the exact UTF-8 range".into(),
            })
        }
    }
    fn validate_semantic_contract(
        &self,
        request: SemanticValidationRequest<'_>,
        contract: &ValidationContract,
    ) -> Result<ContractValidation, SemanticOperationError> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.document.text(), ORIGINAL);
        assert_eq!(request.document.version(), 7);
        assert_eq!(request.candidate_source, CANDIDATE);
        assert_eq!(request.arguments, &json!({}));
        assert_eq!(request.document_id, "Scratch(42)");
        assert_eq!(contract.id, "task-contract");
        Ok(self.observations.clone())
    }
}

fn task(policy: AssurancePolicy, accepted: bool) -> TaskContract {
    TaskContract {
        original_request: "Replace old with 新".into(),
        goal: "Preserve the remaining UTF-8 source".into(),
        policy,
        criteria: BTreeMap::from([(
            "behavior".into(),
            TaskCriterion {
                expectation: json!({"opaque": [CANDIDATE, null]}),
                origin: ExpectationOrigin::Agent("fixture author".into()),
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

fn json_value(value: ToolValue) -> Value {
    match value {
        ToolValue::Null => Value::Null,
        ToolValue::Boolean(value) => json!(value),
        ToolValue::Integer(value) => json!(value),

        ToolValue::String(value) => json!(value),
        ToolValue::Array(values) => Value::Array(values.into_iter().map(json_value).collect()),
        ToolValue::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key, json_value(value)))
                .collect(),
        ),
    }
}

fn invoke(
    service: &Service,
    controller: &mut SourceEditController,
    denied_boundary: Option<usize>,
    precondition: &str,
    session_allowed: bool,
) -> Result<Value, ToolError> {
    let target = Target;
    let mut authorizations = 0;
    let provider = SemanticToolProvider::new(
        service,
        &target,
        controller,
        PermittedEditScope::Document(target.source_id()),
        None,
        |_| {
            authorizations += 1;
            denied_boundary != Some(authorizations)
        },
    )
    .unwrap();
    let mut session = AgentSession::builder().provider(provider).build().unwrap();
    let name = session.catalog().tools[0].definition.name.clone();
    let result = session.invoke_with_policy(
        ToolCall::new(name)
            .with_argument(
                "expected_source",
                if precondition == "source" {
                    "stale"
                } else {
                    ORIGINAL
                }
                .into(),
            )
            .with_argument(
                "expected_version",
                if precondition == "version" { "6" } else { "7" }.into(),
            )
            .with_argument("arguments", ToolValue::object([] as [(&str, ToolValue); 0])),
        |_| session_allowed,
    );
    assert_eq!(target.text(), ORIGINAL);
    assert_eq!(target.version(), 7);
    result.map(|result| json_value(result.value))
}

#[test]
fn proposal_feedback_distinguishes_acceptance_policy_and_observation_outcomes() {
    for policy in [
        AssurancePolicy::Exploratory,
        AssurancePolicy::Guided,
        AssurancePolicy::Strict,
    ] {
        for accepted in [false, true] {
            for outcome in [
                Some(CheckOutcome::Passed),
                Some(CheckOutcome::Failed),
                Some(CheckOutcome::Inconclusive),
                None,
            ] {
                let service = Service::new(outcome);
                let task = task(policy, accepted);
                let mut controller = SourceEditController::with_task_contract(task.clone());
                let result = invoke(&service, &mut controller, None, "fresh", true).unwrap();
                let blocked = accepted
                    && ((policy != AssurancePolicy::Exploratory
                        && outcome == Some(CheckOutcome::Failed))
                        || (policy == AssurancePolicy::Strict
                            && outcome != Some(CheckOutcome::Passed)));
                assert_eq!(
                    result["status"],
                    if blocked {
                        "validation_failed"
                    } else {
                        "proposal"
                    },
                    "{policy:?}, accepted={accepted}, {outcome:?}"
                );
                assert_eq!(result["operation_id"], "inspect_or_replace");
                assert_eq!(result["rationale"], "replace the exact UTF-8 range");
                assert_eq!(result["validation_status"], "NotEvaluated");
                assert_eq!(result["diagnostics"], json!([]));
                assert_eq!(result["contract_id"], "task-contract");
                assert_eq!(result["assurance_policy"], format!("{policy:?}"));
                assert_eq!(result["task_contract"], task.context());
                assert_eq!(
                    result["unresolved_obligations"],
                    if outcome == Some(CheckOutcome::Passed) {
                        json!([])
                    } else {
                        json!(["behavior"])
                    }
                );
                assert_eq!(
                    result["checks"],
                    json!([{
                        "id": "behavior",
                        "outcome": outcome.map(|outcome| format!("{outcome:?}")).unwrap_or_else(|| "NotEvaluated".into()),
                        "expected_json": task.criteria["behavior"].expectation.to_string(),
                        "actual_json": service.observations.checks.get("behavior").map(|check| check.actual.to_string()).unwrap_or_else(|| "null".into()),
                        "observed": outcome.is_some(),
                    }])
                );
                assert!(
                    result["evidence_scope"]
                        .as_str()
                        .unwrap()
                        .contains("not universal correctness")
                );
                assert_eq!(service.validations.load(Ordering::SeqCst), 1);
                if blocked {
                    assert!(result.get("proposal_id").is_none());
                    assert_eq!(result["correction_eligible"], true);
                    assert_eq!(
                        result["reason"],
                        if outcome == Some(CheckOutcome::Failed) {
                            "FailedEvidence"
                        } else {
                            "MissingEvidence"
                        }
                    );
                    assert_eq!(controller.list().count(), 0);
                } else {
                    let id = ProposalId(result["proposal_id"].as_str().unwrap().parse().unwrap());
                    let proposal = controller.get(id).unwrap();
                    assert_eq!(proposal.task_contract(), Some(&task));
                    let evidence = proposal.evidence().unwrap();
                    assert_eq!(evidence.operation, "inspect_or_replace");
                    assert_eq!(evidence.arguments, json!({}));
                    assert_eq!(evidence.contract, task.validation_contract());
                    assert_eq!(evidence.observations, service.observations);
                    assert_eq!(evidence.target, SourceId::Scratch(42));
                    assert_eq!(evidence.version, 7);
                    assert_eq!(evidence.original, ORIGINAL);
                    assert_eq!(evidence.candidate, CANDIDATE);
                    assert_eq!(proposal.preview(&Target).unwrap(), evidence.candidate);
                    assert_eq!(controller.list().count(), 1);
                }
            }
        }
    }
}

#[test]
fn opaque_task_numbers_are_preserved_in_explicit_json_fallback() {
    let service = Service::new(None);
    let mut task = task(AssurancePolicy::Exploratory, false);
    task.criteria.get_mut("behavior").unwrap().expectation = json!({"threshold": 0.5});
    let mut controller = SourceEditController::with_task_contract(task.clone());
    let result = invoke(&service, &mut controller, None, "fresh", true).unwrap();
    assert!(result.get("task_contract").is_none());
    let context: Value =
        serde_json::from_str(result["task_contract_json"].as_str().unwrap()).unwrap();
    assert_eq!(context, task.context());
    assert_eq!(controller.list().count(), 1);
}

#[test]
fn feedback_includes_union_of_contract_and_provider_only_checks_without_inventing_evidence() {
    let mut service = Service::new(None);
    service.observations.checks.insert(
        "provider-only".into(),
        CheckObservation {
            outcome: CheckOutcome::Passed,
            actual: json!({"native": [1, null, false]}),
        },
    );
    let task = task(AssurancePolicy::Guided, false);
    let mut controller = SourceEditController::with_task_contract(task.clone());
    let result = invoke(&service, &mut controller, None, "fresh", true).unwrap();
    assert_eq!(result["status"], "proposal");
    assert_eq!(
        result["checks"],
        json!([
            {"id": "behavior", "outcome": "NotEvaluated", "expected_json": task.criteria["behavior"].expectation.to_string(), "actual_json": "null", "observed": false},
            {"id": "provider-only", "outcome": "Passed", "expected_json": "null", "actual_json": service.observations.checks["provider-only"].actual.to_string(), "observed": true}
        ])
    );
    assert_eq!(result["unresolved_obligations"], json!(["behavior"]));
    let id = ProposalId(result["proposal_id"].as_str().unwrap().parse().unwrap());
    let evidence = controller.get(id).unwrap().evidence().unwrap();
    assert_eq!(evidence.observations, service.observations);
    assert!(!evidence.observations.checks.contains_key("behavior"));
}

#[test]
fn accepted_task_does_not_override_authorization_or_stale_preconditions() {
    for denied in 1..=3 {
        let service = Service::new(Some(CheckOutcome::Passed));
        let mut controller =
            SourceEditController::with_task_contract(task(AssurancePolicy::Strict, true));
        let result = invoke(&service, &mut controller, Some(denied), "fresh", true).unwrap();
        assert_eq!(result["status"], "unauthorized");
        assert!(result.get("proposal_id").is_none());
        assert_eq!(controller.list().count(), 0);
        assert_eq!(
            service.validations.load(Ordering::SeqCst),
            usize::from(denied == 3)
        );
    }
    for precondition in ["version", "source", "session_policy"] {
        let service = Service::new(Some(CheckOutcome::Passed));
        let mut controller =
            SourceEditController::with_task_contract(task(AssurancePolicy::Strict, true));
        let result = invoke(
            &service,
            &mut controller,
            None,
            precondition,
            precondition != "session_policy",
        );
        if precondition == "session_policy" {
            assert!(matches!(result, Err(ToolError::Denied { .. })));
        } else {
            assert_eq!(result.unwrap()["status"], "revision_conflict");
        }
        assert_eq!(controller.list().count(), 0);
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
        assert_eq!(service.validations.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn successful_read_only_result_does_not_claim_semantic_satisfaction() {
    let mut service = Service::new(Some(CheckOutcome::Passed));
    service.read_only = true;
    let mut controller =
        SourceEditController::with_task_contract(task(AssurancePolicy::Strict, true));
    let result = invoke(&service, &mut controller, None, "fresh", true).unwrap();
    assert_eq!(
        result,
        json!({
            "status": "read_only", "operation_id": "inspect_or_replace",
            "value": {"success": true, "native": [null, false]},
            "rationale": "inspection succeeded, not semantic satisfaction"
        })
    );
    assert_eq!(controller.list().count(), 0);
    assert_eq!(service.calls.load(Ordering::SeqCst), 1);
    assert_eq!(service.validations.load(Ordering::SeqCst), 0);
}
