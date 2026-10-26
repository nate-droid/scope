//! Existing ScriptRunner, explicitly host-driven; not an agent tool or benchmark arm.
use super::SOURCE;
use scope_forth::LANGUAGE;
use scope_ide::scratch::ScratchBuffers;
use scope_script::{
    presentation::value_json,
    runner::{ScriptRunner, Snapshot},
};
use serde_json::{Value, json};
use std::error::Error;

const SCRIPT: &str = r#"let words = scope.invoke("semantic", "language.example.forth.words", {document_only: true});
let misleading = scope.invoke("semantic", "language.example.forth.describe", {name: "misleading-square", symbolic_inputs: 1});
let square = scope.invoke("semantic", "language.example.forth.describe", {name: "square", symbolic_inputs: 1});
{total: words.value.total,
 misleading: {name: misleading.value.word.name, status: misleading.value.status, stack: misleading.value.symbolic.stack, stack_is_final: misleading.value.symbolic.stack_is_final, outcome: misleading.value.symbolic.outcome.status},
 square: {name: square.value.word.name, status: square.value.status, stack: square.value.symbolic.stack, stack_is_final: square.value.symbolic.stack_is_final, outcome: square.value.symbolic.outcome.status}}
"#;
fn snapshot(s: &Snapshot) -> Value {
    json!({"identity":format!("{:?}",s.identity),"revision":format!("{:?}",s.revision),"text":s.text})
}
pub fn demonstration(authorized: bool) -> Result<Value, Box<dyn Error>> {
    let mut buffers = ScratchBuffers::default();
    let target_id = buffers.create("example.forth", SOURCE);
    let script_id = buffers.create("script", SCRIPT);
    let target = buffers.get(target_id).unwrap();
    let script = buffers.get(script_id).unwrap();
    let revision = target.version();
    let dirty = target.is_dirty();
    let mut runner = ScriptRunner::with_operations(
        &LANGUAGE,
        ["words", "describe"].map(|id| (id.to_owned(), Some(authorized))),
    )?;
    runner.invoke_script_with_target(script, Some(target), &mut Vec::new())?;
    let record = runner.records.last().ok_or("missing Script RunRecord")?;
    let (final_value, error) = match &record.outcome {
        Ok(v) => (Some(value_json(v)), None),
        Err(e) => {
            let mut chain = Vec::new();
            let mut cause = e.source();
            while let Some(c) = cause {
                chain.push(c.to_string());
                cause = c.source();
            }
            (
                None,
                Some(
                    json!({"message":e.to_string(),"offset":e.offset,"debug":format!("{e:?}"),"source_chain":chain}),
                ),
            )
        }
    };
    let unchanged =
        target.text() == SOURCE && target.version() == revision && target.is_dirty() == dirty;
    assert!(unchanged);
    Ok(
        json!({"kind":"standalone host-driven existing ScriptRunner API",
        "assessment":"Not an equivalent agent benchmark or a measurement of fewer LLM rounds; Script's 64-call limit is not shared with agent budgets. Interpreter success alone is not semantic success.",
        "binding":{"script_scratch_id":script_id,"target_scratch_id":target_id,"target_version":revision,"script_version":script.version()},
                "authorization":authorized,"installed_read_operations":["words","describe"],"source_unchanged":unchanged,
        "reference_scope":"References index only this retained record.results array; not durable references",
        "partial":record.results.len() != 3,
        "result_indexes":(["words", "misleading-square", "square"].into_iter().take(record.results.len()).enumerate().map(|(index, name)| (name.to_owned(), json!(format!("results:{index}")))).collect::<serde_json::Map<String, Value>>()),
        "record":{"operation":record.operation,"source":snapshot(&record.source),"target":snapshot(&record.target),
            "selection":format!("{:?}",record.selection),"catalog":value_json(&record.catalog),
            "results":record.results.iter().map(value_json).collect::<Vec<_>>(),
            "printed":record.printed.iter().map(value_json).collect::<Vec<_>>(),
            "outcome":{"interpreter_success":record.outcome.is_ok(),"final_value":final_value,"error":error}}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_three_bound_results_and_projects_known_fields() {
        let report = demonstration(true).unwrap();
        let r = &report["record"];
        assert_eq!(report["source_unchanged"], true);
        assert_eq!(r["source"]["text"], SCRIPT);
        assert_eq!(r["target"]["text"], SOURCE);
        assert_eq!(r["source"]["revision"], "RevisionId(0)");
        assert_eq!(r["target"]["revision"], "RevisionId(0)");
        assert_ne!(
            report["binding"]["script_scratch_id"],
            report["binding"]["target_scratch_id"]
        );
        assert_eq!(r["catalog"].as_array().unwrap().len(), 2);
        let results = r["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        for (e, id) in results.iter().zip(["words", "describe", "describe"]) {
            assert_eq!(e["operation_id"], id);
            assert_eq!(e["status"], "read_only");
        }
        for e in &results[1..] {
            assert_eq!(e["value"]["source"]["revision"], "0");
        }
        assert_eq!(r["outcome"]["interpreter_success"], true);
        assert!(r["outcome"]["error"].is_null());
        assert_eq!(
            r["outcome"]["final_value"],
            json!({"total":2,
            "misleading":{"name":"misleading-square","status":"found","stack":[0,0],"stack_is_final":true,"outcome":"constructed-conditional"},
            "square":{"name":"square","status":"found","stack":[1],"stack_is_final":true,"outcome":"constructed-conditional"}})
        );
    }
    #[test]
    fn denial_is_not_successful_evidence() {
        let report = demonstration(false).unwrap();
        let r = &report["record"];
        assert_eq!(report["source_unchanged"], true);
        assert_eq!(r["outcome"]["interpreter_success"], false);
        assert!(r["outcome"]["final_value"].is_null());
        assert!(!r["outcome"]["error"].is_null());
        assert_eq!(r["results"], json!([]));
        assert_eq!(report["result_indexes"], json!({}));
        assert_eq!(report["partial"], true);
    }
}
