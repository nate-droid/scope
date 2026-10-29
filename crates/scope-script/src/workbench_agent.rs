//! Opt-in, in-memory Script agent operations. Parsing never executes Script.
use scope_ide::{
    agent::{AgentLimits, EditCapabilities, SemanticEditOptions},
    cli::{InteractiveEditConfig, RetainedScratchAgentRun, SemanticAgentBinding},
    task_contract::{Acceptance, AssurancePolicy, TaskContract, TaskCriterion},
    tool::semantic::SemanticAuthorization,
};
use scope_language::*;
use serde_json::json;

pub static SCRIPT_SERVICE: ScriptService = ScriptService;
pub struct ScriptService;

const LIMITATIONS: &str = "Syntax only: no execution, type/name checking, behavioral equivalence, or verification of the requested change. Review the diff before accepting; run Script separately with explicit consent.";

fn complete(source: &str) -> bool {
    matches!(
        crate::admit_snapshot(source),
        crate::SnapshotAdmission::CompletePure
            | crate::SnapshotAdmission::CompleteExplicitRunRequired
    )
}

impl LanguageService for ScriptService {
    fn language_id(&self) -> &'static str {
        "scope-script"
    }
    fn language_name(&self) -> &'static str {
        "Scope Script"
    }
    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("scope")
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        vec![
            SemanticOperationDescription {
                id: "inspect".into(),
                description: "Read the exact selected Script source and syntax admission; does not execute it".into(),
                arguments: vec![], effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document, constraints: None,
            },
            SemanticOperationDescription {
                id: "replace".into(),
                description: format!("Propose replacing the selected Script source. {LIMITATIONS}"),
                arguments: vec![SemanticArgument { name: "candidate".into(), kind: SemanticArgumentKind::String, required: true, description: "Complete replacement Script source".into() }],
                effect: SemanticOperationEffect::ProposeChanges,
                target: SemanticTargetRequirement::Document, constraints: None,
            },
        ]
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        match request.operation_id {
            "inspect" => Ok(SemanticOperationResult::ReadOnly {
                value: json!({"source": request.document.text(), "version": request.document.version(), "complete_syntax": complete(request.document.text()), "limitations": LIMITATIONS}),
                rationale: "Exact invocation-local Script snapshot; no execution".into(),
            }),
            "replace" => {
                let candidate = request.arguments["candidate"].as_str().ok_or_else(|| {
                    SemanticOperationError::InvalidArguments("candidate must be a string".into())
                })?;
                let text = request.document.text();
                let end = scope_ide::text::TextBuffer::new(text)
                    .range_for_span(scope_ide::text::Span::new(0, text.len()))
                    .expect("whole source span")
                    .end;
                Ok(SemanticOperationResult::Changes {
                    edits: vec![SemanticEdit {
                        range: TextRange::new(Position::new(0, 0), end),
                        replacement: candidate.into(),
                    }],
                    rationale: LIMITATIONS.into(),
                })
            }
            id => Err(SemanticOperationError::UnsupportedOperation(id.into())),
        }
    }
    fn validate_semantic_candidate(
        &self,
        request: SemanticValidationRequest<'_>,
    ) -> Result<CandidateValidation, SemanticOperationError> {
        Ok(CandidateValidation {
            status: if complete(request.candidate_source) {
                ValidationStatus::AcceptedWithLimitations
            } else {
                ValidationStatus::Rejected
            },
            limitations: vec![LIMITATIONS.into()],
            ..Default::default()
        })
    }
    fn validate_semantic_contract(
        &self,
        request: SemanticValidationRequest<'_>,
        contract: &ValidationContract,
    ) -> Result<ContractValidation, SemanticOperationError> {
        // This contract is deliberately syntax-only, not a behavioral requirement.
        Ok(ContractValidation {
            checks: contract
                .checks
                .iter()
                .map(|(id, expectation)| {
                    let actual = complete(request.candidate_source);
                    let outcome = if id != "complete-script-syntax" || expectation != &json!(true) {
                        CheckOutcome::Inconclusive
                    } else if actual {
                        CheckOutcome::Passed
                    } else {
                        CheckOutcome::Failed
                    };
                    (
                        id.clone(),
                        CheckObservation {
                            outcome,
                            actual: json!(actual),
                        },
                    )
                })
                .collect(),
        })
    }
}

pub(super) struct AgentBinding {
    allowed: Vec<String>,
    authorize: Box<dyn for<'a> FnMut(SemanticAuthorization<'a>) -> bool>,
    observe: fn(&RetainedScratchAgentRun<'_>),
}
impl AgentBinding {
    pub(super) fn new() -> Self {
        Self {
            allowed: vec!["inspect".into()],
            authorize: Box::new(|request| {
                request.operation.id == "inspect"
                    && request.operation.effect == SemanticOperationEffect::Read
            }),
            observe: |_| {},
        }
    }
    pub(super) fn binding(&mut self) -> SemanticAgentBinding<'_> {
        SemanticAgentBinding {
            service: &SCRIPT_SERVICE,
            allowed_operation_ids: &self.allowed,
            authorize: &mut self.authorize,
            limits: AgentLimits::default(),
            trace: None,
            model: None,
            observe: &mut self.observe,
        }
    }
}

/// Independent proposal authority. This does not grant reads, execution or saves.
pub fn editing_config() -> InteractiveEditConfig {
    InteractiveEditConfig {
        service: &SCRIPT_SERVICE,
        task: TaskContract {
            original_request: "Propose a Script edit for explicit review".into(),
            goal: format!("Complete Script syntax only. {LIMITATIONS}"),
            policy: AssurancePolicy::Strict,
            criteria: [(
                "complete-script-syntax".into(),
                TaskCriterion {
                    expectation: json!(true),
                    origin: ExpectationOrigin::LanguageInvariant("scope-script parser".into()),
                    acceptance: Acceptance::Accepted {
                        consumer: "workbench --allow-edit".into(),
                    },
                    mandatory: true,
                },
            )]
            .into(),
        },
        options: SemanticEditOptions {
            capabilities: EditCapabilities {
                propose: vec!["language.scope-script.replace".into()],
                inspect_first: false,
                ..Default::default()
            },
            ..Default::default()
        },
        authorize: Box::new(|request| {
            request.operation.id == "replace"
                && request.operation.effect == SemanticOperationEffect::ProposeChanges
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scope_ide::{cli::ReadOnlyCommand, scratch::ScratchBuffers, source_edit::SourceTarget};

    #[test]
    fn default_runner_has_no_binding_and_read_binding_grants_only_inspection() {
        let mut runner = crate::workbench::WorkbenchRunner::default();
        assert!(runner.semantic_agent().is_none());
        assert!(!runner.allow_app_agent_fallback());
        let mut runner = runner.with_agent_reads();
        let binding = runner.semantic_agent().unwrap();
        assert_eq!(binding.allowed_operation_ids, ["inspect"]);
        assert!(binding.model.is_none());
        let mut sources = ScratchBuffers::default();
        sources.create("scope-script", "print(7); missing");
        let source = sources.selected().unwrap().snapshot();
        let operations = SCRIPT_SERVICE.semantic_operations();
        for operation in &operations {
            let authorization = SemanticAuthorization {
                operation,
                source,
                arguments: &json!({}),
            };
            assert_eq!(
                (binding.authorize)(authorization),
                operation.id == "inspect"
            );
        }
    }

    #[test]
    fn replacement_validation_is_syntax_only_and_never_executes_source() {
        let mut sources = ScratchBuffers::default();
        sources.create("scope-script", "print(1);\r\n\"雪😀\"\r\n");
        let source = sources.selected().unwrap();
        let document = LanguageDocument::new(
            LanguagePath::new("scratch"),
            source.text(),
            source.version(),
        );
        let arguments =
            json!({"candidate":"print(2); scope.invoke(\"fs\", \"write\", {}); missing"});
        let result = SCRIPT_SERVICE
            .execute_semantic_operation(SemanticOperationRequest {
                operation_id: "replace",
                document: document.clone(),
                selection: None,
                arguments: arguments.clone(),
            })
            .unwrap();
        let SemanticOperationResult::Changes { edits, rationale } = result else {
            panic!()
        };
        assert!(rationale.contains("Syntax only"));
        let mut text = scope_ide::text::TextBuffer::new(source.text());
        text.apply_edits(
            edits
                .iter()
                .map(|edit| scope_ide::text::TextEdit::replace(edit.range, &edit.replacement)),
        )
        .unwrap();
        assert_eq!(text.as_str(), arguments["candidate"].as_str().unwrap());
        let contract = editing_config().task.validation_contract();
        for (candidate, valid) in [(text.as_str(), true), ("if true {", false), ("[)", false)] {
            let request = SemanticValidationRequest {
                operation_id: "replace",
                document: document.clone(),
                candidate_source: candidate,
                edits: &edits,
                selection: None,
                arguments: &arguments,
                document_id: "scratch",
            };
            let validation = SCRIPT_SERVICE
                .validate_semantic_candidate(request.clone())
                .unwrap();
            assert_eq!(
                validation.status,
                if valid {
                    ValidationStatus::AcceptedWithLimitations
                } else {
                    ValidationStatus::Rejected
                }
            );
            let checks = SCRIPT_SERVICE
                .validate_semantic_contract(request, &contract)
                .unwrap();
            assert_eq!(
                checks.checks["complete-script-syntax"].outcome,
                if valid {
                    CheckOutcome::Passed
                } else {
                    CheckOutcome::Failed
                }
            );
        }
        assert_eq!(source.text(), "print(1);\r\n\"雪😀\"\r\n");
    }
}
