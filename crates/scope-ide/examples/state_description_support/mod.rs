//! Example-only state machine: one `FROM -> TO on EVENT` transition per line.
//! Output is provider-owned, not a universal selection-description schema.
use scope_language::{
    LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService,
    SemanticOperationDescription, SemanticOperationEffect, SemanticOperationError,
    SemanticOperationRequest, SemanticOperationResult, SemanticTargetRequirement,
};
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const SOURCE: &str = "idle -> running on start\nrunning -> idle on stop\n";

#[derive(Default)]
pub struct StateMachineLanguage {
    pub calls: AtomicUsize,
}

fn parse_transition(line: &str) -> Result<(&str, &str, &str), SemanticOperationError> {
    let words: Vec<_> = line.split_whitespace().collect();
    let valid_name = |name: &str| {
        !name.is_empty()
            && name
                .bytes()
                .enumerate()
                .all(|(i, c)| c.is_ascii_alphabetic() || c == b'_' || (i > 0 && c.is_ascii_digit()))
    };
    if let [from, "->", to, "on", event] = words.as_slice() {
        if valid_name(from) && valid_name(to) && valid_name(event) {
            return Ok((from, to, event));
        }
    }
    Err(SemanticOperationError::UnsupportedOperation(
        "only `FROM -> TO on EVENT` transitions with ASCII identifiers are supported; guards, actions, and other constructs are unsupported".into(),
    ))
}

impl LanguageService for StateMachineLanguage {
    fn language_id(&self) -> &'static str {
        "example.state_machine"
    }

    fn language_name(&self) -> &'static str {
        "Example state machine"
    }

    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("state")
    }

    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }

    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        vec![SemanticOperationDescription {
            id: "describe_selection".into(),
            description: "Read-only explicit state topology for one complete selected transition line; no execution, runtime reachability, type analysis, or edits.".into(),
            arguments: vec![],
            effect: SemanticOperationEffect::Read,
            target: SemanticTargetRequirement::Selection,
            constraints: Some(json!({
                "selection": "one complete transition line, excluding newline; UTF-8 byte columns",
                "syntax": "FROM -> TO on EVENT; ASCII identifiers; blank document lines ignored",
                "topology": "supported: explicit endpoint states and outgoing transitions from the selected source state",
                "reachability": {"status": "unknown", "reason": "runtime guards unavailable"},
                "inferred_type": {"status": "unsupported"},
                "unsupported": ["guards", "actions", "implicit states", "type analysis"]
            })),
        }]
    }

    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if request.operation_id != "describe_selection" {
            return Err(SemanticOperationError::UnsupportedOperation(
                request.operation_id.into(),
            ));
        }
        let selection = request
            .selection
            .ok_or_else(|| SemanticOperationError::InvalidArguments("selection required".into()))?;
        let text = request.document.text();
        let line = text
            .lines()
            .nth(selection.start.line)
            .ok_or_else(|| SemanticOperationError::InvalidArguments("line unavailable".into()))?;
        if selection.start.line != selection.end.line
            || selection.start.column != 0
            || selection.end.column != line.len()
            || line.trim().is_empty()
        {
            return Err(SemanticOperationError::InvalidArguments(
                "select one complete transition line, excluding newline".into(),
            ));
        }
        let (from, to, event) = parse_transition(line)?;
        let mut states = BTreeSet::new();
        let mut outgoing = Vec::new();
        for (index, document_line) in text.lines().enumerate() {
            if document_line.trim().is_empty() {
                continue;
            }
            let (source, destination, trigger) = parse_transition(document_line).map_err(|_| {
                SemanticOperationError::UnsupportedOperation(format!(
                    "unsupported construct on document line {}; expected `FROM -> TO on EVENT` with ASCII identifiers and no guards or actions",
                    index + 1
                ))
            })?;
            states.insert(source);
            states.insert(destination);
            if source == from {
                outgoing.push(json!({"from": source, "to": destination, "event": trigger}));
            }
        }
        Ok(SemanticOperationResult::ReadOnly {
            value: json!({
                "transition": {"from": from, "to": to, "event": event},
                "topology": {
                    "states": states.into_iter().collect::<Vec<_>>(),
                    "outgoing": outgoing
                },
                "capabilities": {
                    "topology": {"status": "supported"},
                    "reachability": {"status": "unknown", "reason": "runtime guards unavailable"}
                },
                "inferred_type": {"status": "unsupported"}
            }),
            rationale: "Parsed explicit transitions from the authorized document snapshot only. States are sorted; outgoing transitions from the selected source retain document order. Runtime guards are unavailable, so topology does not establish runtime reachability. No execution, type analysis, or edits performed.".into(),
        })
    }
}
