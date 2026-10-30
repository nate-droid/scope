use scope_ide::source_edit::SourceTarget;
use scope_ide::{
    agent::*,
    cli::{run_retained_scratch_agent_observed, RetainedScratchAgentConfig},
    scratch::ScratchBuffers,
    tool::agent_turns::{AgentTurnStore, AgentTurnToolProvider},
};
use serde_json::{json, Value};
use std::{error::Error, path::PathBuf};
struct Script(usize);
impl DecisionModel for Script {
    fn decide(&mut self, _: &[DecisionMessage], _: u32) -> Result<String, String> {
        self.0 += 1;
        Ok(if self.0 == 1 {
            json!({"type":"tool","name":"language.language.path.inspect_example","arguments":{}})
        } else {
            json!({"type":"complete","content":"Kernel result observed; compiled fixture only."})
        }
        .to_string())
    }
}
fn value(v: &ToolValue) -> Value {
    match v {
        ToolValue::Null => Value::Null,
        ToolValue::Boolean(x) => json!(x),
        ToolValue::Integer(x) => json!(x),
        ToolValue::String(x) => json!(x),
        ToolValue::Array(x) => Value::Array(x.iter().map(value).collect()),
        ToolValue::Object(x) => {
            Value::Object(x.iter().map(|(k, v)| (k.clone(), value(v))).collect())
        }
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let mode = std::env::args().nth(1).unwrap_or("fake".into());
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/results");
    std::fs::create_dir_all(&out)?;
    let mut model: Box<dyn DecisionModel> = match mode.as_str() {
        "real" => Box::new(CellAgent::new(AgentConfig::local(
            "qwen3-30b-a3b",
            "http://127.0.0.1:1234/v1",
        ))?),
        "fake" => Box::new(Script(0)),
        _ => return Err("mode must be real or fake".into()),
    };
    let mut buffers = ScratchBuffers::default();
    let id = buffers.create("language.path", path_scope::FIXTURE_SOURCE);
    let source = buffers.get(id)?;
    let store = AgentTurnStore::new();
    let mut callbacks = 0;
    let mut publish_errors = 0;
    let mut last = None;
    let mut callback = |turn: &AgentTurn| {
        callbacks += 1;
        if store.publish(turn).is_err() {
            publish_errors += 1;
        }
        last = Some(turn.id());
    };
    let allowed = vec!["inspect_example".to_string()];
    let mut authorized = 0;
    let mut authorize = |a: scope_ide::tool::semantic::SemanticAuthorization<'_>| {
        let ok = a.operation.id == "inspect_example"
            && a.source.id == source.snapshot().id
            && a.source.version == source.version()
            && a.source.text == path_scope::FIXTURE_SOURCE;
        if ok {
            authorized += 1;
        }
        ok
    };
    let result = run_retained_scratch_agent_observed(
        source,
        &path_scope::LANGUAGE,
        model.as_mut(),
        "Call language.language.path.inspect_example with empty arguments exactly once, then complete with a brief conclusion grounded in its kernel.checked result. Do not complete before invoking the tool. This is a compiled fixture inspection, not a proof of edited source. /no_think",
        AgentLimits {
            max_decisions: 3,
            max_tool_calls: 1,
            max_malformed: 1,
            max_tokens_per_decision: 320,
            max_message_bytes: 65536,
            max_conversation_bytes: 196608,
        },
        RetainedScratchAgentConfig {
            trace: None,
            allowed_operation_ids: &allowed,
            authorize: &mut authorize,
        },
        &AgentCancellationToken::default(),
        &mut callback,
    );
    let turn_id = last.ok_or("no lifecycle callback")?;
    let mut reader = AgentSession::builder()
        .provider(AgentTurnToolProvider::new(store.clone()))
        .build()?;
    let retrieved = reader
        .invoke(ToolCall::new("scope.agent.get").with_argument("id", turn_id.to_string().into()))?;
    let snapshot = store.get(turn_id)?.ok_or("snapshot missing")?;
    assert_eq!(value(&retrieved.value), snapshot);
    let imported: AgentTurn = serde_json::from_value(snapshot.clone())?;
    assert!(imported.state().is_terminal());
    std::fs::write(
        out.join(format!("{mode}-snapshot.json")),
        serde_json::to_vec_pretty(&snapshot)?,
    )?;
    let mut kernel_checked = false;
    let mut success_count = 0;
    let (stop, fresh, run_error) = match &result {
        Ok(run) => {
            for e in run.run.turn.evidence() {
                if let AgentEvidence::Tool {
                    name,
                    outcome: AgentToolOutcome::Success,
                    output: Some(output),
                    ..
                } = e
                {
                    if name == "language.language.path.inspect_example" {
                        success_count += 1;
                        let v = value(output);
                        kernel_checked |= v.pointer("/value/kernel/checked") == Some(&json!(true))
                            && v.pointer("/value/kernel/proof_check/status")
                                == Some(&json!("passed"));
                    }
                }
            }
            (
                format!("{:?}", run.run.stop),
                run.source_is_fresh(source),
                None,
            )
        }
        Err(e) => ("error".into(), false, Some(e.to_string())),
    };
    let passed = stop == "Completed"
        && kernel_checked
        && success_count == 1
        && authorized == 1
        && publish_errors == 0
        && fresh;
    let summary = json!({"mode":mode,"passed":passed,"stop":stop,"state":format!("{:?}",imported.state()),"callbacks":callbacks,"publish_errors":publish_errors,"authorizations":authorized,"semantic_success_count":success_count,"kernel_checked":kernel_checked,"fresh":fresh,"source_revision":source.version(),"source_unchanged":source.text()==path_scope::FIXTURE_SOURCE,"scope_agent_get_matches":true,"terminal_snapshot_deserialized":true,"usage":imported.usage(),"error":run_error});
    std::fs::write(
        out.join(format!("{mode}-summary.json")),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    if !passed {
        return Err("H1 acceptance criteria failed; see sanitized summary".into());
    }
    Ok(())
}
