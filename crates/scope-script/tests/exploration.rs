#[path = "../examples/exploration.rs"]
mod exploration;

use scope_script::Value;
use serde_json::{Value as Json, json};

const SOURCE: &str = include_str!("../examples/semantic-workspace/example.decl");
const ACTION: &str = "language.example.declarations.describe_selection";
const RATIONALE: &str = "Parsed only the selected declaration from the authorized snapshot; no reference or type analysis performed.";

fn json_value(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Boolean(value) => (*value).into(),
        Value::Integer(value) => (*value).into(),
        Value::String(value) => value.clone().into(),
        Value::Array(values) => Json::Array(values.iter().map(json_value).collect()),
        Value::Object(fields) => Json::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), json_value(value)))
                .collect(),
        ),
    }
}

fn run(script: &str) -> Result<(Value, Vec<Value>), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("example.decl"), SOURCE).unwrap();
    let result = exploration::run(dir.path().to_str().unwrap(), script);
    // Check the actual bytes and directory contents on both success and error.
    assert_eq!(
        std::fs::read(dir.path().join("example.decl")).unwrap(),
        SOURCE.as_bytes()
    );
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files, vec![std::ffi::OsString::from("example.decl")]);
    result
}

fn script(name: &str) -> &'static str {
    exploration::SCRIPTS
        .iter()
        .find(|(id, _)| *id == name)
        .unwrap()
        .1
}

#[test]
fn e1_semantic_explorer_reports_selected_declaration_without_mutation() {
    let (report, output) = run(script("e1")).unwrap();
    assert_eq!(
        json_value(&report),
        json!({
            "title": "Semantic explorer: selected declaration",
            "operation": ACTION,
            "status": "read_only",
            "name": "answer",
            "kind": "integer_literal_binding",
            "literal": "42",
            "partial": true,
            "inferred_type": "unknown",
            "references": "unsupported",
            "rationale": RATIONALE
        })
    );
    assert_eq!(output, vec![report]);
}

#[test]
fn e2_capability_inspector_reports_metadata_without_invocation_or_mutation() {
    let source = script("e2");
    // This is a structural check, not an invocation counter: the runner does
    // not expose one. The sole host access in this example is discovery.
    assert_eq!(source.matches("scope.").count(), 1);
    assert_eq!(source.matches("scope.operations()").count(), 1);
    assert!(!source.contains("invoke"));

    let (report, output) = run(source).unwrap();
    assert_eq!(
        json_value(&report),
        json!({
            "title": "Capability metadata (not an authorization grant)",
            "available": {
                "provider": "semantic",
                "identifier": ACTION,
                "description": "Read-only partial description of the selected declaration; no evaluation or edits. Select exactly one complete declaration line.",
                "arguments": [],
                "constraints_json": "{\"selection\":\"one complete declaration line, excluding newline\",\"unsupported\":[\"references\",\"inferred_type\"]}",
                "target": "Selection",
                "effect": "Read",
                "purpose": "Exploration",
                "produces_evaluation": false,
                "invocation_schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["expected_version", "expected_source", "arguments"],
                    "properties": {
                        "expected_version": {"type": "string", "description": "Copy the host edit_request revision exactly; decimal u64"},
                        "expected_source": {"type": "string", "description": "Copy the host edit_request source exactly"},
                        "arguments": {"type": "object", "additionalProperties": false, "properties": {}, "required": []}
                    }
                }
            },
            "unavailable": {
                "executable_authorization_grant": "not exposed; discovery grants no authority",
                "source_policy": "host-owned; not exposed in operation metadata",
                "selected_document_and_range": "host-owned; not exposed in operation metadata"
            }
        })
    );
    assert_eq!(output, vec![report]);
}

#[test]
fn e3_structured_report_preserves_nested_analysis_and_print_order_without_mutation() {
    let (report, output) = run(script("e3")).unwrap();
    assert_eq!(
        json_value(&report),
        json!({
            "title": "Structured semantic report",
            "provenance": {"operation": ACTION, "status": "read_only"},
            "declaration": {
                "name": "answer",
                "binding": {"kind": "integer_literal_binding", "literal": "42"},
                "location": {
                    "zero_based": {"line": 0, "start_column": 0, "end_column": 15},
                    "display_line": 1
                }
            },
            "analysis": {
                "partial": true,
                "inferred_type": {"status": "unknown", "reason": "fixture has no type analysis"},
                "references": {"status": "unsupported"},
                "rationale": RATIONALE
            },
            "sections": ["declaration", "analysis"]
        })
    );
    assert_eq!(output, vec!["Structured semantic report".into(), report]);
}

#[test]
fn missing_authorization_metadata_fails_without_mutation() {
    let error = run("scope.operations()[0].metadata.authorization").unwrap_err();
    let error = error.downcast_ref::<scope_script::Error>().unwrap();
    assert!(
        matches!(&error.kind, scope_script::ErrorKind::Runtime(message) if message == "missing object field"),
        "{error:?}"
    );
}
