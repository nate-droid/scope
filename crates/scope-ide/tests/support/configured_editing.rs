// Deterministic service and host contract shared by external-consumer tests.
use scope_ide::{
    agent::{EditCapabilities, SemanticEditOptions},
    cli::InteractiveEditConfig,
    task_contract::{Acceptance, AssurancePolicy, TaskContract, TaskCriterion},
};
use scope_language::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

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
                range: scope_ide::text::TextRange::new(Position::new(0, 0), Position::new(0, 3)),
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

pub fn config() -> InteractiveEditConfig {
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
