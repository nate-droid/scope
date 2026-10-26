use scope_ide::{
    scratch::ScratchBuffers,
    tool::{ToolError, operations::OperationError},
};
use scope_script::{
    ErrorKind, Host, Interpreter, Value, host::NoHost, presentation::value_json,
    workbench::WorkbenchRunner,
};
use std::process::{Command, Stdio};

fn success(value: i64) -> Value {
    Value::object([
        ("ok", Value::Boolean(true)),
        ("value", Value::Integer(value)),
    ])
}

fn failure() -> Value {
    Value::object([
        ("ok", Value::Boolean(false)),
        (
            "error",
            Value::object([
                ("code", "invalid_integer".into()),
                ("message", "expected a signed 64-bit decimal integer".into()),
            ]),
        ),
    ])
}

fn conversion(text: &str) -> String {
    format!(
        "parse_int(\"{}\")",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    )
}

#[test]
fn signed_decimal_successes_have_only_active_fields() {
    for (text, expected) in [
        ("0", 0),
        ("-0", 0),
        ("+0", 0),
        ("42", 42),
        ("-42", -42),
        ("+42", 42),
        ("00042", 42),
        ("-00042", -42),
        ("9223372036854775807", i64::MAX),
        ("+9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
    ] {
        assert_eq!(
            Interpreter::new()
                .eval(&conversion(text), &mut NoHost)
                .unwrap(),
            success(expected),
            "{text:?}"
        );
    }
}

#[test]
fn invalid_decimal_strings_are_recoverable_and_have_only_active_fields() {
    for text in [
        "",
        "+",
        "-",
        " 1",
        "1 ",
        "\t1",
        "1\n",
        "\u{a0}1",
        "1.0",
        "1e2",
        "0x10",
        "0b10",
        "1_000",
        "1,000",
        "1 2",
        "++1",
        "--1",
        "+-1",
        "-+1",
        "１２",
        "١٢",
        "−1",
        "not-an-integer",
        "9223372036854775808",
        "-9223372036854775809",
        "+9223372036854775808",
        "999999999999999999999999999999999999999999999999999999999999",
    ] {
        assert_eq!(
            Interpreter::new()
                .eval(&conversion(text), &mut NoHost)
                .unwrap(),
            failure(),
            "{text:?}"
        );
    }
}

#[test]
fn inactive_field_access_is_fatal_not_null() {
    for (source, field) in [
        ("let result = parse_int(\"1\"); result.error", "error"),
        ("let result = parse_int(\"bad\"); result.value", "value"),
    ] {
        let error = Interpreter::new().eval(source, &mut NoHost).unwrap_err();
        assert!(
            matches!(error.kind, ErrorKind::Runtime(ref message) if message.contains("missing object field") && message.contains(field)),
            "{error:?}"
        );
    }
}

#[test]
fn wrong_types_remain_fatal_at_the_call_byte_offset() {
    for argument in ["null", "true", "42", "[]", "{}"] {
        let source = format!("print(\"é\"); parse_int({argument}); print(99)");
        let mut interpreter = Interpreter::new();
        let error = interpreter.eval(&source, &mut NoHost).unwrap_err();
        assert_eq!(
            error.offset,
            source.find("parse_int").unwrap(),
            "{argument}"
        );
        assert!(
            matches!(error.kind, ErrorKind::Runtime(ref message) if message == "parse_int requires a string"),
            "{error:?}"
        );
        assert_eq!(interpreter.take_output(), vec!["é".into()]);
    }
}

#[test]
fn syntax_failure_executes_nothing_even_after_a_recoverable_conversion() {
    for arguments in ["", "\"1\", \"2\""] {
        let source = format!("print(1); parse_int({arguments}); print(99)");
        let mut interpreter = Interpreter::new();
        let error = interpreter.eval(&source, &mut NoHost).unwrap_err();
        assert!(matches!(error.kind, ErrorKind::Syntax(_)));
        assert_eq!(error.offset, source.find("parse_int").unwrap());
        assert!(interpreter.take_output().is_empty());
    }
    let source = "print(parse_int(\"bad\")); @";
    let mut interpreter = Interpreter::new();
    let error = interpreter.eval(source, &mut NoHost).unwrap_err();
    assert!(matches!(error.kind, ErrorKind::Syntax(_)));
    assert_eq!(error.offset, source.find('@').unwrap());
    assert!(interpreter.take_output().is_empty());
}

#[test]
fn nested_helpers_propagate_results_through_iterations_and_branches() {
    let source = r#"
        fn parse(text) { parse_int(text) }
        fn increment(text) {
            let parsed = parse(text)
            let mut result = parsed
            if parsed.ok { result = {ok: true, value: parsed.value + 1} }
            result
        }
        fn relay(text) { increment(text) }
        let mut total = 0
        for text in ["41", "bad", "-2", "9223372036854775808", "+0"] {
            let result = relay(text)
            if result.ok {
                total = total + result.value
                print(result.value)
            } else {
                print(result.error)
            }
        }
        total
    "#;
    let mut interpreter = Interpreter::new();
    assert_eq!(
        interpreter.eval(source, &mut NoHost).unwrap(),
        Value::Integer(42)
    );
    let Value::Object(fields) = failure() else {
        unreachable!()
    };
    let error = fields["error"].clone();
    assert_eq!(
        interpreter.take_output(),
        vec![
            42_i64.into(),
            error.clone(),
            (-1_i64).into(),
            error,
            1_i64.into()
        ]
    );
}

#[test]
fn repeated_evaluation_preserves_results_and_recovers_after_fatal_errors() {
    let mut interpreter = Interpreter::new();
    interpreter
        .eval(
            "let good = parse_int(\"7\"); let bad = parse_int(\"bad\")",
            &mut NoHost,
        )
        .unwrap();
    for _ in 0..3 {
        assert_eq!(interpreter.eval("good", &mut NoHost).unwrap(), success(7));
        assert_eq!(interpreter.eval("bad", &mut NoHost).unwrap(), failure());
        assert!(interpreter.eval("parse_int(false)", &mut NoHost).is_err());
        assert_eq!(
            interpreter.eval("parse_int(\"8\")", &mut NoHost).unwrap(),
            success(8)
        );
        assert!(interpreter.take_output().is_empty());
    }
}

#[test]
fn repeated_recoverable_failures_do_not_bypass_execution_budget() {
    let items = std::iter::repeat_n("\"bad\"", 400)
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(
        "let items = [{items}]; for outer in items {{ for inner in items {{ parse_int(inner) }} }}; print(99)"
    );
    let mut interpreter = Interpreter::new();
    let error = interpreter.eval(&source, &mut NoHost).unwrap_err();
    assert!(matches!(error.kind, ErrorKind::LimitExceeded), "{error:?}");
    assert!(interpreter.take_output().is_empty());
    assert_eq!(
        interpreter.eval("parse_int(\"1\")", &mut NoHost).unwrap(),
        success(1)
    );
}

struct FailingHost {
    error: Option<ErrorKind>,
    calls: usize,
}
impl Host for FailingHost {
    fn operations(&mut self) -> Result<Value, ErrorKind> {
        Err(self.error.take().unwrap())
    }
    fn invoke(
        &mut self,
        provider: &str,
        action: &str,
        arguments: Value,
    ) -> Result<Value, ErrorKind> {
        assert_eq!(
            (provider, action, arguments),
            ("p", "a", Value::object([] as [(&str, Value); 0]))
        );
        self.calls += 1;
        Err(self.error.take().unwrap())
    }
}

#[test]
fn authorization_and_host_failures_are_not_conversion_results() {
    for kind in [
        ErrorKind::Operation(OperationError::Unauthorized),
        ErrorKind::Operation(OperationError::UnknownOperation),
        ErrorKind::Tool(ToolError::Denied {
            tool: "test".into(),
        }),
        ErrorKind::Runtime("host failure".into()),
        ErrorKind::LimitExceeded,
    ] {
        let expected = format!("{kind:?}");
        for call in ["scope.invoke(\"p\", \"a\", {})", "scope.operations()"] {
            let mut host = FailingHost {
                error: Some(match &kind {
                    ErrorKind::Operation(OperationError::Unauthorized) => {
                        ErrorKind::Operation(OperationError::Unauthorized)
                    }
                    ErrorKind::Operation(_) => {
                        ErrorKind::Operation(OperationError::UnknownOperation)
                    }
                    ErrorKind::Tool(_) => ErrorKind::Tool(ToolError::Denied {
                        tool: "test".into(),
                    }),
                    ErrorKind::Runtime(message) => ErrorKind::Runtime(message.clone()),
                    ErrorKind::LimitExceeded => ErrorKind::LimitExceeded,
                    _ => unreachable!(),
                }),
                calls: 0,
            };
            let source = format!(
                "fn helper() {{ parse_int(\"bad\"); {call} }}; print(1); helper(); print(99)"
            );
            let mut interpreter = Interpreter::new();
            let error = interpreter.eval(&source, &mut host).unwrap_err();
            assert_eq!(format!("{:?}", error.kind), expected);
            assert_eq!(error.offset, source.find(call).unwrap());
            assert_eq!(host.calls, usize::from(call.starts_with("scope.invoke")));
            assert_eq!(interpreter.take_output(), vec![1_i64.into()]);
        }
    }
}

#[test]
fn conversion_examples_match_cli_and_repeated_fresh_scratch_runs() {
    for (name, source, expected) in [
        (
            "conversion-handling.scope",
            include_str!("../examples/conversion-handling.scope"),
            serde_json::json!([
                {"input": "42", "next": 43},
                {"input": "not-an-integer", "code": "invalid_integer", "message": "expected a signed 64-bit decimal integer"}
            ]),
        ),
        (
            "conversion-propagation.scope",
            include_str!("../examples/conversion-propagation.scope"),
            serde_json::json!([
                {"input": "41", "value": 42},
                {"input": "not-an-integer", "error": {"code": "invalid_integer", "message": "expected a signed 64-bit decimal integer"}}
            ]),
        ),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(name);
        let output = Command::new(env!("CARGO_BIN_EXE_scope-script"))
            .args(["--output", "json"])
            .arg(&path)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["printed"], expected, "{name}");
        assert_eq!(json["outcome"]["status"], "success");
        assert_eq!(
            json["host"],
            serde_json::json!({"mode": "no-host", "invocations": []})
        );
        let mut buffers = ScratchBuffers::default();
        buffers.create("scope-script", source);
        let mut runner = WorkbenchRunner::default();
        for _ in 0..2 {
            let record = runner.run(buffers.selected().unwrap());
            assert_eq!(
                serde_json::json!(record.printed.iter().map(value_json).collect::<Vec<_>>()),
                expected
            );
            assert_eq!(
                value_json(record.outcome.as_ref().unwrap()),
                json["outcome"]["value"]
            );
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }
}
