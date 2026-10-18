//! Adapter from registered language services to existing agent tools and M1 proposals.
//!
//! Edit runners can obtain purpose without invoking a tool: call `session.catalog()`,
//! then `catalog.lookup(tool_name)`, and read `entry.metadata` as a `ToolValue::Object`.
//! Its `purpose` string is always `"Exploration"` or `"CandidateEvaluation"` for
//! semantic tools. The same field appears at `tools[i].metadata.purpose` in
//! `SessionCatalog::to_value()`. Older/non-semantic metadata may omit it; treat that
//! as exploration. Purpose is independent of effect and `produces_evaluation`;
//! discovery alone neither authorizes execution nor guarantees evaluation output.
use super::*;
use crate::source_edit::{PermittedEditScope, SourceEditController, SourceSnapshot, SourceTarget};
use scope_language::{
    LanguageDocument, LanguagePath, LanguageService, SemanticOperationDescription,
    SemanticOperationEffect, SemanticOperationError, SemanticOperationRequest,
    SemanticOperationResult, execute_semantic_operation,
};
use serde_json::{Value, json};

/// Host envelope composed from the same typed fields used by language validation.
/// Opaque language constraints remain the language implementation's responsibility.
fn invocation_schema(operation: &SemanticOperationDescription) -> Value {
    let properties: serde_json::Map<String, Value> = operation
        .arguments
        .iter()
        .map(|field| {
            let kind = format!("{:?}", field.kind).to_lowercase();
            (
                field.name.clone(),
                json!({"type": kind, "description": field.description}),
            )
        })
        .collect();
    let required: Vec<_> = operation
        .arguments
        .iter()
        .filter(|field| field.required)
        .map(|field| field.name.as_str())
        .collect();
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["expected_version", "expected_source", "arguments"],
        "properties": {
            "expected_version": {"type":"string", "description":"Copy the host edit_request revision exactly; decimal u64"},
            "expected_source": {"type":"string", "description":"Copy the host edit_request source exactly"},
            "arguments": {"type":"object", "additionalProperties":false,
                "properties":properties, "required":required}
        }
    })
}

fn invalid_invocation(operation: &SemanticOperationDescription, message: &str) -> ToolResult {
    ToolResult::new(ToolValue::object([
        ("status", "invalid_arguments".into()),
        ("message", format!("Tool invocation rejected: {message}. Language fields belong inside arguments. Retry using invocation_schema; preserve host source preconditions exactly.").into()),
        ("invocation_schema", from_json(invocation_schema(operation)).expect("schema is representable")),
    ]))
}

/// Host policy is required even when the enclosing agent session permits a tool.
pub struct SemanticAuthorization<'a> {
    pub operation: &'a SemanticOperationDescription,
    pub source: SourceSnapshot<'a>,
    pub arguments: &'a Value,
}

/// Bound to one previously resolved, host-owned target. No mutable source or App
/// is supplied to the language. Dropping the session releases the review controller.
pub struct SemanticToolProvider<'a, T: SourceTarget, P> {
    service: &'a dyn LanguageService,
    target: &'a T,
    controller: &'a mut SourceEditController,
    scope: PermittedEditScope,
    selection: Option<crate::text::TextRange>,
    policy: P,
    operations: Vec<SemanticOperationDescription>,
    tools: Vec<ToolDefinition>,
}
impl<'a, T: SourceTarget, P: for<'b> FnMut(SemanticAuthorization<'b>) -> bool>
    SemanticToolProvider<'a, T, P>
{
    pub fn new(
        service: &'a dyn LanguageService,
        target: &'a T,
        controller: &'a mut SourceEditController,
        scope: PermittedEditScope,
        selection: Option<crate::text::TextRange>,
        policy: P,
    ) -> Result<Self, ToolError> {
        if target.language_id() != service.language_id() {
            return Err(ToolError::Denied {
                tool: "language.semantic".into(),
            });
        }
        let operations = service.semantic_operations();
        let mut tools = Vec::new();
        for operation in &operations {
            let name = format!("language.{}.{}", service.language_id(), operation.id);
            if operation.id.is_empty() || tools.iter().any(|t: &ToolDefinition| t.name == name) {
                return Err(ToolError::DuplicateTool { name });
            }
            // The existing tool protocol supports object arguments. Include the
            // serializable language schema rather than inventing another registry.
            let schema =
                serde_json::to_string(&invocation_schema(operation)["properties"]["arguments"])
                    .map_err(|e| ToolError::execution(&name, e.to_string()))?;
            let description = if operation.effect == SemanticOperationEffect::Read
                && operation.purpose()
                    == scope_language::SemanticOperationPurpose::CandidateEvaluation
            {
                format!(
                    "{} Executes a disposable candidate; does not install its definitions or modify active source. Evidence may support a later proposal, not an applied edit.",
                    operation.description
                )
            } else {
                operation.description.clone()
            };
            tools.push(
                ToolDefinition::new(name, &description)
                    .effect(match operation.effect {
                        SemanticOperationEffect::Read => ToolEffect::Read,
                        SemanticOperationEffect::ProposeChanges => ToolEffect::Write,
                    })
                    .input(ToolInputDefinition::required(
                        "expected_version",
                        ToolInputKind::String,
                        "Exact decimal revision of the resolved target",
                    ))
                    .input(ToolInputDefinition::required(
                        "expected_source",
                        ToolInputKind::String,
                        "Exact original source precondition",
                    ))
                    .input(ToolInputDefinition::required(
                        "arguments",
                        ToolInputKind::Object,
                        schema,
                    )),
            );
        }
        Ok(Self {
            service,
            target,
            controller,
            scope,
            selection,
            policy,
            operations,
            tools,
        })
    }
}
impl<T: SourceTarget, P: for<'b> FnMut(SemanticAuthorization<'b>) -> bool> ToolProvider
    for SemanticToolProvider<'_, T, P>
{
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
    fn provider_name(&self) -> Option<&str> {
        Some(self.service.language_id())
    }
    fn metadata(&self, tool_name: &str) -> Option<ToolValue> {
        let index = self.tools.iter().position(|tool| tool.name == tool_name)?;
        let operation = &self.operations[index];
        let mut description = operation.clone();
        // Constraints are opaque language JSON; preserve numbers beyond the
        // existing agent protocol's signed-integer value representation.
        description.constraints = None;
        let mut value = serde_json::to_value(description).ok()?;
        value.as_object_mut()?.insert(
            "purpose".into(),
            serde_json::to_value(operation.purpose()).ok()?,
        );
        value.as_object_mut()?.insert(
            "produces_evaluation".into(),
            self.service
                .semantic_operation_produces_evaluation(&operation.id)
                .into(),
        );
        value.as_object_mut()?.insert(
            "constraints_json".into(),
            operation
                .constraints
                .as_ref()
                .map(|v| Value::String(v.to_string()))
                .unwrap_or(Value::Null),
        );
        value
            .as_object_mut()?
            .insert("invocation_schema".into(), invocation_schema(operation));
        from_json(value)
    }
    fn output_description(&self, _: &str) -> Option<&str> {
        Some(
            "Structured status: blocked (blocker_category, recoverable, missing_dependencies), rejected (validation_status, diagnostics, correction_eligible), proposal (proposal_id, validation_status, diagnostics, limitations, missing_dependencies), read_only, invalid_arguments, unsupported_operation, invalid_source (diagnostics), ambiguous_target, revision_conflict, unauthorized, internal_failure. Proposals require explicit host review and acceptance.",
        )
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let operation_id = self
            .tools
            .iter()
            .position(|tool| tool.name == call.name)
            .map(|index| self.operations[index].id.clone());
        let mut result = self.invoke_operation(call)?;
        if let (Some(id), ToolValue::Object(fields)) = (operation_id, &mut result.value) {
            fields.insert("operation_id".into(), id.into());
        }
        Ok(result)
    }
}
impl<T: SourceTarget, P: for<'b> FnMut(SemanticAuthorization<'b>) -> bool>
    SemanticToolProvider<'_, T, P>
{
    fn invoke_operation(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let Some(index) = self.tools.iter().position(|t| t.name == call.name) else {
            return Err(ToolError::UnknownTool { name: call.name });
        };
        if let Some(field) = call.arguments.keys().find(|k| {
            !matches!(
                k.as_str(),
                "expected_version" | "expected_source" | "arguments"
            )
        }) {
            return Ok(invalid_invocation(
                &self.operations[index],
                &format!(
                    "unknown invocation field: {field}; only expected_version, expected_source, arguments are allowed; move {field} under arguments if advertised there"
                ),
            ));
        }
        let Some(version) = call
            .arguments
            .get("expected_version")
            .and_then(ToolValue::as_str)
            .and_then(|v| v.parse::<u64>().ok())
        else {
            return Ok(invalid_invocation(
                &self.operations[index],
                "expected_version must be a decimal u64 string",
            ));
        };
        let Some(original) = call
            .arguments
            .get("expected_source")
            .and_then(ToolValue::as_str)
        else {
            return Ok(invalid_invocation(
                &self.operations[index],
                "expected_source must be a string",
            ));
        };
        let Some(arguments) = call.arguments.get("arguments") else {
            return Ok(invalid_invocation(
                &self.operations[index],
                "arguments required",
            ));
        };
        let arguments = to_json(arguments.clone());
        let operation = &self.operations[index];
        if let Err(error) = operation.validate_arguments(&arguments) {
            return Ok(invalid_invocation(operation, &error.to_string()));
        }
        let source = self.target.snapshot();
        if self.scope != PermittedEditScope::Document(source.id) {
            return Ok(outcome("unauthorized", "target outside permitted scope"));
        }
        if version != source.version || original != source.text {
            return Ok(outcome(
                "revision_conflict",
                "target revision or source changed",
            ));
        }
        if !(self.policy)(SemanticAuthorization {
            operation,
            source,
            arguments: &arguments,
        }) {
            return Ok(outcome("unauthorized", "host policy denied operation"));
        }
        if let Some(selection) = self.selection {
            if let Err(error) = crate::text::TextBuffer::new(source.text).span_for_range(selection)
            {
                return Ok(outcome("invalid_arguments", &error.to_string()));
            }
        }
        let result = execute_semantic_operation(
            self.service,
            SemanticOperationRequest {
                operation_id: &operation.id,
                document: LanguageDocument::new(LanguagePath::new(""), source.text, source.version),
                selection: self.selection,
                arguments: arguments.clone(),
            },
        );
        let live = self.target.snapshot();
        if live.id != source.id || live.version != version || live.text != original {
            return Ok(outcome(
                "revision_conflict",
                "target changed during operation",
            ));
        }
        Ok(match result {
            Err(error) => language_error(error),
            Ok(SemanticOperationResult::ReadOnly { value, rationale }) => match from_json(value) {
                Some(value) => ToolResult::new(ToolValue::object([
                    ("status", "read_only".into()),
                    ("operation_id", operation.id.clone().into()),
                    ("value", value),
                    ("rationale", rationale.into()),
                ])),
                None => outcome(
                    "internal_failure",
                    "result not representable by agent tool protocol",
                ),
            },
            Ok(SemanticOperationResult::ReadOnlyEvaluation {
                value,
                rationale,
                evaluation,
            }) => {
                let checks: Vec<_> = evaluation
                    .checks
                    .into_iter()
                    .map(|(id, check)| {
                        json!({
                            "id": id,
                            "outcome": format!("{:?}", check.outcome),
                            "actual": check.actual,
                        })
                    })
                    .collect();
                match from_json(json!({
                    "status": "read_only",
                    "operation_id": operation.id,
                    "value": value,
                    "rationale": rationale,
                    "evaluation": { "checks": checks },
                })) {
                    Some(value) => ToolResult::new(value),
                    None => outcome(
                        "internal_failure",
                        "result not representable by agent tool protocol",
                    ),
                }
            }
            Ok(SemanticOperationResult::Evaluated {
                value,
                rationale,
                evaluation,
            }) => {
                // Bind a reported contract to the immutable host contract. These
                // observations remain feedback, never proposal authorization.
                if evaluation.contract_id.as_deref().is_some_and(|id| {
                    self.controller
                        .validation_contract()
                        .is_none_or(|contract| contract.id != id)
                }) {
                    return Ok(outcome(
                        "internal_failure",
                        "evaluation contract identity mismatch",
                    ));
                }
                match from_json(json!({
                    "status": "read_only",
                    "operation_id": operation.id,
                    "value": value,
                    "rationale": rationale,
                    "evaluation": evaluation,
                })) {
                    Some(value) => ToolResult::new(value),
                    None => outcome(
                        "internal_failure",
                        "result not representable by agent tool protocol",
                    ),
                }
            }
            Ok(SemanticOperationResult::Blocked { blocker }) => {
                if blocker.operation_id != operation.id {
                    return Ok(outcome(
                        "internal_failure",
                        "blocker operation identity mismatch",
                    ));
                }
                let category = match blocker.category {
                    scope_language::BlockerCategory::MissingDependency => {
                        "MissingDependency".into()
                    }
                    scope_language::BlockerCategory::UnsupportedConstruct => {
                        "UnsupportedConstruct".into()
                    }
                    scope_language::BlockerCategory::UnresolvedTarget => "UnresolvedTarget".into(),
                    scope_language::BlockerCategory::UnsatisfiedPrecondition => {
                        "UnsatisfiedPrecondition".into()
                    }
                    scope_language::BlockerCategory::InsufficientContext => {
                        "InsufficientContext".into()
                    }
                    scope_language::BlockerCategory::Other(code) => code,
                };
                ToolResult::new(ToolValue::object([
                    ("status", "blocked".into()),
                    ("operation_id", operation.id.clone().into()),
                    ("blocker_category", category.into()),
                    ("message", blocker.explanation.into()),
                    ("recoverable", blocker.recoverable.into()),
                    (
                        "missing_dependencies",
                        strings(blocker.missing_dependencies),
                    ),
                    ("diagnostics", diagnostics(blocker.diagnostics)),
                    (
                        "suggested_next_action",
                        blocker
                            .suggested_next_action
                            .map(Into::into)
                            .unwrap_or(ToolValue::Null),
                    ),
                ]))
            }
            Ok(
                SemanticOperationResult::Changes { .. }
                | SemanticOperationResult::ValidatedChanges { .. },
            ) if operation.effect == SemanticOperationEffect::Read => outcome(
                "internal_failure",
                "read operation returned proposed changes",
            ),
            Ok(
                changes @ (SemanticOperationResult::Changes { .. }
                | SemanticOperationResult::ValidatedChanges { .. }),
            ) => {
                let (edits, rationale, mut validation) = match changes {
                    SemanticOperationResult::Changes { edits, rationale } => {
                        (edits, rationale, Default::default())
                    }
                    SemanticOperationResult::ValidatedChanges {
                        edits,
                        rationale,
                        validation,
                    } => (edits, rationale, validation),
                    _ => unreachable!(),
                };
                let structural = edits
                    .iter()
                    .map(|edit| TextEdit::replace(edit.range, edit.replacement.clone()))
                    .collect();
                let prepared = self.controller.next().and_then(|id| {
                    crate::proposal::SourceEditProposal::new(
                        id,
                        self.target,
                        structural,
                        Some(rationale),
                    )
                    .map_err(|e| crate::source_edit::ControllerError::Validation(vec![e]))
                });
                let mut proposal = match prepared {
                    Ok(proposal) => proposal,
                    Err(error) => return Ok(outcome("invalid_arguments", &error.to_string())),
                };
                let candidate = match proposal.preview(self.target) {
                    Ok(candidate) => candidate,
                    Err(error) => return Ok(outcome("invalid_arguments", &format!("{error:?}"))),
                };
                if validation.status == scope_language::ValidationStatus::NotEvaluated {
                    validation = match self.service.validate_semantic_candidate(
                        scope_language::SemanticValidationRequest {
                            operation_id: &operation.id,
                            document: LanguageDocument::new(
                                LanguagePath::new(""),
                                source.text,
                                source.version,
                            ),
                            candidate_source: &candidate,
                            edits: &edits,
                            selection: self.selection,
                            arguments: &arguments,
                            document_id: &format!("{:?}", source.id),
                        },
                    ) {
                        Ok(validation) => validation,
                        Err(error) => return Ok(language_error(error)),
                    };
                }
                let rejected = validation.status == scope_language::ValidationStatus::Rejected;
                let mut fields = std::collections::BTreeMap::from([
                    (
                        "status".into(),
                        if rejected { "rejected" } else { "proposal" }.into(),
                    ),
                    ("operation_id".into(), operation.id.clone().into()),
                    (
                        "validation_status".into(),
                        format!("{:?}", validation.status).into(),
                    ),
                    (
                        "diagnostics".into(),
                        diagnostics(validation.diagnostics.clone()),
                    ),
                    (
                        "limitations".into(),
                        strings(validation.limitations.clone()),
                    ),
                    (
                        "missing_dependencies".into(),
                        strings(validation.missing_dependencies.clone()),
                    ),
                    ("correction_eligible".into(), rejected.into()),
                ]);
                fields.insert(
                    "rationale".into(),
                    proposal.rationale().unwrap_or_default().into(),
                );
                if !rejected {
                    let live = self.target.snapshot();
                    if live.id != source.id || live.version != version || live.text != original {
                        return Ok(outcome(
                            "revision_conflict",
                            "target changed during validation",
                        ));
                    }
                    if !(self.policy)(SemanticAuthorization {
                        operation,
                        source: live,
                        arguments: &arguments,
                    }) {
                        return Ok(outcome(
                            "unauthorized",
                            "host policy denied proposal registration",
                        ));
                    }
                    proposal.set_validation(validation);
                    if let Some(contract) = self.controller.validation_contract().cloned() {
                        let observations = match self.service.validate_semantic_contract(
                            scope_language::SemanticValidationRequest {
                                operation_id: &operation.id,
                                document: LanguageDocument::new(
                                    LanguagePath::new(""),
                                    source.text,
                                    source.version,
                                ),
                                candidate_source: &candidate,
                                edits: &edits,
                                selection: self.selection,
                                arguments: &arguments,
                                document_id: &format!("{:?}", source.id),
                            },
                            &contract,
                        ) {
                            Ok(observations) => observations,
                            Err(error) => scope_language::ContractValidation {
                                checks: contract
                                    .checks
                                    .keys()
                                    .map(|id| {
                                        (
                                            id.clone(),
                                            scope_language::CheckObservation {
                                                outcome: scope_language::CheckOutcome::Error,
                                                actual: Value::String(error.to_string()),
                                            },
                                        )
                                    })
                                    .collect(),
                            },
                        };
                        fields.insert("contract_id".into(), contract.id.clone().into());
                        let unresolved: Vec<_> = contract
                            .checks
                            .keys()
                            .filter(|id| {
                                observations.checks.get(*id).is_none_or(|check| {
                                    check.outcome != scope_language::CheckOutcome::Passed
                                })
                            })
                            .cloned()
                            .collect();
                        fields.insert("unresolved_obligations".into(), strings(unresolved));
                        fields.insert(
                            "assurance_policy".into(),
                            self.controller
                                .task_contract()
                                .map(|task| format!("{:?}", task.policy))
                                .unwrap_or_else(|| "StrictLegacyContract".into())
                                .into(),
                        );
                        if let Some(task) = self.controller.task_contract() {
                            let context = task.context();
                            match from_json(context.clone()) {
                                Some(context) => {
                                    fields.insert("task_contract".into(), context);
                                }
                                None => {
                                    fields.insert(
                                        "task_contract_json".into(),
                                        context.to_string().into(),
                                    );
                                }
                            }
                        }
                        fields.insert("evidence_scope".into(), "Only the specified checks on this exact candidate and source revision; not universal correctness".into());
                        fields.insert(
                            "checks".into(),
                            ToolValue::Array(
                                contract
                                    .checks
                                    .keys()
                                    .chain(observations.checks.keys())
                                    .collect::<std::collections::BTreeSet<_>>()
                                    .into_iter()
                                    .map(|id| {
                                        let check = observations.checks.get(id);
                                        ToolValue::object([
                                            ("observed", check.is_some().into()),
                                            ("id", id.clone().into()),
                                            (
                                                "outcome",
                                                format!(
                                                    "{:?}",
                                                    check.map(|check| check.outcome).unwrap_or(
                                                        scope_language::CheckOutcome::NotEvaluated
                                                    )
                                                )
                                                .into(),
                                            ),
                                            (
                                                "expected_json",
                                                contract
                                                    .checks
                                                    .get(id)
                                                    .unwrap_or(&Value::Null)
                                                    .to_string()
                                                    .into(),
                                            ),
                                            (
                                                "actual_json",
                                                check
                                                    .map(|check| &check.actual)
                                                    .unwrap_or(&Value::Null)
                                                    .to_string()
                                                    .into(),
                                            ),
                                        ])
                                    })
                                    .collect(),
                            ),
                        );
                        proposal.set_evidence(crate::proposal::ValidationEvidence {
                            operation: operation.id.clone(),
                            arguments: arguments.clone(),
                            contract,
                            observations,
                            target: source.id,
                            version: source.version,
                            original: source.text.into(),
                            candidate: candidate.clone(),
                        });
                        // The contract hook is another language boundary. Recheck
                        // authority and source after it, independently of success.
                        let live = self.target.snapshot();
                        if live.id != source.id || live.version != version || live.text != original
                        {
                            return Ok(outcome(
                                "revision_conflict",
                                "target changed during contract validation",
                            ));
                        }
                        if !(self.policy)(SemanticAuthorization {
                            operation,
                            source: live,
                            arguments: &arguments,
                        }) {
                            return Ok(outcome(
                                "unauthorized",
                                "host policy denied proposal registration",
                            ));
                        }
                    }
                    match self.controller.register(proposal, self.target) {
                        Ok(id) => {
                            fields.insert("proposal_id".into(), id.0.to_string().into());
                        }
                        Err(crate::source_edit::ControllerError::RequiredValidation(reason)) => {
                            fields.insert("status".into(), "validation_failed".into());
                            fields.insert("reason".into(), format!("{reason:?}").into());
                            fields.insert("correction_eligible".into(), true.into());
                            fields.insert("message".into(), "Revise the candidate; the final preview must pass every host-required check".into());
                        }
                        Err(error) => return Ok(outcome("internal_failure", &error.to_string())),
                    }
                }
                ToolResult::new(ToolValue::Object(fields))
            }
        })
    }
}
fn strings(values: Vec<String>) -> ToolValue {
    ToolValue::array(values.into_iter().map(ToolValue::from))
}
fn diagnostics(values: Vec<scope_language::Diagnostic>) -> ToolValue {
    ToolValue::array(values.into_iter().map(|d| {
        ToolValue::object([
            ("message", d.message.into()),
            ("severity", format!("{:?}", d.severity).into()),
            ("code", d.code.map(Into::into).unwrap_or(ToolValue::Null)),
            (
                "source",
                d.source.map(Into::into).unwrap_or(ToolValue::Null),
            ),
            ("start_line", d.range.start.line.into()),
            ("start_column", d.range.start.column.into()),
            ("end_line", d.range.end.line.into()),
            ("end_column", d.range.end.column.into()),
        ])
    }))
}
fn outcome(status: &str, message: &str) -> ToolResult {
    ToolResult::new(ToolValue::object([
        ("status", status.into()),
        ("message", message.into()),
    ]))
}
fn language_error(error: SemanticOperationError) -> ToolResult {
    match error {
        SemanticOperationError::InvalidArguments(message) => outcome("invalid_arguments", &message),
        SemanticOperationError::UnsupportedOperation(message) => {
            outcome("unsupported_operation", &message)
        }
        SemanticOperationError::AmbiguousTarget(message) => outcome("ambiguous_target", &message),
        SemanticOperationError::InternalFailure(message) => outcome("internal_failure", &message),
        SemanticOperationError::InvalidSource(diagnostics) => ToolResult::new(ToolValue::object([
            ("status", "invalid_source".into()),
            (
                "diagnostics",
                ToolValue::array(diagnostics.into_iter().map(|d| {
                    ToolValue::object([
                        ("message", d.message.into()),
                        ("severity", format!("{:?}", d.severity).into()),
                        ("start_line", d.range.start.line.into()),
                        ("start_column", d.range.start.column.into()),
                        ("end_line", d.range.end.line.into()),
                        ("end_column", d.range.end.column.into()),
                        (
                            "source",
                            d.source.map(Into::into).unwrap_or(ToolValue::Null),
                        ),
                        ("code", d.code.map(Into::into).unwrap_or(ToolValue::Null)),
                    ])
                })),
            ),
        ])),
    }
}
fn to_json(value: ToolValue) -> Value {
    match value {
        ToolValue::Null => Value::Null,
        ToolValue::Boolean(v) => v.into(),
        ToolValue::Integer(v) => v.into(),
        ToolValue::String(v) => v.into(),
        ToolValue::Array(v) => Value::Array(v.into_iter().map(to_json).collect()),
        ToolValue::Object(v) => {
            Value::Object(v.into_iter().map(|(k, v)| (k, to_json(v))).collect())
        }
    }
}
fn from_json(value: Value) -> Option<ToolValue> {
    Some(match value {
        Value::Null => ToolValue::Null,
        Value::Bool(v) => v.into(),
        Value::Number(v) => ToolValue::Integer(v.as_i64()?),
        Value::String(v) => v.into(),
        Value::Array(v) => ToolValue::Array(v.into_iter().map(from_json).collect::<Option<_>>()?),
        Value::Object(v) => ToolValue::Object(
            v.into_iter()
                .map(|(k, v)| Some((k, from_json(v)?)))
                .collect::<Option<_>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_schema_tracks_different_language_argument_types() {
        use scope_language::{SemanticArgument, SemanticArgumentKind, SemanticTargetRequirement};
        for (name, kind, expected) in [
            ("filter", SemanticArgumentKind::String, "string"),
            ("limit", SemanticArgumentKind::Integer, "integer"),
            ("enabled", SemanticArgumentKind::Boolean, "boolean"),
        ] {
            let operation = SemanticOperationDescription {
                id: name.into(),
                description: "Mock read".into(),
                arguments: vec![SemanticArgument {
                    name: name.into(),
                    kind,
                    required: true,
                    description: "Input".into(),
                }],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            };
            let schema = invocation_schema(&operation);
            assert_eq!(
                schema["properties"]["arguments"]["properties"][name]["type"],
                expected
            );
            assert_eq!(schema["properties"]["arguments"]["required"], json!([name]));
            assert!(schema["properties"].get(name).is_none());
        }
    }
    use crate::proposal::{ProposalError, ProposalId};
    use crate::source_edit::SourceId;
    use crate::text::{Position, TextRange};
    use scope_language::{
        CandidateValidation, LanguageAnalysis, SemanticEdit, SemanticTargetRequirement,
        SemanticValidationRequest, ValidationStatus,
    };
    use std::sync::Mutex;

    const ORIGINAL: &str = "α old\nkeep café\nold ω\n";
    const CANDIDATE: &str = "α 新\nkeep café\nlonger ω\n";

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
            "fixture"
        }
        fn apply_source_edit(&mut self, _: TextEdit) -> Result<(), ProposalError> {
            panic!("semantic proposals must never apply source edits")
        }
    }

    fn edit(line: usize, start: usize, end: usize, replacement: &str) -> SemanticEdit {
        SemanticEdit {
            range: TextRange::new(Position::new(line, start), Position::new(line, end)),
            replacement: replacement.into(),
        }
    }
    fn edits() -> Vec<SemanticEdit> {
        // Deliberately unsorted; both ranges refer to the original UTF-8 snapshot.
        vec![edit(2, 0, 3, "longer"), edit(0, 3, 6, "新")]
    }
    fn changes(edits: Vec<SemanticEdit>) -> SemanticOperationResult {
        SemanticOperationResult::Changes {
            edits,
            rationale: "rename both occurrences".into(),
        }
    }

    struct LegacyService(SemanticOperationResult);
    impl LanguageService for LegacyService {
        fn language_id(&self) -> &'static str {
            "fixture"
        }
        fn language_name(&self) -> &'static str {
            "Fixture"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            false
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            panic!("semantic proposals must not analyze or evaluate source")
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            vec![SemanticOperationDescription {
                id: "rename".into(),
                description: "fixture operation".into(),
                arguments: vec![],
                effect: if matches!(
                    self.0,
                    SemanticOperationResult::ReadOnly { .. }
                        | SemanticOperationResult::ReadOnlyEvaluation { .. }
                        | SemanticOperationResult::Evaluated { .. }
                ) {
                    SemanticOperationEffect::Read
                } else {
                    SemanticOperationEffect::ProposeChanges
                },
                target: SemanticTargetRequirement::Document,
                constraints: None,
            }]
        }
        fn semantic_operation_produces_evaluation(&self, operation_id: &str) -> bool {
            operation_id == "rename" && matches!(self.0, SemanticOperationResult::Evaluated { .. })
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            assert_eq!(request.operation_id, "rename");
            assert_eq!(request.document.text(), ORIGINAL);
            assert_eq!(request.document.version(), 7);
            Ok(self.0.clone())
        }
    }

    #[test]
    fn catalog_exposes_purpose_independently_of_effect_and_evaluation_support() {
        use scope_language::SemanticOperationPurpose;
        for result in [
            SemanticOperationResult::ReadOnly {
                value: json!({}),
                rationale: "inspect".into(),
            },
            changes(edits()),
        ] {
            for purpose in [
                SemanticOperationPurpose::Exploration,
                SemanticOperationPurpose::CandidateEvaluation,
            ] {
                let service = LegacyService(result.clone());
                let target = Target;
                let mut controller = SourceEditController::default();
                let mut provider = SemanticToolProvider::new(
                    &service,
                    &target,
                    &mut controller,
                    PermittedEditScope::Document(target.source_id()),
                    None,
                    |_| true,
                )
                .unwrap();
                let original_effect = provider.tools[0].effect;
                // Leave the legacy description untouched for the default case.
                if purpose == SemanticOperationPurpose::CandidateEvaluation {
                    provider.operations[0] = provider.operations[0]
                        .clone()
                        .with_purpose(purpose)
                        .unwrap();
                }
                let session = AgentSession::builder().provider(provider).build().unwrap();
                let catalog = session.catalog();
                let entry = catalog.lookup("language.fixture.rename").unwrap();
                assert_eq!(entry.definition.effect, original_effect);
                let metadata = to_json(entry.metadata.clone().unwrap());
                assert_eq!(metadata["purpose"], serde_json::to_value(purpose).unwrap());
                assert_eq!(metadata["produces_evaluation"], false);
                let exported = to_json(catalog.to_value());
                assert_eq!(
                    exported["tools"][0]["metadata"]["purpose"],
                    metadata["purpose"]
                );
            }
        }
    }

    struct ValidatingService {
        legacy: LegacyService,
        validation: CandidateValidation,
        candidates: Mutex<Vec<String>>,
    }
    impl ValidatingService {
        fn new(result: SemanticOperationResult, status: ValidationStatus) -> Self {
            Self {
                legacy: LegacyService(result),
                validation: CandidateValidation {
                    status,
                    ..Default::default()
                },
                candidates: Mutex::new(vec![]),
            }
        }
    }
    impl LanguageService for ValidatingService {
        fn language_id(&self) -> &'static str {
            self.legacy.language_id()
        }
        fn language_name(&self) -> &'static str {
            self.legacy.language_name()
        }
        fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
            self.legacy.matches_path(path)
        }
        fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
            self.legacy.analyze(document)
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            self.legacy.semantic_operations()
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            self.legacy.perform_semantic_operation(request)
        }
        fn validate_semantic_candidate(
            &self,
            request: SemanticValidationRequest<'_>,
        ) -> Result<CandidateValidation, SemanticOperationError> {
            assert_eq!(request.operation_id, "rename");
            assert_eq!(request.document.text(), ORIGINAL);
            assert_eq!(request.document.version(), 7);
            assert_eq!(request.edits, edits());
            assert_eq!(request.selection, None);
            assert_eq!(request.arguments, &serde_json::json!({}));
            assert_eq!(request.document_id, "Scratch(42)");
            self.candidates
                .lock()
                .unwrap()
                .push(request.candidate_source.into());
            Ok(self.validation.clone())
        }
    }

    fn invoke(
        service: &dyn LanguageService,
        controller: &mut SourceEditController,
        policy: impl for<'a> FnMut(SemanticAuthorization<'a>) -> bool,
    ) -> Value {
        let target = Target;
        let mut provider = SemanticToolProvider::new(
            service,
            &target,
            controller,
            PermittedEditScope::Document(target.source_id()),
            None,
            policy,
        )
        .unwrap();
        let result = provider
            .invoke(
                ToolCall::new("language.fixture.rename")
                    .with_argument("expected_version", "7".into())
                    .with_argument("expected_source", ORIGINAL.into())
                    .with_argument("arguments", ToolValue::Object(Default::default())),
            )
            .unwrap();
        assert_eq!(target.text(), ORIGINAL);
        assert_eq!(target.version(), 7);
        to_json(result.value)
    }

    #[test]
    fn validation_observes_full_utf8_preview_and_evidence_is_registered() {
        for status in [
            ValidationStatus::Accepted,
            ValidationStatus::AcceptedWithLimitations,
        ] {
            let mut service = ValidatingService::new(changes(edits()), status);
            service.validation.limitations = vec!["static validation only".into()];
            let mut controller = SourceEditController::default();
            let result = invoke(&service, &mut controller, |_| true);
            assert_eq!(result["status"], "proposal");
            assert_eq!(result["validation_status"], format!("{status:?}"));
            assert_eq!(
                result["limitations"],
                serde_json::json!(["static validation only"])
            );
            assert_eq!(*service.candidates.lock().unwrap(), vec![CANDIDATE]);
            assert_eq!(controller.list().count(), 1);
            let id = ProposalId(result["proposal_id"].as_str().unwrap().parse().unwrap());
            let proposal = controller.get(id).unwrap();
            assert_eq!(proposal.preview(&Target).unwrap(), CANDIDATE);
            assert_eq!(proposal.validation(), &service.validation);
        }
    }

    #[test]
    fn malformed_edits_are_rejected_before_validation_or_registration() {
        for malformed in [
            vec![edit(0, 3, 6, "x"), edit(0, 4, 6, "y")],
            vec![edit(0, 1, 2, "x")], // Byte 1 splits α.
        ] {
            let service = ValidatingService::new(changes(malformed), ValidationStatus::Accepted);
            let mut controller = SourceEditController::default();
            let result = invoke(&service, &mut controller, |_| true);
            assert_eq!(result["status"], "invalid_arguments");
            assert!(result.get("proposal_id").is_none());
            assert!(service.candidates.lock().unwrap().is_empty());
            assert_eq!(controller.list().count(), 0);
        }
    }

    #[test]
    fn denied_final_registration_is_unauthorized_not_semantically_rejected() {
        let service = ValidatingService::new(changes(edits()), ValidationStatus::Accepted);
        let mut controller = SourceEditController::default();
        let mut policy_calls = 0;
        let result = invoke(&service, &mut controller, |_| {
            policy_calls += 1;
            policy_calls == 1
        });
        assert_eq!(policy_calls, 2);
        assert_eq!(result["status"], "unauthorized");
        assert!(result.get("validation_status").is_none());
        assert!(result.get("correction_eligible").is_none());
        assert!(result.get("proposal_id").is_none());
        assert_eq!(*service.candidates.lock().unwrap(), vec![CANDIDATE]);
        assert_eq!(controller.list().count(), 0);
    }

    #[test]
    fn semantic_rejection_never_registers_a_proposal() {
        let service = ValidatingService::new(changes(edits()), ValidationStatus::Rejected);
        let mut controller = SourceEditController::default();
        let result = invoke(&service, &mut controller, |_| true);
        assert_eq!(result["status"], "rejected");
        assert_eq!(result["validation_status"], "Rejected");
        assert_eq!(result["correction_eligible"], true);
        assert!(result.get("proposal_id").is_none());
        assert_eq!(*service.candidates.lock().unwrap(), vec![CANDIDATE]);
        assert_eq!(controller.list().count(), 0);
    }

    #[test]
    fn legacy_default_validation_remains_not_evaluated_in_registered_proposal() {
        let service = LegacyService(changes(edits()));
        let mut controller = SourceEditController::default();
        let result = invoke(&service, &mut controller, |_| true);
        assert_eq!(result["status"], "proposal");
        assert_eq!(result["validation_status"], "NotEvaluated");
        assert_eq!(controller.list().count(), 1);
        let id = ProposalId(result["proposal_id"].as_str().unwrap().parse().unwrap());
        let proposal = controller.get(id).unwrap();
        assert_eq!(proposal.validation(), &CandidateValidation::default());
        assert_eq!(proposal.preview(&Target).unwrap(), CANDIDATE);
    }

    fn required_contract(
        origin: scope_language::ExpectationOrigin,
    ) -> scope_language::ValidationContract {
        scope_language::ValidationContract {
            id: "preview-contract".into(),
            origin,
            checks: std::collections::BTreeMap::from([(
                "exact-preview".into(),
                serde_json::json!(CANDIDATE),
            )]),
        }
    }

    struct ContractService {
        legacy: LegacyService,
        requests: Mutex<Vec<(String, scope_language::ValidationContract)>>,
        not_evaluated: bool,
    }

    impl ContractService {
        fn new(result: SemanticOperationResult) -> Self {
            Self {
                legacy: LegacyService(result),
                requests: Mutex::new(vec![]),
                not_evaluated: false,
            }
        }
    }

    impl LanguageService for ContractService {
        fn language_id(&self) -> &'static str {
            self.legacy.language_id()
        }
        fn language_name(&self) -> &'static str {
            self.legacy.language_name()
        }
        fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
            self.legacy.matches_path(path)
        }
        fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
            self.legacy.analyze(document)
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            self.legacy.semantic_operations()
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            self.legacy.perform_semantic_operation(request)
        }
        fn validate_semantic_contract(
            &self,
            request: SemanticValidationRequest<'_>,
            contract: &scope_language::ValidationContract,
        ) -> Result<scope_language::ContractValidation, SemanticOperationError> {
            use scope_language::{CheckObservation, CheckOutcome, ContractValidation};
            assert_eq!(request.operation_id, "rename");
            assert_eq!(request.document.text(), ORIGINAL);
            assert_eq!(request.document.version(), 7);
            assert_eq!(request.selection, None);
            assert_eq!(request.arguments, &serde_json::json!({}));
            assert_eq!(request.document_id, "Scratch(42)");
            assert_eq!(
                request.edits,
                match &self.legacy.0 {
                    SemanticOperationResult::Changes { edits, .. }
                    | SemanticOperationResult::ValidatedChanges { edits, .. } => edits,
                    _ => panic!("expected edits"),
                }
            );
            self.requests
                .lock()
                .unwrap()
                .push((request.candidate_source.into(), contract.clone()));
            let actual = serde_json::json!(request.candidate_source);
            Ok(ContractValidation {
                checks: contract
                    .checks
                    .iter()
                    .map(|(id, expected)| {
                        let outcome = if self.not_evaluated {
                            CheckOutcome::NotEvaluated
                        } else if expected == &actual {
                            CheckOutcome::Passed
                        } else {
                            CheckOutcome::Failed
                        };
                        (
                            id.clone(),
                            CheckObservation {
                                outcome,
                                actual: actual.clone(),
                            },
                        )
                    })
                    .collect(),
            })
        }
    }

    fn assert_contract_feedback(result: &Value, outcome: &str, actual: &str) {
        assert_eq!(result["contract_id"], "preview-contract");
        assert_eq!(
            result["checks"],
            serde_json::json!([{
                "id": "exact-preview",
                "observed": true,
                "outcome": outcome,
                "expected_json": serde_json::json!(CANDIDATE).to_string(),
                "actual_json": serde_json::json!(actual).to_string(),
            }])
        );
    }

    fn assert_contract_blocked(result: &Value, reason: &str, controller: &SourceEditController) {
        assert_eq!(result["status"], "validation_failed");
        assert_eq!(result["reason"], reason);
        assert_eq!(result["correction_eligible"], true);
        assert!(result.get("proposal_id").is_none());
        assert_eq!(controller.list().count(), 0);
    }

    #[test]
    fn required_contract_checks_final_preview_and_allows_corrected_candidate() {
        let contract = required_contract(scope_language::ExpectationOrigin::User(
            "fixture user".into(),
        ));
        let mut controller = SourceEditController::with_validation_contract(contract.clone());
        let mut service = ContractService::new(changes(vec![edit(0, 3, 6, "新")]));
        let incorrect = "α 新\nkeep café\nold ω\n";
        let result = invoke(&service, &mut controller, |_| true);
        assert_contract_blocked(&result, "FailedEvidence", &controller);
        assert_contract_feedback(&result, "Failed", incorrect);

        service.legacy.0 = changes(edits());
        let result = invoke(&service, &mut controller, |_| true);
        assert_eq!(result["status"], "proposal");
        assert_contract_feedback(&result, "Passed", CANDIDATE);
        assert_eq!(
            *service.requests.lock().unwrap(),
            vec![
                (incorrect.into(), contract.clone()),
                (CANDIDATE.into(), contract),
            ]
        );
        assert_eq!(controller.list().count(), 1);
        let id = ProposalId(result["proposal_id"].as_str().unwrap().parse().unwrap());
        assert_eq!(
            controller.get(id).unwrap().preview(&Target).unwrap(),
            CANDIDATE
        );
    }

    #[test]
    fn required_contract_blocks_default_absent_observations() {
        let service = LegacyService(changes(edits()));
        let mut controller = SourceEditController::with_validation_contract(required_contract(
            scope_language::ExpectationOrigin::User("fixture user".into()),
        ));
        let result = invoke(&service, &mut controller, |_| true);
        assert_contract_blocked(&result, "MissingEvidence", &controller);
        assert_eq!(result["contract_id"], "preview-contract");
        assert_eq!(
            result["checks"],
            serde_json::json!([{
                "id": "exact-preview",
                "observed": false,
                "outcome": "NotEvaluated",
                "expected_json": serde_json::json!(CANDIDATE).to_string(),
                "actual_json": "null",
            }])
        );
    }

    #[test]
    fn required_contract_blocks_not_evaluated_observations() {
        let mut service = ContractService::new(changes(edits()));
        service.not_evaluated = true;
        let mut controller = SourceEditController::with_validation_contract(required_contract(
            scope_language::ExpectationOrigin::User("fixture user".into()),
        ));
        let result = invoke(&service, &mut controller, |_| true);
        assert_contract_blocked(&result, "MissingEvidence", &controller);
        assert_contract_feedback(&result, "NotEvaluated", CANDIDATE);
        assert_eq!(service.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn required_contract_blocks_agent_origin_even_when_checks_pass() {
        let service = ContractService::new(changes(edits()));
        let contract = required_contract(scope_language::ExpectationOrigin::Agent(
            "fixture agent".into(),
        ));
        let mut controller = SourceEditController::with_validation_contract(contract.clone());
        let result = invoke(&service, &mut controller, |_| true);
        assert_contract_blocked(&result, "UntrustedExpectations", &controller);
        assert_contract_feedback(&result, "Passed", CANDIDATE);
        assert_eq!(
            *service.requests.lock().unwrap(),
            vec![(CANDIDATE.into(), contract)]
        );
    }

    #[test]
    fn static_prevalidated_changes_cannot_bypass_required_contract() {
        for status in [
            ValidationStatus::Accepted,
            ValidationStatus::AcceptedWithLimitations,
        ] {
            let service = ContractService::new(SemanticOperationResult::ValidatedChanges {
                edits: vec![edit(0, 3, 6, "新")],
                rationale: "static acceptance is not contract evidence".into(),
                validation: CandidateValidation {
                    status,
                    ..Default::default()
                },
            });
            let contract = required_contract(scope_language::ExpectationOrigin::User(
                "fixture user".into(),
            ));
            let mut controller = SourceEditController::with_validation_contract(contract.clone());
            let result = invoke(&service, &mut controller, |_| true);
            let incorrect = "α 新\nkeep café\nold ω\n";
            assert_contract_blocked(&result, "FailedEvidence", &controller);
            assert_contract_feedback(&result, "Failed", incorrect);
            assert_eq!(result["validation_status"], format!("{status:?}"));
            assert_eq!(
                *service.requests.lock().unwrap(),
                vec![(incorrect.into(), contract)]
            );
        }
    }

    struct ArgumentContractService {
        requests: Mutex<Vec<(Value, String)>>,
    }

    impl LanguageService for ArgumentContractService {
        fn language_id(&self) -> &'static str {
            "fixture"
        }
        fn language_name(&self) -> &'static str {
            "Fixture"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            false
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            panic!("contract validation must not analyze or evaluate source")
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            let mut operations = LegacyService(changes(edits())).semantic_operations();
            operations[0].arguments = vec![scope_language::SemanticArgument {
                name: "replacement".into(),
                kind: scope_language::SemanticArgumentKind::String,
                required: true,
                description: "Replacement for the last occurrence".into(),
            }];
            operations
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            assert_eq!(request.operation_id, "rename");
            assert_eq!(request.document.text(), ORIGINAL);
            assert_eq!(request.document.version(), 7);
            Ok(changes(vec![
                edit(2, 0, 3, request.arguments["replacement"].as_str().unwrap()),
                edit(0, 3, 6, "新"),
            ]))
        }
        fn validate_semantic_contract(
            &self,
            request: SemanticValidationRequest<'_>,
            contract: &scope_language::ValidationContract,
        ) -> Result<scope_language::ContractValidation, SemanticOperationError> {
            use scope_language::{CheckObservation, CheckOutcome, ContractValidation};
            assert_eq!(request.operation_id, "rename");
            assert_eq!(request.document.text(), ORIGINAL);
            assert_eq!(request.document.version(), 7);
            assert_eq!(request.document_id, "Scratch(42)");
            assert_eq!(request.selection, None);
            let replacement = request.arguments["replacement"].as_str().unwrap();
            assert_eq!(
                request.edits,
                vec![edit(2, 0, 3, replacement), edit(0, 3, 6, "新")]
            );
            assert_eq!(
                request.candidate_source,
                format!("α 新\nkeep café\n{replacement} ω\n")
            );
            self.requests
                .lock()
                .unwrap()
                .push((request.arguments.clone(), request.candidate_source.into()));
            let actual = serde_json::json!(request.candidate_source);
            Ok(ContractValidation {
                checks: contract
                    .checks
                    .iter()
                    .map(|(id, expected)| {
                        (
                            id.clone(),
                            CheckObservation {
                                outcome: if expected == &actual {
                                    CheckOutcome::Passed
                                } else {
                                    CheckOutcome::Failed
                                },
                                actual: actual.clone(),
                            },
                        )
                    })
                    .collect(),
            })
        }
    }

    #[test]
    fn different_arguments_validate_fresh_preview_and_bind_exact_evidence() {
        let service = ArgumentContractService {
            requests: Mutex::new(vec![]),
        };
        let contract = required_contract(scope_language::ExpectationOrigin::User(
            "fixture user".into(),
        ));
        let mut controller = SourceEditController::with_validation_contract(contract.clone());
        let invoke_arguments = |controller: &mut SourceEditController, replacement: &str| {
            let target = Target;
            let mut provider = SemanticToolProvider::new(
                &service,
                &target,
                controller,
                PermittedEditScope::Document(target.source_id()),
                None,
                |_| true,
            )
            .unwrap();
            let result = provider
                .invoke(
                    ToolCall::new("language.fixture.rename")
                        .with_argument("expected_version", "7".into())
                        .with_argument("expected_source", ORIGINAL.into())
                        .with_argument(
                            "arguments",
                            ToolValue::object([("replacement", replacement.into())]),
                        ),
                )
                .unwrap();
            assert_eq!(target.text(), ORIGINAL);
            assert_eq!(target.version(), 7);
            to_json(result.value)
        };
        let accepted = invoke_arguments(&mut controller, "longer");
        assert_eq!(accepted["status"], "proposal");
        assert_contract_feedback(&accepted, "Passed", CANDIDATE);
        let id = ProposalId(accepted["proposal_id"].as_str().unwrap().parse().unwrap());
        let proposal = controller.get(id).unwrap();
        let evidence = proposal.evidence().unwrap().clone();
        assert_eq!(evidence.operation, "rename");
        assert_eq!(
            evidence.arguments,
            serde_json::json!({"replacement": "longer"})
        );
        assert_eq!(evidence.target, SourceId::Scratch(42));
        assert_eq!(evidence.version, 7);
        assert_eq!(evidence.original, ORIGINAL);
        assert_eq!(evidence.candidate, proposal.preview(&Target).unwrap());
        assert_eq!(evidence.candidate, CANDIDATE);
        assert_eq!(evidence.contract, contract);
        assert_eq!(
            evidence.observations.checks["exact-preview"].outcome,
            scope_language::CheckOutcome::Passed
        );
        assert_eq!(
            evidence.observations.checks["exact-preview"].actual,
            serde_json::json!(CANDIDATE)
        );

        let rejected = invoke_arguments(&mut controller, "different");
        let different = "α 新\nkeep café\ndifferent ω\n";
        assert_eq!(rejected["status"], "validation_failed");
        assert_eq!(rejected["reason"], "FailedEvidence");
        assert_eq!(rejected["correction_eligible"], true);
        assert!(rejected.get("proposal_id").is_none());
        assert_contract_feedback(&rejected, "Failed", different);
        assert_eq!(
            *service.requests.lock().unwrap(),
            vec![
                (
                    serde_json::json!({"replacement": "longer"}),
                    CANDIDATE.into()
                ),
                (
                    serde_json::json!({"replacement": "different"}),
                    different.into()
                ),
            ]
        );
        assert_eq!(controller.list().count(), 1);
        assert_eq!(controller.get(id).unwrap().evidence(), Some(&evidence));
    }

    #[test]
    fn authorization_denial_with_required_contract_never_registers() {
        // Deny each authorization boundary, including the one after passing contract checks.
        for denied_call in 1..=3 {
            let service = ContractService::new(changes(edits()));
            let mut controller = SourceEditController::with_validation_contract(required_contract(
                scope_language::ExpectationOrigin::User("fixture user".into()),
            ));
            let mut calls = 0;
            let result = invoke(&service, &mut controller, |_| {
                calls += 1;
                calls != denied_call
            });
            assert_eq!(calls, denied_call);
            assert_eq!(result["status"], "unauthorized");
            assert!(result.get("proposal_id").is_none());
            assert!(result.get("correction_eligible").is_none());
            assert_eq!(controller.list().count(), 0);
            assert_eq!(
                service.requests.lock().unwrap().len(),
                usize::from(denied_call == 3)
            );
        }
    }

    #[test]
    fn generic_outcomes_preserve_provider_evidence_and_contract_binding() {
        use scope_language::{SemanticEvaluation, SemanticOutcome};
        for outcome in [
            SemanticOutcome::Satisfied,
            SemanticOutcome::Violated,
            SemanticOutcome::Inconclusive,
        ] {
            let evaluation = SemanticEvaluation {
                outcome,
                summary: "Compare the candidate with the expected answer".into(),
                diagnostics: Some(json!({"expected": 4, "actual": 3})),
                failed_criteria: if outcome == SemanticOutcome::Violated {
                    vec!["answer".into()]
                } else {
                    vec![]
                },
                contract_id: Some("preview-contract".into()),
            };
            let service = LegacyService(SemanticOperationResult::Evaluated {
                value: json!({"passed": false, "native": [3, 4]}),
                rationale: "provider result".into(),
                evaluation: evaluation.clone(),
            });
            let mut controller = SourceEditController::with_validation_contract(required_contract(
                scope_language::ExpectationOrigin::User("fixture user".into()),
            ));
            let result = invoke(&service, &mut controller, |_| true);
            assert_eq!(result["status"], "read_only");
            assert_eq!(
                result["evaluation"],
                serde_json::to_value(&evaluation).unwrap()
            );
            assert_eq!(result["value"], json!({"passed": false, "native": [3, 4]}));
            assert_eq!(controller.list().count(), 0);

            {
                let target = Target;
                let provider = SemanticToolProvider::new(
                    &service,
                    &target,
                    &mut controller,
                    PermittedEditScope::Document(target.source_id()),
                    None,
                    |_| true,
                )
                .unwrap();
                let metadata = to_json(provider.metadata("language.fixture.rename").unwrap());
                assert_eq!(metadata["produces_evaluation"], true);
            }
            let mut wrong_contract = SourceEditController::default();
            let rejected = invoke(&service, &mut wrong_contract, |_| true);
            assert_eq!(rejected["status"], "internal_failure");
            let denied = invoke(&service, &mut controller, |_| false);
            assert_eq!(denied["status"], "unauthorized");
        }
    }

    #[test]
    fn failed_read_only_evaluation_preserves_observations_without_a_proposal() {
        use scope_language::{CheckObservation, CheckOutcome, ContractValidation};

        let value = json!({"answer": 3});
        let service = ValidatingService::new(
            SemanticOperationResult::ReadOnlyEvaluation {
                value: value.clone(),
                rationale: "evaluated without editing".into(),
                evaluation: ContractValidation {
                    checks: std::collections::BTreeMap::from([(
                        "answer-check".into(),
                        CheckObservation {
                            outcome: CheckOutcome::Failed,
                            actual: json!({"answer": 3, "details": [false, null]}),
                        },
                    )]),
                },
            },
            ValidationStatus::Rejected,
        );
        let mut controller = SourceEditController::default();
        // invoke also asserts that source text and version remain unchanged.
        let result = invoke(&service, &mut controller, |_| true);
        assert_eq!(
            result,
            json!({
                "status": "read_only",
                "operation_id": "rename",
                "value": value,
                "rationale": "evaluated without editing",
                "evaluation": {"checks": [{
                    "id": "answer-check",
                    "outcome": "Failed",
                    "actual": {"answer": 3, "details": [false, null]},
                }]},
            })
        );
        assert!(service.candidates.lock().unwrap().is_empty());
        assert_eq!(controller.list().count(), 0);
    }

    #[test]
    fn read_only_result_is_unchanged_and_skips_candidate_validation() {
        let value = serde_json::json!({"occurrences": ["α", "ω"], "count": 2});
        let service = ValidatingService::new(
            SemanticOperationResult::ReadOnly {
                value: value.clone(),
                rationale: "inspection".into(),
            },
            ValidationStatus::Rejected,
        );
        let mut controller = SourceEditController::default();
        let result = invoke(&service, &mut controller, |_| true);
        assert_eq!(
            result,
            serde_json::json!({
                "status": "read_only", "operation_id": "rename", "value": value,
                "rationale": "inspection",
            })
        );
        assert!(service.candidates.lock().unwrap().is_empty());
        assert_eq!(controller.list().count(), 0);
    }
}
