use scope_ide::scratch::ScratchBuffers;
use scope_script::{presentation::value_json, workbench::WorkbenchRunner};
use std::process::Command;

fn compare_runs(name: &str, source: &str, succeeds: bool) -> serde_json::Value {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(name);
    std::fs::write(&path, source).unwrap();
    let mut buffers = ScratchBuffers::default();
    buffers.create("scope-script", source);
    let mut runner = WorkbenchRunner::default();
    let mut previous = None;
    for _ in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_scope-script"))
            .args(["--output", "json"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.success(), succeeds, "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if let Some(previous) = &previous {
            assert_eq!(previous, &json, "{name}: repeated CLI run");
        }
        let record = runner.run(buffers.selected().unwrap());
        assert_eq!(record.outcome.is_ok(), succeeds, "{name}");
        assert_eq!(
            json["printed"],
            serde_json::json!(record.printed.iter().map(value_json).collect::<Vec<_>>()),
            "{name}"
        );
        match &record.outcome {
            Ok(value) => assert_eq!(json["outcome"]["value"], value_json(value), "{name}"),
            Err(error) => {
                assert_eq!(json["outcome"]["error"]["offset"], error.offset, "{name}");
                assert_eq!(
                    json["outcome"]["error"]["message"],
                    error.to_string(),
                    "{name}"
                );
            }
        }
        previous = Some(json);
    }
    previous.unwrap()
}

#[test]
fn pure_examples_match_repeated_cli_and_scratch_runs() {
    let text = compare_runs(
        "stdlib-text.scope",
        include_str!("../examples/stdlib-text.scope"),
        true,
    );
    assert_eq!(text["printed"][0], "PUBLICATION CHECKLIST");
    assert_eq!(text["printed"][1], "install-guide.md, api-notes.md");
    assert_eq!(text["printed"][2]["accepted"], 2);

    let collections = compare_runs(
        "stdlib-collections.scope",
        include_str!("../examples/stdlib-collections.scope"),
        true,
    );
    assert_eq!(
        collections["printed"][0],
        serde_json::json!({"recent_first": [0, 10, 2], "peak": 10, "total": 15})
    );
    assert_eq!(
        collections["printed"][1],
        serde_json::json!({"days": [1, 2, 3, 4], "original": [-3, 2, -12, 0]})
    );

    let experiment = compare_runs(
        "stdlib-experiment.scope",
        include_str!("../examples/stdlib-experiment.scope"),
        true,
    );
    assert_eq!(
        experiment["printed"][0],
        serde_json::json!({
            "accepted": [{"stage": "S10", "milliseconds": 42}, {"stage": "S12", "milliseconds": 37}],
            "rejected": [{"stage": "S11", "code": "invalid_integer"}, {"stage": "S13", "code": "missing_value"}],
            "total_ms": 79
        })
    );
    let formatted = experiment["printed"][1].as_str().unwrap();
    assert!(formatted.contains("\"accepted_count\": 2"));
    assert!(formatted.contains("\"rejected_count\": 2"));
}

#[test]
fn recoverable_boundaries_are_values_not_execution_failures() {
    let json = compare_runs(
        "results.scope",
        r#"
print(get([null], 0))
print(get([1], -1))
print(get({}, "absent"))
print(slice([1], 1, 1))
print(slice([1], 1, 0))
print(parse_int("9223372036854775808"))
print(range(3, 1))
"#,
        true,
    );
    let printed = &json["printed"];
    assert_eq!(printed[0], serde_json::json!({"ok": true, "value": null}));
    assert_eq!(printed[1]["error"]["code"], "missing_value");
    assert_eq!(printed[2]["error"]["code"], "missing_value");
    assert_eq!(printed[3], serde_json::json!({"ok": true, "value": []}));
    assert_eq!(printed[4]["error"]["code"], "invalid_index");
    assert_eq!(printed[5]["error"]["code"], "invalid_integer");
    assert_eq!(printed[6], serde_json::json!([]));
}

#[test]
fn fatal_failures_repeat_and_preserve_prior_output() {
    for (expression, message) in [
        ("trim(1)", "trim requires (string)"),
        (
            "get([], \"zero\")",
            "get requires an array and integer or object and string",
        ),
        ("abs(-9223372036854775807 - 1)", "integer overflow"),
        ("range(0, 1000001)", "limit"),
    ] {
        let json = compare_runs(
            "failure.scope",
            &format!("print(\"before\")\n{expression}"),
            false,
        );
        assert_eq!(json["printed"], serde_json::json!(["before"]));
        assert!(
            json["outcome"]["error"]["message"]
                .as_str()
                .unwrap()
                .contains(message),
            "{json}"
        );
    }
}
