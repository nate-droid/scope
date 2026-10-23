use super::*;

const READ: &str = "language.mock.inspect";

fn investigation_options() -> SemanticInvestigationOptions {
    SemanticInvestigationOptions {
        operations: vec![READ.into()],
        ..Default::default()
    }
}

struct InvestigationResult {
    run: EditRun,
    contexts: Vec<serde_json::Value>,
    dispatches: usize,
    policies: usize,
    completions: usize,
}

fn investigate(
    decisions: Vec<String>,
    options: SemanticInvestigationOptions,
    initial_operations: Vec<String>,
    authorize: bool,
    assessment: InvestigationCompletion,
) -> InvestigationResult {
    let target = Target;
    let service = Service {
        language: "mock",
        dispatches: AtomicUsize::new(0),
    };
    let mut controller = SourceEditController::default();
    let mut model = Model {
        decisions: decisions.into(),
        ..Default::default()
    };
    let policies = Cell::new(0);
    let completions = Cell::new(0);
    let run = run_semantic_investigation_with_discovery(
        &mut model,
        &mut controller,
        &target,
        &service,
        AgentEditRequest::new(&target, "inspect the source without changing it"),
        &options,
        &SemanticDiscoveryOptions {
            initial_operations,
            page_size: 8,
        },
        |_| {
            policies.set(policies.get() + 1);
            authorize
        },
        |content, evidence| {
            completions.set(completions.get() + 1);
            assert_eq!(content, "done");
            for item in evidence {
                assert_eq!(item.call.name, READ);
                assert_snapshot(&item.source);
                assert_eq!(
                    field(&item.response, "status").and_then(ToolValue::as_str),
                    Some("read_only")
                );
            }
            assessment
        },
    );
    assert_eq!(target.text(), "old");
    assert_eq!(target.version(), 7);
    assert!(run.proposal.is_none());
    assert_eq!(run.proposal_attempts, 0);
    assert!(controller.get(ProposalId(1)).is_err());
    assert_snapshot(
        run.source_snapshot
            .as_ref()
            .expect("snapshot on every exit"),
    );
    InvestigationResult {
        run,
        contexts: model.contexts,
        dispatches: service.dispatches.load(Ordering::SeqCst),
        policies: policies.get(),
        completions: completions.get(),
    }
}

fn assert_snapshot(snapshot: &ToolValue) {
    assert_eq!(
        field(snapshot, "source_id").and_then(ToolValue::as_str),
        Some("Scratch(42)")
    );
    assert_eq!(
        field(snapshot, "version").and_then(ToolValue::as_str),
        Some("7")
    );
    assert_eq!(
        field(snapshot, "source").and_then(ToolValue::as_str),
        Some("old")
    );
}

#[test]
fn discovery_read_and_host_completion_retain_evidence_without_proposals() {
    let result = investigate(
        vec![discover(), tool(READ), complete()],
        investigation_options(),
        vec![],
        true,
        InvestigationCompletion::Completed,
    );
    assert_eq!(result.run.outcome, EditOutcome::InvestigationCompleted);
    assert_eq!(
        (result.dispatches, result.policies, result.completions),
        (1, 1, 1)
    );
    assert_eq!(result.run.model_invocations, 3);
    assert_eq!(result.run.inspections, 1);
    assert_eq!(result.run.conclusion.as_deref(), Some("done"));
    assert_eq!(result.run.discovered_operations, vec![READ.to_string()]);
    assert!(names(&result.contexts[0]).is_empty());
    assert_eq!(names(&result.contexts[1]), vec![READ]);
    assert_eq!(names(&result.contexts[2]), vec![READ]);
    assert!(
        result
            .contexts
            .iter()
            .all(|context| context["phase"] == "Inspect")
    );
    assert_eq!(result.run.evidence.len(), 1);
    let evidence = &result.run.evidence[0];
    assert_eq!(
        evidence.source,
        *result.run.source_snapshot.as_ref().unwrap()
    );
    assert_eq!(Some(&evidence.response), result.run.last_result.as_ref());
    assert_eq!(
        field(&evidence.response, "value").and_then(ToolValue::as_str),
        Some("old")
    );
    assert!(
        result
            .run
            .trace
            .contains(&EditEvent::Discovery { revealed: 1 })
    );
}

#[test]
fn discovered_read_still_requires_host_authorization() {
    let result = investigate(
        vec![discover(), tool(READ)],
        investigation_options(),
        vec![],
        false,
        InvestigationCompletion::Completed,
    );
    assert_eq!(result.run.outcome, EditOutcome::AuthorizationFailure);
    assert_eq!(
        (result.dispatches, result.policies, result.completions),
        (0, 1, 0)
    );
    assert_eq!(result.run.evidence.len(), 1);
    let evidence = &result.run.evidence[0];
    assert_eq!(evidence.call.name, READ);
    assert_snapshot(&evidence.source);
    assert_eq!(
        field(&evidence.response, "status").and_then(ToolValue::as_str),
        Some("unauthorized")
    );
    assert_eq!(result.run.last_result.as_ref(), Some(&evidence.response));
    assert!(result.run.conclusion.is_none());
}

#[test]
fn hidden_read_is_not_dispatched_before_discovery() {
    let result = investigate(
        vec![tool(READ)],
        investigation_options(),
        vec![],
        true,
        InvestigationCompletion::Completed,
    );
    assert_eq!(result.run.outcome, EditOutcome::AuthorizationFailure);
    assert_eq!(
        (result.dispatches, result.policies, result.completions),
        (0, 0, 0)
    );
    assert!(result.run.evidence.is_empty());
}

#[test]
fn investigation_filters_write_unknown_and_foreign_operations_from_discovery() {
    for forbidden in [
        "language.mock.replace",
        "language.mock.ungranted",
        "language.other.inspect",
        "language.mock.unknown",
    ] {
        let mut options = investigation_options();
        options.operations.push(forbidden.into());
        let result = investigate(
            vec![discover(), tool(forbidden)],
            options,
            vec![],
            true,
            InvestigationCompletion::Completed,
        );
        assert_eq!(names(&result.contexts[1]), vec![READ]);
        assert_eq!(result.run.outcome, EditOutcome::AuthorizationFailure);
        assert_eq!(
            (result.dispatches, result.policies, result.completions),
            (0, 0, 0)
        );
        assert!(result.run.evidence.is_empty());
    }
}

#[test]
fn host_acceptance_without_read_evidence_is_inconclusive() {
    for decisions in [vec![complete()], vec![discover(), complete()]] {
        let result = investigate(
            decisions,
            investigation_options(),
            vec![],
            true,
            InvestigationCompletion::Completed,
        );
        assert_eq!(result.run.outcome, EditOutcome::InvestigationInconclusive);
        assert_eq!(
            (result.dispatches, result.policies, result.completions),
            (0, 0, 1)
        );
        assert_eq!(result.run.conclusion.as_deref(), Some("done"));
        assert!(result.run.evidence.is_empty());
    }
}

#[test]
fn host_assessment_controls_conclusion_even_after_successful_read() {
    for (assessment, outcome) in [
        (
            InvestigationCompletion::Inconclusive,
            EditOutcome::InvestigationInconclusive,
        ),
        (
            InvestigationCompletion::Unsupported,
            EditOutcome::UnsupportedRequest,
        ),
    ] {
        let result = investigate(
            vec![tool(READ), complete()],
            investigation_options(),
            vec![READ.into()],
            true,
            assessment,
        );
        assert_eq!(result.run.outcome, outcome);
        assert_eq!(
            (result.dispatches, result.policies, result.completions),
            (1, 1, 1)
        );
        assert_eq!(result.run.evidence.len(), 1);
        assert_eq!(result.run.conclusion.as_deref(), Some("done"));
    }
}

#[test]
fn malformed_decision_can_recover_but_cannot_create_evidence() {
    let mut options = investigation_options();
    options.budgets.transport.max_malformed = 1;
    let result = investigate(
        vec!["not json".into(), tool(READ), complete()],
        options,
        vec![READ.into()],
        true,
        InvestigationCompletion::Completed,
    );
    assert_eq!(result.run.outcome, EditOutcome::InvestigationCompleted);
    assert_eq!(result.run.model_invocations, 3);
    assert_eq!(result.dispatches, 1);
    assert_eq!(result.run.evidence.len(), 1);
    assert!(
        result
            .run
            .trace
            .iter()
            .any(|event| matches!(event, EditEvent::ProtocolRetry { attempt: 1, .. }))
    );
}

#[test]
fn malformed_retry_exhaustion_stops_without_dispatch() {
    let mut options = investigation_options();
    options.budgets.transport.max_malformed = 1;
    let result = investigate(
        vec!["not json".into(), "still not json".into()],
        options,
        vec![],
        true,
        InvestigationCompletion::Completed,
    );
    assert_eq!(result.run.outcome, EditOutcome::NoProgress);
    assert_eq!(result.run.model_invocations, 2);
    assert_eq!(
        (result.dispatches, result.policies, result.completions),
        (0, 0, 0)
    );
    assert!(result.run.evidence.is_empty());
    assert!(result.run.conclusion.is_none());
}

#[test]
fn repeated_read_is_suppressed_and_does_not_duplicate_evidence() {
    let result = investigate(
        vec![tool(READ), tool(READ)],
        investigation_options(),
        vec![READ.into()],
        true,
        InvestigationCompletion::Completed,
    );
    assert_eq!(result.run.outcome, EditOutcome::NoProgress);
    assert_eq!(
        (result.dispatches, result.policies, result.completions),
        (1, 1, 0)
    );
    assert_eq!(result.run.evidence.len(), 1);
    assert_eq!(result.run.inspections, 1);
    assert!(result.run.trace.contains(&EditEvent::RepeatedCall));
    assert_eq!(
        result.run.last_result.as_ref(),
        Some(&result.run.evidence[0].response)
    );
}

#[test]
fn suppressed_read_can_reuse_evidence_and_complete_within_retry_budget() {
    let mut options = investigation_options();
    options.budgets.repeated_calls = 1;
    let result = investigate(
        vec![tool(READ), tool(READ), complete()],
        options,
        vec![READ.into()],
        true,
        InvestigationCompletion::Completed,
    );
    assert_eq!(result.run.outcome, EditOutcome::InvestigationCompleted);
    assert_eq!(
        (result.dispatches, result.policies, result.completions),
        (1, 1, 1)
    );
    assert_eq!(result.run.evidence.len(), 1);
}

#[test]
fn investigation_budgets_stop_before_unauthorized_extra_work() {
    for budget in ["model", "inspection", "tool", "discovery"] {
        let mut options = investigation_options();
        let (decisions, initial, outcome) = match budget {
            "model" => {
                options.budgets.model_invocations = 0;
                (vec![], vec![], EditOutcome::OperationBudgetExhausted)
            }
            "inspection" => {
                options.budgets.inspections = 0;
                (
                    vec![tool(READ)],
                    vec![READ.into()],
                    EditOutcome::ExplorationBudgetExhausted,
                )
            }
            "tool" => {
                options.budgets.transport.max_tool_calls = 0;
                (
                    vec![tool(READ)],
                    vec![READ.into()],
                    EditOutcome::OperationBudgetExhausted,
                )
            }
            _ => {
                options.budgets.transport.max_tool_calls = 1;
                (
                    vec![discover(), tool(READ)],
                    vec![],
                    EditOutcome::OperationBudgetExhausted,
                )
            }
        };
        let result = investigate(
            decisions,
            options,
            initial,
            true,
            InvestigationCompletion::Completed,
        );
        assert_eq!(result.run.outcome, outcome, "{budget}");
        assert_eq!(
            (result.dispatches, result.policies, result.completions),
            (0, 0, 0)
        );
        assert!(result.run.evidence.is_empty());
        assert!(result.run.conclusion.is_none());
    }
}

#[test]
fn model_budget_exit_preserves_already_collected_evidence() {
    let mut options = investigation_options();
    options.budgets.model_invocations = 1;
    let result = investigate(
        vec![tool(READ)],
        options,
        vec![READ.into()],
        true,
        InvestigationCompletion::Completed,
    );
    assert_eq!(result.run.outcome, EditOutcome::OperationBudgetExhausted);
    assert_eq!(
        (result.dispatches, result.policies, result.completions),
        (1, 1, 0)
    );
    assert_eq!(result.run.evidence.len(), 1);
    assert!(result.run.conclusion.is_none());
}

#[test]
fn predispatch_validation_failure_still_returns_bound_source_snapshot() {
    let target = Target;
    let service = Service {
        language: "mock",
        dispatches: AtomicUsize::new(0),
    };
    for failure in ["version", "source", "instruction", "scope"] {
        let mut request = AgentEditRequest::new(&target, "inspect source");
        match failure {
            "version" => request.source.version = 6,
            "source" => request.source.text = "stale",
            "instruction" => request.instruction = " ",
            _ => request.scope = PermittedEditScope::Document(SourceId::Scratch(99)),
        }
        let mut model = Model::default();
        let mut controller = SourceEditController::default();
        let run = run_semantic_investigation_with_discovery(
            &mut model,
            &mut controller,
            &target,
            &service,
            request,
            &investigation_options(),
            &SemanticDiscoveryOptions {
                initial_operations: vec![],
                page_size: 8,
            },
            |_| panic!("pre-dispatch failure must not authorize"),
            |_, _| panic!("pre-dispatch failure must not assess completion"),
        );
        assert_eq!(
            run.outcome,
            if failure == "scope" {
                EditOutcome::AuthorizationFailure
            } else {
                EditOutcome::MissingPrerequisite
            }
        );
        assert_snapshot(run.source_snapshot.as_ref().unwrap());
        assert!(model.contexts.is_empty());
        assert_eq!(run.model_invocations, 0);
        assert!(run.evidence.is_empty());
        assert!(run.conclusion.is_none());
        assert!(run.proposal.is_none());
        assert!(controller.get(ProposalId(1)).is_err());
    }
    assert_eq!(service.dispatches.load(Ordering::SeqCst), 0);
}

const EVALUATE: &str = "language.mock.evaluate_example";

struct InvestigationService(Service);

impl LanguageService for InvestigationService {
    fn language_id(&self) -> &'static str {
        self.0.language_id()
    }
    fn language_name(&self) -> &'static str {
        self.0.language_name()
    }
    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        self.0.matches_path(path)
    }
    fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
        self.0.analyze(document)
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        let mut operations = self.0.semantic_operations();
        let mut evaluation = operations
            .iter()
            .find(|operation| operation.id == "inspect")
            .unwrap()
            .clone();
        evaluation.id = "evaluate_example".into();
        evaluation.description = "Compute a disposable example without changing source".into();
        evaluation.constraints = None;
        operations.push(evaluation);
        operations
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        if request.operation_id != "evaluate_example" {
            return self.0.perform_semantic_operation(request);
        }
        self.0.dispatches.fetch_add(1, Ordering::SeqCst);
        let input = request.arguments["text"]
            .as_str()
            .expect("validated string argument");
        Ok(SemanticOperationResult::ReadOnly {
            value: json!({"input": input, "length": input.chars().count(), "disposable": true}),
            rationale: "Computed in isolation; no source changes".into(),
        })
    }
}

struct MutableInvestigationTarget {
    text: String,
    version: u64,
}

impl SourceTarget for MutableInvestigationTarget {
    fn source_id(&self) -> SourceId {
        SourceId::Scratch(42)
    }
    fn text(&self) -> &str {
        &self.text
    }
    fn version(&self) -> u64 {
        self.version
    }
    fn language_id(&self) -> &str {
        "mock"
    }
    fn apply_source_edit(
        &mut self,
        _: crate::text::TextEdit,
    ) -> Result<(), crate::proposal::ProposalError> {
        panic!("investigation must not apply source edits")
    }
}

#[test]
fn multiple_reads_support_host_assessment_and_survive_later_target_changes() {
    let mut target = MutableInvestigationTarget {
        text: "old".into(),
        version: 7,
    };
    let service = InvestigationService(Service {
        language: "mock",
        dispatches: AtomicUsize::new(0),
    });
    let mut model = Model {
        decisions: vec![discover(), tool(READ), tool(EVALUATE), complete()].into(),
        ..Default::default()
    };
    let mut controller = SourceEditController::default();
    let assessments = Cell::new(0);
    let run = run_semantic_investigation_with_discovery(
        &mut model,
        &mut controller,
        &target,
        &service,
        AgentEditRequest::new(&target, "Inspect source and evaluate a disposable example"),
        &SemanticInvestigationOptions {
            operations: vec![READ.into(), EVALUATE.into()],
            ..Default::default()
        },
        &SemanticDiscoveryOptions {
            initial_operations: vec![],
            page_size: 8,
        },
        |_| true,
        |content, evidence| {
            assessments.set(assessments.get() + 1);
            let supported = content == "done"
                && evidence.len() == 2
                && evidence[0].call.name == READ
                && field(&evidence[0].response, "value").and_then(ToolValue::as_str) == Some("old")
                && evidence[1].call.name == EVALUATE
                && field(&evidence[1].response, "value")
                    == Some(&ToolValue::object([
                        ("input", "new".into()),
                        ("length", ToolValue::Integer(3)),
                        ("disposable", true.into()),
                    ]));
            if supported {
                InvestigationCompletion::Completed
            } else {
                InvestigationCompletion::Inconclusive
            }
        },
    );
    assert_eq!(run.outcome, EditOutcome::InvestigationCompleted);
    assert_eq!(assessments.get(), 1);
    assert_eq!(service.0.dispatches.load(Ordering::SeqCst), 2);
    assert_eq!(run.inspections, 2);
    assert_eq!(run.proposal_attempts, 0);
    assert!(run.proposal.is_none());
    assert_eq!(controller.list().count(), 0);
    assert_eq!(target.text(), "old");
    assert_eq!(target.version(), 7);
    let response = run.evidence[1].response.clone();
    let call = run.evidence[1].call.clone();
    // Simulate a later host edit after the investigation has released its borrow.
    target.text = "later host edit".into();
    target.version += 1;
    assert_eq!(target.text(), "later host edit");
    assert_eq!(target.version(), 8);
    assert_snapshot(run.source_snapshot.as_ref().unwrap());
    for evidence in &run.evidence {
        assert_snapshot(&evidence.source);
    }
    assert_eq!(run.evidence[1].response, response);
    assert_eq!(run.evidence[1].call, call);
    assert_eq!(run.conclusion.as_deref(), Some("done"));
}

#[test]
fn invalid_read_arguments_can_be_corrected_without_losing_rejection_evidence() {
    let target = Target;
    let service = InvestigationService(Service {
        language: "mock",
        dispatches: AtomicUsize::new(0),
    });
    let invalid = json!({"type":"tool", "name":EVALUATE,
        "arguments":{"expected_version":"7", "expected_source":"old", "arguments":{"text":42}}})
    .to_string();
    let mut model = Model {
        decisions: vec![invalid, tool(EVALUATE), complete()].into(),
        ..Default::default()
    };
    let mut controller = SourceEditController::default();
    let mut options = SemanticInvestigationOptions {
        operations: vec![EVALUATE.into()],
        ..Default::default()
    };
    options.budgets.transport.max_malformed = 1;
    let run = run_semantic_investigation_with_discovery(
        &mut model,
        &mut controller,
        &target,
        &service,
        AgentEditRequest::new(&target, "evaluate a disposable example"),
        &options,
        &SemanticDiscoveryOptions {
            initial_operations: vec![EVALUATE.into()],
            page_size: 8,
        },
        |_| true,
        |content, evidence| {
            assert_eq!(content, "done");
            assert_eq!(evidence.len(), 2);
            assert_eq!(
                field(&evidence[0].response, "status").and_then(ToolValue::as_str),
                Some("invalid_arguments")
            );
            assert_eq!(
                field(&evidence[1].response, "status").and_then(ToolValue::as_str),
                Some("read_only")
            );
            let value = field(&evidence[1].response, "value").unwrap();
            assert_eq!(field(value, "length"), Some(&ToolValue::Integer(3)));
            assert_eq!(field(value, "disposable"), Some(&ToolValue::Boolean(true)));
            InvestigationCompletion::Completed
        },
    );
    assert_eq!(run.outcome, EditOutcome::InvestigationCompleted);
    assert_eq!(run.model_invocations, 3);
    assert_eq!(service.0.dispatches.load(Ordering::SeqCst), 1);
    assert!(run.trace.iter().any(|event| matches!(event, EditEvent::ProtocolRetry { category, attempt: 1 } if category == "invalid_arguments")));
    assert_ne!(run.evidence[0].call, run.evidence[1].call);
    for evidence in &run.evidence {
        assert_snapshot(&evidence.source);
    }
    assert_eq!(run.last_result.as_ref(), Some(&run.evidence[1].response));
    assert_eq!(run.proposal_attempts, 0);
    assert!(run.proposal.is_none());
    assert_eq!(controller.list().count(), 0);
    assert_eq!(target.text(), "old");
}
