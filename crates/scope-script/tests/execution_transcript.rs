#[path = "../examples/execution_transcript.rs"]
mod example;

use scope_script::{ErrorKind, Value};

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    let Value::Object(fields) = value else {
        panic!("expected object")
    };
    &fields[key]
}

#[test]
fn transcript_preserves_values_errors_and_execution_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let source = include_str!("../examples/semantic-workspace/example.decl");
    std::fs::write(dir.path().join("example.decl"), source).unwrap();
    for case in example::CASES {
        let transcript = example::run(dir.path().to_str().unwrap(), case).unwrap();
        let prefix = vec![
            Value::String("starting".into()),
            Value::object([("phase", 2_i64.into())]),
        ];
        let mut expected = prefix.clone();
        match case {
            "success" | "rejection" => {
                let result = transcript.result.as_ref().unwrap();
                assert_eq!(
                    field(result, "status").as_str(),
                    Some(if case == "success" {
                        "read_only"
                    } else {
                        "invalid_arguments"
                    })
                );
                if case == "rejection" {
                    let Value::Object(fields) = result else {
                        unreachable!()
                    };
                    assert!(!fields.contains_key("value"));
                }
                expected.push(result.clone());
                expected.push("finished".into());
            }
            "field" => {
                assert!(
                    matches!(&transcript.result.as_ref().unwrap_err().kind, ErrorKind::Runtime(message) if message == "missing object field")
                );
                assert_eq!(
                    field(&transcript.output[2], "status").as_str(),
                    Some("read_only")
                );
                expected.push(transcript.output[2].clone());
            }
            "denied" | "missing_authorization" => {
                assert!(matches!(
                    &transcript.result.as_ref().unwrap_err().kind,
                    ErrorKind::Tool(scope_ide::tool::ToolError::Denied { tool }) if tool == "language.example.declarations.describe_selection"
                ));
            }
            "unavailable" => {
                assert!(
                    matches!(&transcript.result.as_ref().unwrap_err().kind, ErrorKind::Runtime(message) if message == "unavailable semantic operation")
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(transcript.output, expected, "{case}");
        let mut rendered = String::new();
        for (index, value) in expected.iter().enumerate() {
            rendered.push_str(&format!("output[{}]: {value:?}\n", index + 1));
        }
        match &transcript.result {
            Ok(value) => rendered.push_str(&format!("result: {value:?}\n")),
            Err(error) => rendered.push_str(&format!(
                "error: {:?} at byte {}\n",
                error.kind, error.offset
            )),
        }
        assert_eq!(transcript.render(), rendered);
        assert_eq!(transcript.render(), rendered); // Re-rendering performs no execution.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("example.decl")).unwrap(),
            source
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
