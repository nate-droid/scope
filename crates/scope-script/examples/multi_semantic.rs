//! Run with `cargo run -p scope-script --example multi_semantic`.
#[path = "support/multi_semantic.rs"]
mod fixture;
use fixture::*;
use scope_ide::scratch::ScratchBuffers;
use scope_script::runner::ScriptRunner;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = MultiLanguage::default();
    let mut runner = ScriptRunner::with_operations(&service, bindings())?;
    let mut scripts = ScratchBuffers::default();
    scripts.create("scope-script", COMBINED);
    let mut targets = ScratchBuffers::default();
    targets.create(LANGUAGE, "independent target source");
    runner.invoke_script_with_target(
        scripts.selected().unwrap(),
        targets.selected().ok(),
        &mut std::io::stdout(),
    )?;
    assert!(runner.records[0].outcome.is_ok());
    assert_eq!(runner.records[0].results.len(), 2);
    for (action, authorization) in [("hidden", Some(true)), ("denied", Some(false))] {
        let selected = if action == "denied" {
            vec![("echo".into(), Some(true)), (action.into(), authorization)]
        } else {
            bindings()
        };
        let mut restricted = ScriptRunner::with_operations(&service, selected)?;
        scripts.create(
            "scope-script",
            format!("scope.invoke(\"semantic\", \"language.example.multi.{action}\", {{}})"),
        );
        restricted.invoke_script_with_target(
            scripts.selected().unwrap(),
            targets.selected().ok(),
            &mut std::io::stdout(),
        )?;
        assert!(restricted.records[0].outcome.is_err());
        println!("Rejected {action} without dispatch");
    }
    assert_eq!(*service.calls.lock().unwrap(), ["echo", "measure"]);
    Ok(())
}
