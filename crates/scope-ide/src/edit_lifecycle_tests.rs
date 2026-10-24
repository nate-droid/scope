use super::*;
use crate::agent::DecisionMessage;
use crate::tool::{ToolProvider, ToolResult};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct NativeTarget {
    text: String,
    version: u64,
}
impl SourceTarget for NativeTarget {
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
        "neutral"
    }
    fn apply_source_edit(
        &mut self,
        edit: crate::text::TextEdit,
    ) -> Result<(), crate::proposal::ProposalError> {
        let mut text = crate::text::TextBuffer::new(&self.text);
        text.apply_edit(edit)
            .map_err(crate::proposal::ProposalError::Text)?;
        self.text = text.as_str().into();
        self.version += 1;
        Ok(())
    }
}
struct NativeService;
impl scope_language::LanguageService for NativeService {
    fn language_id(&self) -> &'static str {
        "neutral"
    }
    fn language_name(&self) -> &'static str {
        "Neutral"
    }
    fn matches_path(&self, _: &scope_language::LanguagePath<'_>) -> bool {
        true
    }
    fn analyze(&self, _: scope_language::LanguageDocument<'_>) -> scope_language::LanguageAnalysis {
        Default::default()
    }
    fn semantic_operations(&self) -> Vec<scope_language::SemanticOperationDescription> {
        use scope_language::*;
        vec![SemanticOperationDescription {
            id: "replace".into(),
            description: "replace source".into(),
            arguments: vec![SemanticArgument {
                name: "text".into(),
                kind: SemanticArgumentKind::String,
                required: true,
                description: "replacement".into(),
            }],
            effect: SemanticOperationEffect::ProposeChanges,
            target: SemanticTargetRequirement::Document,
            constraints: None,
        }]
    }
    fn perform_semantic_operation(
        &self,
        request: scope_language::SemanticOperationRequest<'_>,
    ) -> Result<scope_language::SemanticOperationResult, scope_language::SemanticOperationError>
    {
        use scope_language::*;
        let text = request.arguments["text"].as_str().unwrap();
        if text == "bad" {
            return Err(SemanticOperationError::InvalidArguments(
                "choose a valid replacement".into(),
            ));
        }
        Ok(SemanticOperationResult::Changes {
            edits: vec![SemanticEdit {
                range: crate::text::TextRange::new(
                    crate::text::Position::new(0, 0),
                    crate::text::Position::new(0, 3),
                ),
                replacement: text.into(),
            }],
            rationale: "requested replacement".into(),
        })
    }
}
#[test]
fn real_m1_m2_adapter_preserves_review_and_binds_snapshot() {
    let mut target = NativeTarget {
        text: "old".into(),
        version: 7,
    };
    let mut controller = SourceEditController::default();
    let responses = ["bad", "new"].map(|text| {
        json!({"type":"tool","name":"language.neutral.replace","arguments":{"text":text}})
            .to_string()
    });
    let mut model = script(responses.into());
    let options = SemanticEditOptions {
        capabilities: EditCapabilities {
            propose: vec!["language.neutral.replace".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let result = run_semantic_edit(
        &mut model,
        &mut controller,
        &target,
        &NativeService,
        AgentEditRequest::new(&target, "replace"),
        &options,
        |_| true,
    );
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.corrections, 1);
    let id = result.proposal.unwrap();
    assert_eq!(target.text(), "old");
    assert_eq!(target.version(), 7);
    assert_eq!(
        controller.get(id).unwrap().status(),
        crate::proposal::ProposalStatus::Pending
    );
    assert_eq!(controller.preview(id, &target).unwrap(), "new");
    controller.accept(id, &mut target).unwrap();
    assert_eq!(target.text(), "new");
    assert_eq!(target.version(), 8);
}

#[test]
fn semantic_runners_bind_multiline_source_and_accept_domain_only_calls() {
    let original = "old\nsecond\n";
    let replacement = "new\nline";
    let target = NativeTarget {
        text: original.into(),
        version: 7,
    };
    let mut controller = SourceEditController::default();
    let mut model = script(vec![
        semantic_call("survey", replacement),
        json!({"type":"complete","content":"Observed\nsource"}).to_string(),
    ]);
    let investigation = run_semantic_investigation_with_discovery(
        &mut model,
        &mut controller,
        &target,
        &EvaluatingService,
        AgentEditRequest::new(&target, "inspect"),
        &SemanticInvestigationOptions {
            operations: vec!["language.neutral.survey".into()],
            ..Default::default()
        },
        &SemanticDiscoveryOptions {
            initial_operations: vec!["language.neutral.survey".into()],
            page_size: 1,
        },
        |authorization| {
            assert_eq!(authorization.source.text, original);
            assert_eq!(authorization.arguments, &json!({"text":replacement}));
            true
        },
        |content, evidence| {
            assert_eq!(content, "Observed\nsource");
            assert_eq!(evidence.len(), 1);
            InvestigationCompletion::Completed
        },
    );
    assert_eq!(investigation.outcome, EditOutcome::InvestigationCompleted);
    let mut model = script(vec![semantic_call("replace", replacement)]);
    let edit = run_semantic_edit(
        &mut model,
        &mut controller,
        &target,
        &NativeService,
        AgentEditRequest::new(&target, "replace"),
        &SemanticEditOptions {
            capabilities: EditCapabilities {
                propose: vec!["language.neutral.replace".into()],
                ..Default::default()
            },
            ..Default::default()
        },
        |authorization| {
            assert_eq!(authorization.source.text, original);
            assert_eq!(authorization.arguments, &json!({"text":replacement}));
            true
        },
    );
    assert_eq!(edit.outcome, EditOutcome::ProposalReady);
    assert_eq!(
        controller.preview(edit.proposal.unwrap(), &target).unwrap(),
        "new\nline\nsecond\n"
    );
    assert_eq!(target.text(), original);
    assert_eq!(target.version(), 7);
}

struct EvaluatingService;
impl scope_language::LanguageService for EvaluatingService {
    fn language_id(&self) -> &'static str {
        "neutral"
    }
    fn language_name(&self) -> &'static str {
        "Neutral"
    }
    fn matches_path(&self, _: &scope_language::LanguagePath<'_>) -> bool {
        true
    }
    fn analyze(&self, _: scope_language::LanguageDocument<'_>) -> scope_language::LanguageAnalysis {
        Default::default()
    }
    fn semantic_operations(&self) -> Vec<scope_language::SemanticOperationDescription> {
        use scope_language::*;
        let mut operations = NativeService.semantic_operations();
        let mut evaluate = operations[0].clone();
        evaluate.id = "evaluate".into();
        evaluate.effect = SemanticOperationEffect::Read;
        operations.push(
            evaluate
                .with_purpose(SemanticOperationPurpose::CandidateEvaluation)
                .unwrap(),
        );
        let mut inspection = operations[0].clone();
        inspection.id = "survey".into();
        inspection.effect = SemanticOperationEffect::Read;
        operations.push(inspection);
        operations
    }
    fn semantic_operation_produces_evaluation(&self, operation_id: &str) -> bool {
        operation_id == "evaluate"
    }
    fn perform_semantic_operation(
        &self,
        request: scope_language::SemanticOperationRequest<'_>,
    ) -> Result<scope_language::SemanticOperationResult, scope_language::SemanticOperationError>
    {
        use scope_language::*;
        if request.operation_id == "survey" {
            if request.arguments["text"] == "fourth-power" {
                return Ok(SemanticOperationResult::ReadOnly {
                    value: json!({"status":"not_found", "name":"fourth-power"}),
                    rationale: "Temporary candidate definitions are not installed".into(),
                });
            }
            return Ok(SemanticOperationResult::ReadOnly {
                value: json!({"inspection_only": request.arguments["text"]}),
                rationale: "unrelated inspection evidence".into(),
            });
        }
        if request.operation_id != "evaluate" {
            return NativeService.perform_semantic_operation(request);
        }
        let satisfied = request.arguments["text"] == "new";
        Ok(SemanticOperationResult::Evaluated {
            value: json!({"observed": request.arguments["text"]}),
            rationale: "generic behavioral expectation".into(),
            evaluation: SemanticEvaluation {
                outcome: if satisfied {
                    SemanticOutcome::Satisfied
                } else {
                    SemanticOutcome::Violated
                },
                summary: "Candidate must produce the expected text".into(),
                diagnostics: Some(json!({"expected": "new"})),
                failed_criteria: if satisfied {
                    vec![]
                } else {
                    vec!["expected-text".into()]
                },
                contract_id: None,
            },
        })
    }
}

#[test]
fn generic_provider_violation_reaches_refinement_and_revised_proposal() {
    let target = NativeTarget {
        text: "old".into(),
        version: 7,
    };
    let mut controller = SourceEditController::default();
    let call = |operation: &str, text: &str| {
        json!({
            "type":"tool", "name":format!("language.neutral.{operation}"),
            "arguments":{"text":text}
        })
        .to_string()
    };
    let mut model = script(vec![
        call("evaluate", "wrong"),
        call("evaluate", "new"),
        complete(),
        call("replace", "new"),
    ]);
    let options = SemanticEditOptions {
        capabilities: EditCapabilities {
            inspect: vec!["language.neutral.evaluate".into()],
            propose: vec!["language.neutral.replace".into()],
            inspect_first: true,
        },
        ..Default::default()
    };
    let result = run_semantic_edit(
        &mut model,
        &mut controller,
        &target,
        &EvaluatingService,
        AgentEditRequest::new(&target, "produce expected text"),
        &options,
        |_| true,
    );
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.inspections, 0);
    assert_eq!(result.evaluations, 2);
    assert_eq!(result.refinements, 1);
    assert_eq!(result.corrections, 0);
    let context = model.messages[1]
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(context.contains("Revise the candidate and evaluate again"));
    assert!(context.contains("observed"));
    assert!(context.contains("wrong"));
    assert!(context.contains("produce expected text"));
    assert!(context.contains("produces_evaluation"));
    assert_eq!(
        controller
            .preview(result.proposal.unwrap(), &target)
            .unwrap(),
        "new"
    );
    assert_eq!(target.text(), "old");
}

fn semantic_call(operation: &str, text: &str) -> String {
    json!({
        "type": "tool", "name": format!("language.neutral.{operation}"),
        "arguments": {"text": text}
    })
    .to_string()
}

fn refinement_run(
    responses: Vec<String>,
    budgets: EditBudgets,
    authorize: bool,
) -> (EditRun, Script, SourceEditController, NativeTarget) {
    let target = NativeTarget {
        text: "old".into(),
        version: 7,
    };
    let mut controller = SourceEditController::default();
    let mut model = script(responses);
    let options = SemanticEditOptions {
        capabilities: EditCapabilities {
            inspect: vec![
                "language.neutral.survey".into(),
                "language.neutral.evaluate".into(),
            ],
            propose: vec!["language.neutral.replace".into()],
            inspect_first: true,
        },
        budgets,
    };
    let result = run_semantic_edit(
        &mut model,
        &mut controller,
        &target,
        &EvaluatingService,
        AgentEditRequest::new(&target, "produce expected text"),
        &options,
        |authorization| {
            authorize
                || authorization.operation.effect == scope_language::SemanticOperationEffect::Read
        },
    );
    (result, model, controller, target)
}

#[test]
fn refinement_aware_acceptance_preserves_inspection_and_review_boundaries() {
    assert_eq!(EditBudgets::default().refinements, 2);
    let (result, model, controller, target) = refinement_run(
        vec![
            semantic_call("survey", "unrelated-one"),
            semantic_call("survey", "unrelated-two"),
            semantic_call("evaluate", "wrong"),
            semantic_call("evaluate", "new"),
            complete(),
            semantic_call("replace", "new"),
        ],
        EditBudgets::default(),
        true,
    );
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(
        (result.inspections, result.evaluations, result.refinements),
        (2, 2, 1)
    );
    assert_eq!((result.proposal_attempts, result.corrections), (1, 0));
    assert_eq!(model.calls, 6);
    assert_eq!(
        result
            .trace
            .iter()
            .filter(|e| matches!(e, EditEvent::Inspection(_)))
            .count(),
        2
    );
    assert_eq!(
        result
            .trace
            .iter()
            .filter(|e| matches!(e, EditEvent::CandidateEvaluation(_)))
            .count(),
        2
    );
    assert_eq!(
        result
            .trace
            .iter()
            .filter(|e| matches!(e, EditEvent::RefinementAttempt(_)))
            .count(),
        1
    );
    assert_eq!(
        result
            .trace
            .iter()
            .filter(|e| matches!(e, EditEvent::ToolCall { .. }))
            .count(),
        5
    );
    let feedback = model.messages[3]
        .iter()
        .filter_map(|m| serde_json::from_str::<serde_json::Value>(&m.content).ok())
        .find(|v| v["kind"] == "candidate_evaluation_feedback")
        .unwrap();
    let focused = feedback.to_string();
    assert!(focused.contains("Revise the candidate and evaluate again"));
    assert!(focused.contains("expected-text"));
    assert!(focused.contains("wrong"));
    assert!(!focused.contains("unrelated-one"));
    assert!(!focused.contains("unrelated-two"));
    assert!(!focused.contains("inspection_only"));
    let id = result.proposal.unwrap();
    assert_eq!(controller.preview(id, &target).unwrap(), "new");
    assert_eq!(
        controller.get(id).unwrap().status(),
        crate::proposal::ProposalStatus::Pending
    );
    assert_eq!((target.text(), target.version()), ("old", 7));
}

#[test]
fn guided_refinement_survives_optional_source_inspection() {
    let (run, model, _, target) = refinement_run(
        vec![
            semantic_call("evaluate", "wrong"),
            semantic_call("survey", "existing-source"),
            semantic_call("evaluate", "new"),
            complete(),
            semantic_call("replace", "new"),
        ],
        EditBudgets::default(),
        true,
    );
    assert_eq!(run.outcome, EditOutcome::ProposalReady);
    assert_eq!((run.evaluations, run.inspections), (2, 1));
    for index in [1, 2] {
        let context = model.messages[index]
            .iter()
            .filter_map(|m| serde_json::from_str::<serde_json::Value>(&m.content).ok())
            .find(|v| v["kind"] == "edit_continuation")
            .unwrap();
        let guidance = &context["candidate_refinement"];
        assert_eq!(
            guidance["primary_action"],
            "submit_changed_candidate_using_evaluator"
        );
        assert_eq!(guidance["decision"]["arguments"]["text"], "wrong");
        assert_eq!(guidance["remaining_refinements"], 2);
        assert!(
            guidance["inspection_scope"]
                .as_str()
                .unwrap()
                .contains("installs nothing")
        );
        assert_eq!(
            guidance["evaluation_orchestration"],
            "explicit_model_invocation"
        );
    }
    assert_eq!(target.text(), "old");
}

#[test]
fn original_five_decision_failure_is_bounded_and_never_redispatched() {
    let (run, model, _, _) = refinement_run(
        vec![
            semantic_call("survey", "square"),
            semantic_call("survey", "cube"),
            semantic_call("evaluate", "wrong"),
            semantic_call("survey", "fourth-power"),
            semantic_call("evaluate", "wrong"),
        ],
        EditBudgets::default(),
        true,
    );
    assert_eq!(run.outcome, EditOutcome::NoProgress);
    assert_eq!(model.calls, 5);
    assert_eq!((run.evaluations, run.refinements), (1, 0));
    assert_eq!(run.summary().duplicate_calls_suppressed, 1);
}

#[test]
fn unresolved_candidate_completion_reports_inability_without_proposal() {
    let (run, _, controller, _) = refinement_run(
        vec![semantic_call("evaluate", "wrong"), complete()],
        EditBudgets::default(),
        true,
    );
    assert_eq!(run.outcome, EditOutcome::ValidationFailure);
    assert_eq!(run.proposal_attempts, 0);
    assert!(run.proposal.is_none());
    drop(controller);
}

#[test]
fn marked_evaluations_exhaust_refinements_not_exploration() {
    for refinements in [0, 2] {
        let (result, model, _, _) = refinement_run(
            (0..=refinements)
                .map(|n| semantic_call("evaluate", &format!("wrong-{n}")))
                .collect(),
            EditBudgets {
                refinements,
                ..Default::default()
            },
            true,
        );
        assert_eq!(result.outcome, EditOutcome::RefinementBudgetExhausted);
        assert_eq!(result.inspections, 0);
        assert_eq!(result.evaluations, refinements + 1);
        assert_eq!(result.refinements, refinements);
        assert_eq!(model.calls, refinements + 1);
        assert_eq!(result.proposal_attempts, 0);
        assert!(result.proposal.is_none());
    }
}

#[test]
fn third_operation_violation_honors_explicit_global_and_refinement_limits() {
    for budgets in [
        EditBudgets {
            model_invocations: 3,
            ..Default::default()
        },
        EditBudgets {
            refinements: 0,
            ..Default::default()
        },
    ] {
        let expected = if budgets.refinements == 0 {
            EditOutcome::RefinementBudgetExhausted
        } else {
            EditOutcome::OperationBudgetExhausted
        };
        let (result, model, controller, _) = refinement_run(
            vec![
                semantic_call("survey", "words"),
                semantic_call("survey", "describe(square)"),
                semantic_call("evaluate", "wrong"),
                semantic_call("evaluate", "new"),
            ],
            budgets,
            true,
        );
        assert_eq!(result.outcome, expected);
        assert_eq!(model.calls, 3);
        assert_eq!(
            (result.inspections, result.evaluations, result.refinements),
            (2, 1, 0)
        );
        assert_eq!(result.proposal_attempts, 0);
        assert_eq!(controller.list().count(), 0);
        assert!(field(result.last_result.as_ref().unwrap(), "evaluation").is_some());
    }
}

#[test]
fn marked_evaluations_obey_global_model_and_tool_limits() {
    for tool_limit in [false, true] {
        let mut budgets = EditBudgets::default();
        if tool_limit {
            budgets.transport.max_tool_calls = 1;
        } else {
            budgets.model_invocations = 1;
        }
        let (result, model, _, _) = refinement_run(
            vec![
                semantic_call("evaluate", "wrong"),
                semantic_call("evaluate", "new"),
            ],
            budgets,
            true,
        );
        assert_eq!(result.outcome, EditOutcome::OperationBudgetExhausted);
        assert_eq!(
            (result.inspections, result.evaluations, result.refinements),
            (0, 1, 0)
        );
        assert_eq!(model.calls, if tool_limit { 2 } else { 1 });
        assert_eq!(
            result
                .trace
                .iter()
                .filter(|e| matches!(e, EditEvent::ToolCall { .. }))
                .count(),
            1
        );
        assert_eq!(result.proposal_attempts, 0);
    }
}

#[test]
fn duplicate_marked_evaluation_does_not_consume_refinement() {
    let (result, model, _, _) = refinement_run(
        vec![
            semantic_call("evaluate", "wrong"),
            semantic_call("evaluate", "wrong"),
        ],
        EditBudgets::default(),
        true,
    );
    assert_eq!(result.outcome, EditOutcome::NoProgress);
    assert_eq!((result.evaluations, result.refinements), (1, 0));
    assert_eq!(result.summary().duplicate_calls_suppressed, 1);
    assert_eq!(model.calls, 2);
}

#[test]
fn refined_candidate_cannot_bypass_proposal_authorization() {
    let (result, model, _, target) = refinement_run(
        vec![
            semantic_call("evaluate", "wrong"),
            semantic_call("evaluate", "new"),
            complete(),
            semantic_call("replace", "new"),
        ],
        EditBudgets::default(),
        false,
    );
    assert_eq!(result.outcome, EditOutcome::AuthorizationFailure);
    assert_eq!((result.evaluations, result.refinements), (2, 1));
    assert_eq!(model.calls, 4);
    assert!(result.proposal.is_none());
    assert_eq!(target.text(), "old");
}

#[test]
fn exhausted_exploration_still_permits_candidate_evaluation() {
    let budgets = EditBudgets {
        inspections: 1,
        ..Default::default()
    };
    let (result, _, _, _) = refinement_run(
        vec![
            semantic_call("survey", "one"),
            semantic_call("evaluate", "wrong"),
            semantic_call("evaluate", "new"),
            complete(),
            semantic_call("replace", "new"),
        ],
        budgets,
        true,
    );
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(
        (result.inspections, result.evaluations, result.refinements),
        (1, 2, 1)
    );
    let (result, model, _, _) = refinement_run(
        vec![
            semantic_call("survey", "one"),
            semantic_call("survey", "two"),
        ],
        budgets,
        true,
    );
    assert_eq!(result.outcome, EditOutcome::ExplorationBudgetExhausted);
    assert_eq!((result.inspections, result.evaluations), (1, 0));
    assert_eq!(model.calls, 2);
}

struct Script {
    responses: VecDeque<Result<String, String>>,
    calls: usize,
    messages: Vec<Vec<DecisionMessage>>,
}
impl DecisionModel for Script {
    fn decide(&mut self, messages: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.calls += 1;
        self.messages.push(messages.to_vec());
        self.responses
            .pop_front()
            .expect("unexpected model invocation")
    }
}
fn script(responses: Vec<String>) -> Script {
    Script {
        responses: responses.into_iter().map(Ok).collect(),
        calls: 0,
        messages: Vec::new(),
    }
}
fn tool(name: &str, n: usize) -> String {
    json!({"type":"tool", "name":name, "arguments":{"n":n}}).to_string()
}
fn complete() -> String {
    json!({"type":"complete","content":"ready"}).to_string()
}
fn value(status: &str) -> ToolValue {
    if status == "read_only" {
        ToolValue::object([
            ("status", status.into()),
            ("value", "source information".into()),
        ])
    } else {
        ToolValue::object([("status", status.into())])
    }
}
struct Provider {
    definitions: Vec<ToolDefinition>,
    replies: VecDeque<ToolValue>,
    calls: Arc<AtomicUsize>,
}
impl ToolProvider for Provider {
    fn tools(&self) -> &[ToolDefinition] {
        &self.definitions
    }
    fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::new(
            self.replies.pop_front().expect("unexpected dispatch"),
        ))
    }
}
fn caps(inspect: bool) -> EditCapabilities {
    EditCapabilities {
        inspect: vec!["read".into()],
        propose: vec!["edit".into()],
        inspect_first: inspect,
    }
}
fn proposal() -> ToolValue {
    ToolValue::object([("status", "proposal".into()), ("proposal_id", "1".into())])
}
fn invalid() -> ToolValue {
    ToolValue::object([
        ("status", "invalid_source".into()),
        (
            "diagnostics",
            ToolValue::array([ToolValue::object([(
                "message",
                "invalid identifier".into(),
            )])]),
        ),
    ])
}
fn session(replies: Vec<ToolValue>) -> (AgentSession<'static>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Provider {
        definitions: vec![
            ToolDefinition::new("read", "inspect"),
            ToolDefinition::new("edit", "propose").effect(ToolEffect::Write),
        ],
        replies: replies.into(),
        calls: calls.clone(),
    };
    (
        AgentSession::builder().provider(provider).build().unwrap(),
        calls,
    )
}
fn run(
    model: &mut Script,
    session: &mut AgentSession<'_>,
    inspect: bool,
    budgets: EditBudgets,
) -> EditRun {
    run_edit_session(
        model,
        session,
        Ok(SourceId::Scratch(1)),
        "scoped edit request",
        &caps(inspect),
        budgets,
        |_, _| true,
    )
}
#[test]
fn successful_inspection_and_proposal_terminates_without_extra_decision() {
    let mut model = script(vec![
        tool("read", 0),
        complete(),
        tool("edit", 0),
        complete(),
    ]);
    let (mut session, calls) = session(vec![value("read_only"), proposal()]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.state.phase(), EditPhase::Finished);
    assert_eq!(result.proposal, Some(ProposalId(1)));
    assert_eq!(model.calls, 3);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(result.trace.contains(&EditEvent::NewSourceInformation));
    let inspect_context = model.messages[0].last().unwrap().content.clone();
    assert!(inspect_context.contains("read"));
    assert!(!inspect_context.contains("\"name\":\"edit\""));
    let propose_context = model.messages[2].last().unwrap().content.clone();
    assert!(propose_context.contains("\"name\":\"edit\""));
    assert!(!propose_context.contains("\"name\":\"read\""));
}
#[test]
fn unchanged_arguments_with_changed_relevant_state_can_dispatch() {
    let mut model = script(vec![
        tool("read", 0),
        tool("read", 0),
        complete(),
        tool("edit", 0),
    ]);
    let (mut session, calls) = session(vec![
        value("read_only"),
        ToolValue::object([
            ("status", "read_only".into()),
            ("value", "new source information".into()),
        ]),
        proposal(),
    ]);
    let mut revision = 0;
    let mut state = |_: &ToolCall| {
        revision += 1;
        ToolValue::Integer(revision)
    };
    let result = run_edit_session_with_state(
        &mut model,
        &mut session,
        Ok(SourceId::Scratch(1)),
        "request",
        &caps(true),
        EditBudgets::default(),
        EditDispatch {
            policy: &mut |_, _| true,
            relevant_state: &mut state,
        },
    );
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(!result.trace.contains(&EditEvent::RepeatedCall));
}
#[test]
fn changed_rationale_is_not_new_source_information() {
    let mut model = script(vec![
        tool("read", 0),
        tool("read", 1),
        complete(),
        tool("edit", 0),
    ]);
    let reads = ["first rationale", "restated rationale"].map(|rationale| {
        ToolValue::object([
            ("status", "read_only".into()),
            ("value", "same source".into()),
            ("rationale", rationale.into()),
        ])
    });
    let (mut session, _) = session(vec![reads[0].clone(), reads[1].clone(), proposal()]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(
        result
            .trace
            .iter()
            .filter(|event| **event == EditEvent::NewSourceInformation)
            .count(),
        1
    );
}
#[test]
fn direct_proposal() {
    let mut model = script(vec![tool("edit", 0), complete()]);
    let (mut session, _) = session(vec![proposal()]);
    let result = run(&mut model, &mut session, false, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.inspections, 0);
    assert_eq!(result.proposal_attempts, 1);
    assert_eq!(model.calls, 1);
}
#[test]
fn invalid_transitions_are_rejected_and_terminal_states_are_closed() {
    let mut state = EditState::default();
    assert!(state.transition(EditPhase::Validate).is_err());
    assert_eq!(state.phase(), EditPhase::Resolve);
    state.transition(EditPhase::Propose).unwrap();
    assert!(state.transition(EditPhase::Inspect).is_err());
    state.transition(EditPhase::Validate).unwrap();
    state.transition(EditPhase::Finished).unwrap();
    for phase in [
        EditPhase::Resolve,
        EditPhase::Inspect,
        EditPhase::Propose,
        EditPhase::Validate,
        EditPhase::Finished,
        EditPhase::Blocked,
    ] {
        assert!(state.transition(phase).is_err());
    }
}
#[test]
fn repeated_identical_inspection_is_not_dispatched() {
    let mut model = script(vec![tool("read", 0), tool("read", 0)]);
    let (mut session, calls) = session(vec![value("read_only")]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::NoProgress);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(result.last_result.is_some());
}
#[test]
fn inspection_budget_is_enforced_before_dispatch() {
    let mut model = script((0..4).map(|n| tool("read", n)).collect());
    let (mut session, calls) = session(vec![value("read_only"); 3]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::OperationBudgetExhausted);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
#[test]
fn one_correction_then_valid_proposal() {
    let mut model = script(vec![tool("edit", 0), tool("edit", 1), complete()]);
    let (mut session, _) = session(vec![invalid(), proposal()]);
    let result = run(&mut model, &mut session, false, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.corrections, 1);
    assert_eq!(result.proposal_attempts, 2);
    assert!(
        result
            .trace
            .contains(&EditEvent::RevisedProposalProduced(ProposalId(1)))
    );
    assert!(
        model.messages[1]
            .iter()
            .any(|m| m.content.contains("invalid identifier"))
    );
}
#[test]
fn failed_correction_terminates() {
    let mut model = script(vec![tool("edit", 0), tool("edit", 1), tool("edit", 2)]);
    let (mut session, calls) = session(vec![invalid(), invalid()]);
    let result = run(&mut model, &mut session, false, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ValidationFailure);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(model.calls, 2);
}
#[test]
fn missing_prerequisite_and_ambiguous_target_block_before_model() {
    for (resolved, outcome) in [
        (
            Err(TargetResolutionError::NotFound),
            EditOutcome::MissingPrerequisite,
        ),
        (
            Err(TargetResolutionError::Ambiguous { candidates: vec![] }),
            EditOutcome::AmbiguousTarget,
        ),
    ] {
        let mut model = script(vec![]);
        let (mut session, _) = session(vec![]);
        let result = run_edit_session(
            &mut model,
            &mut session,
            resolved,
            "request",
            &caps(false),
            EditBudgets::default(),
            |_, _| true,
        );
        assert_eq!(result.outcome, outcome);
        assert_eq!(result.state.phase(), EditPhase::Blocked);
        assert_eq!(model.calls, 0);
    }
}
#[test]
fn authorization_failure_never_retries() {
    let mut model = script(vec![tool("edit", 0), tool("edit", 1)]);
    let (mut session, calls) = session(vec![]);
    let result = run_edit_session(
        &mut model,
        &mut session,
        Ok(SourceId::Scratch(1)),
        "request",
        &caps(false),
        EditBudgets::default(),
        |_, _| false,
    );
    assert_eq!(result.outcome, EditOutcome::AuthorizationFailure);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(model.calls, 1);
    let mut model = script(vec![tool("edit", 0), tool("edit", 1)]);
    let (mut session, _) = self::session(vec![value("unauthorized")]);
    assert_eq!(
        run(&mut model, &mut session, false, EditBudgets::default()).outcome,
        EditOutcome::AuthorizationFailure
    );
    assert_eq!(model.calls, 1);
}
#[test]
fn backend_error_terminates_cleanly_without_logging_error_text() {
    let mut model = script(vec![]);
    model.responses.push_back(Err("secret credential".into()));
    let (mut session, _) = session(vec![]);
    let result = run(&mut model, &mut session, false, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::AgentBackendFailure);
    assert_eq!(result.model_invocations, 1);
    assert!(!format!("{:?}", result.trace).contains("secret"));
}
#[test]
fn model_and_proposal_budgets_and_repetition_allowance_are_finite() {
    for budgets in [
        EditBudgets {
            model_invocations: 0,
            ..Default::default()
        },
        EditBudgets {
            proposals: 0,
            ..Default::default()
        },
    ] {
        let mut model = script(vec![tool("edit", 0)]);
        let (mut session, calls) = session(vec![]);
        assert_eq!(
            run(&mut model, &mut session, false, budgets).outcome,
            EditOutcome::OperationBudgetExhausted
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    let mut model = script(vec![tool("read", 0), tool("read", 0), tool("read", 0)]);
    let (mut session, calls) = session(vec![value("read_only")]);
    let result = run(
        &mut model,
        &mut session,
        true,
        EditBudgets {
            repeated_calls: 1,
            ..Default::default()
        },
    );
    assert_eq!(result.outcome, EditOutcome::NoProgress);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn malformed_decision_receives_feedback_then_proposal_finishes() {
    let malformed = "{\"type\":\"complete\",\"content\":\"first\nsecond\"}";
    let mut model = script(vec![malformed.into(), tool("edit", 0)]);
    let (mut session, calls) = session(vec![ToolValue::object([
        ("status", "proposal".into()),
        ("proposal_id", "1".into()),
    ])]);
    let result = run(&mut model, &mut session, false, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.model_invocations, 2);
    assert_eq!(result.corrections, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        model.messages[1]
            .iter()
            .all(|m| m.role != "assistant" && m.content != malformed)
    );
    let feedback = model.messages[1]
        .iter()
        .filter(|message| message.role == "user")
        .filter_map(|message| serde_json::from_str::<serde_json::Value>(&message.content).ok())
        .find(|value| value["kind"] == "protocol_error")
        .expect("retained protocol correction");
    assert_eq!(feedback["category"], "malformed_response");
    let instruction = feedback["instruction"].as_str().unwrap();
    assert!(instruction.contains(r#"newlines inside JSON strings as \n"#));
    assert!(instruction.contains("exactly one complete JSON decision"));
    assert!(
        model.messages[1]
            .iter()
            .any(|m| m.role == "user" && m.content.contains("protocol_error"))
    );
}

#[test]
fn malformed_decisions_stop_at_transport_budget_without_dispatch() {
    let mut model = script(vec![
        "{\"type\":\"complete\",\"content\":\"first\nsecond\"}"
            .into();
        3
    ]);
    let (mut session, calls) = session(vec![]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::NoProgress);
    assert_eq!(result.model_invocations, 3);
    assert_eq!(result.inspections, 0);
    assert_eq!(result.proposal_attempts, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let evidence = result.last_result.unwrap();
    assert_eq!(
        field(&evidence, "status").and_then(ToolValue::as_str),
        Some("protocol_error")
    );
}

#[test]
fn zero_malformed_budget_preserves_immediate_stop() {
    let mut model = script(vec!["not json".into()]);
    let (mut session, calls) = session(vec![]);
    let mut budgets = EditBudgets::default();
    budgets.transport.max_malformed = 0;
    let result = run(&mut model, &mut session, true, budgets);
    assert_eq!(result.outcome, EditOutcome::NoProgress);
    assert_eq!(result.model_invocations, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn invalid_inspection_arguments_receive_bounded_feedback() {
    let mut model = script(vec![
        tool("read", 0),
        tool("read", 1),
        complete(),
        tool("edit", 0),
    ]);
    let (mut session, calls) = session(vec![
        ToolValue::object([
            ("status", "invalid_arguments".into()),
            ("message", "unknown invocation field: prefix".into()),
        ]),
        value("read_only"),
        ToolValue::object([("status", "proposal".into()), ("proposal_id", "1".into())]),
    ]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.inspections, 2);
    assert_eq!(result.corrections, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(
        model.messages[1]
            .iter()
            .any(|m| m.role == "user" && m.content.contains("unknown invocation field: prefix"))
    );
}

#[test]
fn invalid_arguments_changed_calls_recover_despite_identical_errors() {
    for inspecting in [false, true] {
        let name = if inspecting { "read" } else { "edit" };
        let mut responses = vec![tool(name, 0), tool(name, 1)];
        let error = ToolValue::object([
            ("status", "invalid_arguments".into()),
            ("message", "unknown invocation field".into()),
        ]);
        let mut replies = vec![error.clone(), error];
        if inspecting {
            responses.extend([tool("read", 2), tool("edit", 0)]);
            replies.push(value("read_only"));
        } else {
            responses.push(tool("edit", 2));
        }
        replies.push(proposal());
        let mut model = script(responses);
        let (mut session, calls) = session(replies);
        let result = run(
            &mut model,
            &mut session,
            inspecting,
            EditBudgets {
                corrections: 2,
                ..Default::default()
            },
        );
        assert_eq!(result.outcome, EditOutcome::ProposalReady);
        assert_eq!(result.corrections, if inspecting { 0 } else { 2 });
        assert_eq!(calls.load(Ordering::SeqCst), if inspecting { 4 } else { 3 });
        assert!(!result.trace.contains(&EditEvent::RepeatedCall));
        for messages in &model.messages[1..=2] {
            assert!(
                messages
                    .iter()
                    .any(|message| { message.content.contains("unknown invocation field") })
            );
        }
    }
}

#[test]
fn identical_invalid_arguments_call_stops_without_redispatch() {
    for inspecting in [false, true] {
        let name = if inspecting { "read" } else { "edit" };
        let mut model = script(vec![tool(name, 0), tool(name, 0)]);
        let error = ToolValue::object([
            ("status", "invalid_arguments".into()),
            ("message", "unknown invocation field".into()),
        ]);
        let (mut session, calls) = session(vec![error.clone()]);
        let result = run(&mut model, &mut session, inspecting, EditBudgets::default());
        assert_eq!(result.outcome, EditOutcome::NoProgress);
        assert_eq!(result.last_result, Some(error));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(model.calls, 2);
        assert_eq!(result.summary().duplicate_calls_suppressed, 1);
        assert_eq!(result.corrections, if inspecting { 0 } else { 1 });
    }
}

#[test]
fn invalid_proposal_arguments_stop_at_correction_budget() {
    let mut model = script(vec![tool("edit", 0), tool("edit", 1)]);
    let error = ToolValue::object([
        ("status", "invalid_arguments".into()),
        ("message", "unknown invocation field".into()),
    ]);
    let (mut session, calls) = session(vec![error.clone(), error]);
    let result = run(&mut model, &mut session, false, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ValidationFailure);
    assert_eq!(result.corrections, 1);
    assert_eq!(result.proposal_attempts, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!result.trace.contains(&EditEvent::RepeatedCall));
    assert_eq!(
        field(result.last_result.as_ref().unwrap(), "refinement_stopped")
            .and_then(ToolValue::as_str),
        Some("correction_budget_exhausted")
    );
}

#[test]
fn repeated_invalid_inspection_arguments_exhaust_budget() {
    let mut model = script((0..3).map(|n| tool("read", n)).collect());
    let (mut session, calls) = session(vec![
        ToolValue::object([
            ("status", "invalid_arguments".into()),
            ("message", "unknown invocation field".into())
        ]);
        3
    ]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ValidationFailure);
    assert_eq!(result.model_invocations, 3);
    assert_eq!(result.proposal_attempts, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(result.proposal.is_none());
}

fn evaluation(outcome: &str) -> ToolValue {
    ToolValue::object([
        ("status", "read_only".into()),
        ("value", "provider candidate evidence".into()),
        (
            "evaluation",
            ToolValue::object([(
                "checks",
                ToolValue::array([ToolValue::object([
                    ("id", "required behavior".into()),
                    ("outcome", outcome.into()),
                    (
                        "actual",
                        ToolValue::object([
                            ("input", ToolValue::array([2i64.into()])),
                            ("expected", ToolValue::array([16i64.into()])),
                            ("actual", ToolValue::array([8i64.into()])),
                        ]),
                    ),
                ])]),
            )]),
        ),
    ])
}

#[test]
fn failed_evaluation_feedback_revised_candidate_and_existing_proposal_phase() {
    let mut model = script(vec![
        tool("read", 0),
        tool("read", 1),
        complete(),
        tool("edit", 1),
    ]);
    let (mut session, calls) =
        session(vec![evaluation("Failed"), evaluation("Passed"), proposal()]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(result.inspections, 2);
    assert_eq!(result.corrections, 0); // Candidate retries are inspections, not proposal corrections.
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let feedback = model.messages[1]
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(feedback.contains("Revise the candidate and evaluate again"));
    assert!(feedback.contains("expected"));
    assert!(feedback.contains("16"));
    assert!(feedback.contains("Failed"));
    let feedback: serde_json::Value = model.messages[1]
        .iter()
        .filter_map(|m| serde_json::from_str::<serde_json::Value>(&m.content).ok())
        .find(|v| v["kind"] == "candidate_evaluation_feedback")
        .unwrap();
    assert_candidate_feedback_lifecycle(&feedback);
    assert_eq!(
        feedback["evaluation"],
        to_json(field(&evaluation("Failed"), "evaluation").unwrap().clone())
    );
    assert!(feedback["checks"].is_null());
    assert!(feedback.get("observations").is_none());
    let proposal_context = model.messages[3]
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(proposal_context.contains("Passed"));
    assert!(!proposal_context.contains("candidate_evaluation_feedback"));
}

fn assert_candidate_feedback_lifecycle(feedback: &serde_json::Value) {
    let lifecycle = feedback["candidate_lifecycle"].as_str().unwrap();
    assert!(lifecycle.contains("Disposable evaluation only"));
    assert!(lifecycle.contains("candidate definitions are not installed"));
    assert!(lifecycle.contains("active source is unchanged"));
    assert_eq!(feedback["remaining_refinements"], 2);
    assert_eq!(feedback["next_action"], "revise_candidate_and_evaluate");
    assert_eq!(
        feedback["next_permissible_actions"],
        json!([
            "evaluate a revised candidate",
            "inspect existing document definitions if needed",
            "complete to advance to Propose only when evidence supports the candidate"
        ])
    );
}

#[test]
fn reordered_nested_candidate_objects_are_the_same_call() {
    // Raw JSON preserves the distinct wire order instead of letting json! normalize it.
    let mut model = script(vec![
        r#"{"type":"tool","name":"read","arguments":{"candidate":{"source":"new","expectations":{"output":[1,2],"options":{"strict":true,"mode":"exact"}}}}}"#.into(),
        r#"{"type":"tool","name":"read","arguments":{"candidate":{"expectations":{"options":{"mode":"exact","strict":true},"output":[1,2]},"source":"new"}}}"#.into(),
    ]);
    let (mut session, calls) = session(vec![generic_evaluation("Violated")]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::NoProgress);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.inspections, 1);
    assert_eq!(result.summary().duplicate_calls_suppressed, 1);
    assert!(result.trace.contains(&EditEvent::NoProgressReason {
        reason: "repeated_suppressed_operation".into(),
    }));
    assert_eq!(result.proposal_attempts, 0);
}

#[test]
fn candidate_source_expectations_and_host_state_each_restore_dispatch_eligibility() {
    for changed in ["source", "expectations", "state"] {
        let candidate_call = |source: &str, expected: i64| {
            json!({"type":"tool", "name":"read", "arguments":{
                "candidate":{"source":source,"expectations":{"output":[expected]}}
            }})
            .to_string()
        };
        let mut model = script(vec![
            candidate_call("new", 1),
            candidate_call(
                if changed == "source" {
                    "revised"
                } else {
                    "new"
                },
                if changed == "expectations" { 2 } else { 1 },
            ),
            complete(),
            tool("edit", 0),
        ]);
        let (mut session, calls) = session(vec![
            generic_evaluation("Violated"),
            generic_evaluation("Satisfied"),
            proposal(),
        ]);
        // Advance host state only after the first provider dispatch, not on reads
        // of the token (continuation refresh also reads it).
        let observed_calls = calls.clone();
        let result = run_edit_session_with_state(
            &mut model,
            &mut session,
            Ok(SourceId::Scratch(1)),
            "request",
            &caps(true),
            EditBudgets::default(),
            EditDispatch {
                policy: &mut |_, _| true,
                relevant_state: &mut |_| {
                    ToolValue::object([(
                        "revision",
                        if changed == "state" && observed_calls.load(Ordering::SeqCst) > 0 {
                            2i64.into()
                        } else {
                            1i64.into()
                        },
                    )])
                },
            },
        );
        assert_eq!(result.outcome, EditOutcome::ProposalReady, "{changed}");
        assert_eq!(calls.load(Ordering::SeqCst), 3, "{changed}");
        assert_eq!(result.inspections, 2, "{changed}");
        assert_eq!(result.summary().duplicate_calls_suppressed, 0, "{changed}");
    }
}

#[test]
fn evaluation_only_does_not_install_source_or_allocate_a_proposal() {
    for candidate in ["wrong", "new"] {
        let mut budgets = EditBudgets::default();
        budgets.model_invocations = 1;
        let (result, model, controller, target) =
            refinement_run(vec![semantic_call("evaluate", candidate)], budgets, true);
        assert_eq!(result.outcome, EditOutcome::OperationBudgetExhausted);
        assert_eq!(model.calls, 1);
        assert_eq!(result.evaluations, 1);
        assert_eq!(result.proposal_attempts, 0);
        assert_eq!(result.proposal, None);
        assert!(controller.get(ProposalId(1)).is_err());
        assert_eq!(target.text(), "old");
        assert_eq!(target.version(), 7);
    }
}

#[test]
fn identical_failed_candidate_is_suppressed() {
    let mut model = script(vec![tool("read", 0), tool("read", 0)]);
    let (mut session, calls) = session(vec![evaluation("Failed")]);
    let result = run(&mut model, &mut session, true, EditBudgets::default());
    assert_eq!(result.outcome, EditOutcome::NoProgress);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.proposal_attempts, 0);
}

#[test]
fn unsuccessful_evaluations_exhaust_inspection_budget_without_proposing() {
    for outcome in ["Failed", "Inconclusive", "Error", "NotEvaluated"] {
        let mut model = script(vec![tool("read", 0), tool("read", 1), tool("read", 2)]);
        let (mut session, calls) = session(vec![evaluation(outcome); 3]);
        let result = run(&mut model, &mut session, true, EditBudgets::default());
        assert_eq!(result.outcome, EditOutcome::ValidationFailure);
        assert_eq!(result.inspections, 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(result.proposal_attempts, 0);
        assert_eq!(result.proposal, None);
        assert!(
            !result
                .trace
                .iter()
                .any(|e| matches!(e, EditEvent::ValidProposalReached(_)))
        );
    }
}

fn generic_evaluation(outcome: &str) -> ToolValue {
    ToolValue::object([
        ("status", "read_only".into()),
        (
            "evaluation",
            ToolValue::object([
                ("outcome", outcome.into()),
                ("summary", "provider-scoped observation".into()),
                (
                    "diagnostics",
                    ToolValue::array([ToolValue::object([(
                        "nested",
                        ToolValue::array([true.into(), 16i64.into(), "detail".into()]),
                    )])]),
                ),
                (
                    "failed_criteria",
                    ToolValue::array(["required criterion".into()]),
                ),
                ("contract_id", "contract-42".into()),
            ]),
        ),
    ])
}

#[test]
fn generic_evaluation_feedback_and_satisfied_continuation() {
    for outcome in ["Violated", "Inconclusive"] {
        let mut model = script(vec![
            tool("read", 0),
            tool("read", 1),
            complete(),
            tool("edit", 1),
        ]);
        let evidence = generic_evaluation(outcome);
        let (mut session, calls) = session(vec![
            evidence.clone(),
            generic_evaluation("Satisfied"),
            proposal(),
        ]);
        let result = run(&mut model, &mut session, true, EditBudgets::default());
        assert_eq!(result.outcome, EditOutcome::ProposalReady);
        assert_eq!(result.inspections, 2);
        assert_eq!(result.corrections, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let feedback: serde_json::Value = model.messages[1]
            .iter()
            .filter_map(|m| serde_json::from_str::<serde_json::Value>(&m.content).ok())
            .find(|v| v["kind"] == "candidate_evaluation_feedback")
            .unwrap();
        assert_eq!(
            feedback["evaluation"],
            to_json(field(&evidence, "evaluation").unwrap().clone())
        );
        assert_candidate_feedback_lifecycle(&feedback);
        assert!(feedback["checks"].is_null());
        assert!(feedback.get("observations").is_none());
        let instruction = feedback["instruction"].as_str().unwrap();
        if outcome == "Violated" {
            assert!(instruction.contains("Revise the candidate and evaluate again"));
        } else {
            assert!(instruction.contains("Resolve the validation uncertainty"));
            assert!(!instruction.contains("Revise the candidate"));
        }
        let context = model.messages[3]
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(context.contains("Satisfied"));
        assert!(!context.contains("candidate_evaluation_feedback"));
        assert!(
            !result
                .trace
                .iter()
                .any(|e| matches!(e, EditEvent::ValidProposalReached(_)))
        );
    }
}

#[test]
fn generic_unsatisfied_evaluations_preserve_inspection_and_duplicate_budgets() {
    for outcome in ["Violated", "Inconclusive"] {
        let mut model = script(vec![tool("read", 0), tool("read", 0)]);
        let (mut session, calls) = session(vec![generic_evaluation(outcome)]);
        let result = run(&mut model, &mut session, true, EditBudgets::default());
        assert_eq!(result.outcome, EditOutcome::NoProgress);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.summary().duplicate_calls_suppressed, 1);

        let mut model = script((0..3).map(|n| tool("read", n)).collect());
        let (mut session, calls) = self::session(vec![generic_evaluation(outcome); 3]);
        let result = run(&mut model, &mut session, true, EditBudgets::default());
        assert_eq!(result.outcome, EditOutcome::ValidationFailure);
        assert_eq!(result.inspections, 3);
        assert_eq!(result.proposal_attempts, 0);
        assert_eq!(result.corrections, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            field(result.last_result.as_ref().unwrap(), "refinement_stopped")
                .and_then(ToolValue::as_str),
            Some("inspection_budget_exhausted")
        );
    }
}

#[test]
fn generic_evaluation_evidence_is_invalidated_by_changed_source() {
    for outcome in ["Satisfied", "Violated", "Inconclusive"] {
        let mut continuation = EditContinuation::default();
        let call = ToolCall::new("read");
        let state = ToolValue::object([("version", "1".into()), ("source", "old".into())]);
        continuation.retain(call, state.clone(), generic_evaluation(outcome));
        continuation.feedback = Some("old feedback".into());
        continuation.refresh(&mut |_| state.clone());
        assert_eq!(continuation.evidence.len(), 1);
        continuation.refresh(&mut |_| {
            ToolValue::object([("version", "1".into()), ("source", "changed".into())])
        });
        assert!(continuation.evidence.is_empty());
        assert!(continuation.feedback.is_none());
        let context: serde_json::Value = serde_json::from_str(&continuation.context(
            EditPhase::Inspect,
            EditBudgets::default().refinements,
            false,
        ))
        .unwrap();
        assert_eq!(context["invalidation"]["removed_results"], 1);
    }
}

#[test]
fn generic_evaluation_transport_budget_is_not_no_progress() {
    let mut model = script(vec![tool("read", 0), tool("read", 1)]);
    let (mut session, calls) = session(vec![generic_evaluation("Violated")]);
    let mut budgets = EditBudgets::default();
    budgets.model_invocations = 1;
    let result = run(&mut model, &mut session, true, budgets);
    assert_eq!(result.outcome, EditOutcome::OperationBudgetExhausted);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.last_result, Some(generic_evaluation("Violated")));
}

#[test]
fn generic_satisfied_without_contract_advances_at_inspection_limit() {
    let mut value = generic_evaluation("Satisfied");
    if let ToolValue::Object(fields) = &mut value {
        if let Some(ToolValue::Object(evaluation)) = fields.get_mut("evaluation") {
            evaluation.remove("contract_id");
        }
    }
    let mut model = script(vec![tool("read", 0), tool("edit", 0)]);
    let (mut session, calls) = session(vec![value, proposal()]);
    let mut budgets = EditBudgets::default();
    budgets.inspections = 1;
    let result = run(&mut model, &mut session, true, budgets);
    assert_eq!(result.outcome, EditOutcome::ProposalReady);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(
        !result
            .trace
            .iter()
            .any(|e| matches!(e, EditEvent::ValidProposalReached(_)))
    );
}
