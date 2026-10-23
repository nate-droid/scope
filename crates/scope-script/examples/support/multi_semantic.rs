//! Small downstream-only service shared by the SI7 tests and demo.
use scope_language::*;
use serde_json::json;
use std::sync::Mutex;

pub const LANGUAGE: &str = "example.multi";
pub const COMBINED: &str = r#"
let operations = scope.operations();
print(operations);
let first = scope.invoke("semantic", "language.example.multi.echo", {label: "demo"});
let second = scope.invoke("semantic", "language.example.multi.measure", {enabled: true, limit: 3});
{echo: first.value, measure: second.value}
"#;

#[derive(Default)]
pub struct MultiLanguage {
    pub calls: Mutex<Vec<String>>,
}
impl LanguageService for MultiLanguage {
    fn language_id(&self) -> &'static str {
        LANGUAGE
    }
    fn language_name(&self) -> &'static str {
        "Multi semantic fixture"
    }
    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("multi")
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        ["echo", "measure", "hidden", "denied", "propose"]
            .into_iter()
            .map(|id| {
                let arguments = match id {
                    "echo" => vec![("label", SemanticArgumentKind::String)],
                    "measure" => vec![
                        ("enabled", SemanticArgumentKind::Boolean),
                        ("limit", SemanticArgumentKind::Integer),
                    ],
                    _ => vec![],
                };
                SemanticOperationDescription {
                    id: id.into(),
                    description: format!("Fixture {id}"),
                    arguments: arguments
                        .into_iter()
                        .map(|(name, kind)| SemanticArgument {
                            name: name.into(),
                            kind,
                            required: true,
                            description: format!("Typed {name}"),
                        })
                        .collect(),
                    effect: if id == "propose" {
                        SemanticOperationEffect::ProposeChanges
                    } else {
                        SemanticOperationEffect::Read
                    },
                    target: SemanticTargetRequirement::Document,
                    constraints: Some(json!({"fixture": "SI7", "operation": id})),
                }
            })
            .collect()
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        self.calls.lock().unwrap().push(request.operation_id.into());
        assert!(
            matches!(request.operation_id, "echo" | "measure"),
            "forbidden dispatch"
        );
        Ok(SemanticOperationResult::ReadOnly {
            value: json!({"operation": request.operation_id, "source": request.document.text(), "arguments": request.arguments}),
            rationale: "Read only the invocation-local snapshot".into(),
        })
    }
}

pub fn bindings() -> Vec<(String, Option<bool>)> {
    vec![("echo".into(), Some(true)), ("measure".into(), Some(true))]
}
