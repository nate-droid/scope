//! Explicit downstream embedding; stock Script installs none of these operations.
//! Run: cargo run -p scope-script --example recoverable_semantic
#[expect(
    dead_code,
    reason = "This example uses the shared service but supplies its own script instead of COMBINED"
)]
#[path = "support/multi_semantic.rs"]
mod fixture;
use fixture::*;
use scope_ide::scratch::ScratchBuffers;
use scope_script::runner::ScriptRunner;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = MultiLanguage::default();
    // Exposure and read authorization are supplied by this embedding, not Script.
    let mut runner = ScriptRunner::with_operations(&service, bindings())?;
    let mut scripts = ScratchBuffers::default();
    scripts.create("scope-script", r#"
fn measure(text) {
    let parsed = parse_int(text);
    let mut result = parsed;
    if parsed.ok {
        result = scope.invoke("semantic", "language.example.multi.measure", {enabled: true, limit: parsed.value});
    }
    result
}
let success = measure("3");
if success.status == "read_only" { print(success.value); } else { print(success); }
let rejected = scope.invoke("semantic", "language.example.multi.measure", {enabled: true, limit: "not an integer"});
if rejected.status == "invalid_arguments" { print(rejected.message); } else { print(rejected); }
{success: success, rejected: rejected}
"#);
    let mut targets = ScratchBuffers::default();
    targets.create(LANGUAGE, "independent target source");
    runner.invoke_script_with_target(
        scripts.selected().unwrap(),
        targets.selected().ok(),
        &mut std::io::stdout(),
    )?;
    let report = runner.records[0].outcome.as_ref().unwrap();
    let scope_script::Value::Object(fields) = report else {
        panic!("expected report")
    };
    for (key, expected) in [("success", "read_only"), ("rejected", "invalid_arguments")] {
        let scope_script::Value::Object(envelope) = &fields[key] else {
            panic!("expected envelope")
        };
        assert_eq!(
            envelope["status"],
            scope_script::Value::String(expected.into())
        );
    }
    assert_eq!(*service.calls.lock().unwrap(), ["measure"]);
    Ok(())
}
