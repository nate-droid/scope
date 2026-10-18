//! Example-only language: one `let NAME = INTEGER` declaration per line.
use scope_language::{
    LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService,
    SemanticOperationDescription, SemanticOperationEffect, SemanticOperationError,
    SemanticOperationRequest, SemanticOperationResult, SemanticTargetRequirement,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const SOURCE: &str = "let answer = 42\nlet count = 7\n";
#[derive(Default)]
pub struct DeclarationLanguage {
    pub calls: AtomicUsize,
}
impl LanguageService for DeclarationLanguage {
    fn language_id(&self) -> &'static str {
        "example.declarations"
    }
    fn language_name(&self) -> &'static str {
        "Example declarations"
    }
    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("decl")
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        vec![SemanticOperationDescription {
            id: "describe_selection".into(),
            description: "Read-only partial description of the selected declaration; no evaluation or edits. Select exactly one complete declaration line.".into(),
            arguments: vec![],
            effect: SemanticOperationEffect::Read,
            target: SemanticTargetRequirement::Selection,
            constraints: Some(json!({"selection": "one complete declaration line, excluding newline", "unsupported": ["references", "inferred_type"]})),
        }]
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.operation_id, "describe_selection");
        let selection = request
            .selection
            .ok_or_else(|| SemanticOperationError::InvalidArguments("selection required".into()))?;
        let line = request
            .document
            .text()
            .lines()
            .nth(selection.start.line)
            .ok_or_else(|| SemanticOperationError::InvalidArguments("line unavailable".into()))?;
        if selection.start.line != selection.end.line
            || selection.start.column != 0
            || selection.end.column != line.len()
            || line.is_empty()
        {
            return Err(SemanticOperationError::InvalidArguments(
                "select one complete declaration line".into(),
            ));
        }
        let words: Vec<_> = line.split_whitespace().collect();
        let valid_name = |name: &str| {
            !name.is_empty()
                && name.bytes().enumerate().all(|(i, c)| {
                    c.is_ascii_alphabetic() || c == b'_' || (i > 0 && c.is_ascii_digit())
                })
        };
        let ["let", name, "=", literal] = words.as_slice() else {
            return Err(SemanticOperationError::UnsupportedOperation(
                "selected construct is not a supported declaration".into(),
            ));
        };
        if !valid_name(name) || literal.parse::<i64>().is_err() {
            return Err(SemanticOperationError::UnsupportedOperation(
                "only named integer declarations are supported".into(),
            ));
        }
        Ok(SemanticOperationResult::ReadOnly {
            value: json!({
                "partial": true, "kind": "integer_literal_binding", "name": name,
                "span": {"line": selection.start.line, "start_column": 0, "end_column": line.len()},
                "literal": literal,
                "references": {"status": "unsupported"},
                "inferred_type": {"status": "unknown", "reason": "fixture has no type analysis"}
            }),
            rationale: "Parsed only the selected declaration from the authorized snapshot; no reference or type analysis performed.".into(),
        })
    }
}
