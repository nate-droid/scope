use super::*;
fn decision(name: &str, args: Value) -> String {
    json!({"type":"tool","name":name,"arguments":args}).to_string()
}
#[test]
fn mixed_calls_and_repeated_workflow_references_are_local() {
    let r = trial(
        true,
        None,
        None,
        AgentLimits {
            max_tool_calls: 12,
            ..limits()
        },
        Some(vec![
            decision(
                "language.example.forth.words",
                json!({"document_only":true}),
            ),
            decision(WORKFLOW, json!({})),
            decision(WORKFLOW, json!({})),
            finish(),
        ]),
    )
    .unwrap();
    assert_eq!(r["evidence"].as_array().unwrap().len(), 9);
    assert_eq!(
        r["workflow_projections"][0]["attempt_indexes"],
        json!([1, 2, 3, 4])
    );
    assert_eq!(
        r["workflow_projections"][1]["attempt_indexes"],
        json!([5, 6, 7, 8])
    );
    assert_eq!(r["observations_satisfied"], true);
    assert_eq!(
        r["projection"]["script_projection"]["fourth"]["stack"],
        json!([2])
    );
}
#[test]
fn model_cannot_override_bound_source_or_revision() {
    let r = trial(
        false,
        None,
        None,
        limits(),
        Some(vec![
            decision(
                "language.example.forth.describe",
                json!({"name":"fourth","expected_version":"99","expected_source":"forged"}),
            ),
            finish(),
        ]),
    )
    .unwrap();
    assert_eq!(r["evidence"][0]["response"]["status"], "invalid_arguments");
    assert_eq!(r["source_unchanged"], true);
    assert_eq!(r["revision"], 0);
    assert_eq!(r["observations_satisfied"], false);
}
fn finish() -> String {
    json!({"type":"complete","content":"done"}).to_string()
}
#[test]
fn paired_observations_and_local_references() {
    let a = trial(false, None, None, limits(), None).unwrap();
    let b = trial(true, None, None, limits(), None).unwrap();
    assert_eq!(a["base_catalog"], b["base_catalog"]);
    assert_eq!(a["evidence"], b["evidence"]);
    let mut buffers = ScratchBuffers::default();
    let id = buffers.create("example.forth", SOURCE);
    let target = buffers.get(id).unwrap();
    let snapshot = target.snapshot();
    let identity = format!("{:?}", snapshot.id);
    for r in [&a, &b] {
        assert_eq!(r["observations_satisfied"], true);
        assert_eq!(r["source_unchanged"], true);
        assert_eq!(r["projection"]["partial"], false);
        assert_eq!(r["projection"]["source"]["revision"], "0");
        assert_eq!(r["source_binding"]["identity"], identity);
        assert_eq!(r["projection"]["source"], r["source_binding"]);
        assert_eq!(r["projection"]["source"]["text"], snapshot.text);
        assert_eq!(r["projection"]["source"]["language"], snapshot.language);
        for o in r["projection"]["observations"].as_array().unwrap() {
            let i = o["evidence_index"].as_u64().unwrap() as usize;
            assert_eq!(o["operation"], r["evidence"][i]["response"]["operation_id"]);
            if o["operation"] == "describe" {
                assert_eq!(o["source"]["revision"], "0");
                assert_eq!(o["symbolic_status"], "constructed-conditional");
                assert_eq!(o["stack_is_final"], true);
                assert!(!o["stack"].is_null());
            }
        }
    }
    assert_eq!(a["decisions"], 5);
    assert_eq!(b["decisions"], 2);
    assert_eq!(a["tool_calls"], 4);
    assert_eq!(b["tool_calls"], 5);
    assert!(
        b["conversation"][1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| {
                m["content"].as_str().is_some_and(|content| {
                    content.contains(&identity) && content.contains("tool_result")
                })
            })
    );
}
#[test]
fn authorization_outer_and_inner() {
    let outer = trial(true, None, Some(WORKFLOW), limits(), None).unwrap();
    assert_eq!(outer["evidence"], json!([]));
    assert_eq!(outer["projection"], Value::Null);
    let r = trial(
        true,
        None,
        Some("language.example.forth.describe"),
        limits(),
        None,
    )
    .unwrap();
    assert_eq!(r["source_unchanged"], true);
    assert_eq!(r["observations_satisfied"], false);
    assert_eq!(r["evidence"].as_array().unwrap().len(), 2);
    assert!(
        r["evidence"][1]["error"]
            .as_str()
            .unwrap()
            .contains("denied")
    );
    assert_eq!(r["projection"]["partial"], true);
    assert_eq!(r["projection"]["observations"].as_array().unwrap().len(), 1);
    assert_eq!(r["projection"]["attempt_indexes"], json!([0, 1]));
}
#[test]
fn typed_invalid_arguments_and_workflow_rejection() {
    for (arm, name, args) in [
        (false, "language.example.forth.describe", json!({"name":17})),
        (true, WORKFLOW, json!({"script":"untrusted"})),
    ] {
        let r = trial(
            arm,
            None,
            None,
            limits(),
            Some(vec![decision(name, args), finish()]),
        )
        .unwrap();
        assert_eq!(r["source_unchanged"], true);
        assert_eq!(r["observations_satisfied"], false);
        if arm {
            assert_eq!(r["evidence"], json!([]));
        } else {
            assert_eq!(r["evidence"][0]["response"]["status"], "invalid_arguments");
            assert_eq!(r["projection"]["partial"], true);
        }
        let feedback = r["conversation"][1]["messages"].to_string();
        assert!(feedback.contains(if arm {
            "accepts no arguments"
        } else {
            "invalid"
        }));
    }
}
#[test]
fn shared_call_budget_preserves_partial_evidence() {
    let r = trial(
        true,
        None,
        None,
        AgentLimits {
            max_tool_calls: 3,
            ..limits()
        },
        None,
    )
    .unwrap();
    assert_eq!(r["stop"], "ToolBudget");
    assert_eq!(r["tool_calls"], 3);
    assert_eq!(r["decisions"], 1);
    assert_eq!(r["source_unchanged"], true);
    assert_eq!(r["projection"]["partial"], true);
    assert_eq!(r["evidence"].as_array().unwrap().len(), 3);
    assert!(!r["evidence"][2]["error"].is_null());
    assert_eq!(r["projection"]["observations"].as_array().unwrap().len(), 2);
}
#[test]
fn byte_budgets_stop_before_model_or_feedback() {
    let r = trial(
        true,
        None,
        None,
        AgentLimits {
            max_message_bytes: 32,
            ..limits()
        },
        None,
    )
    .unwrap();
    assert_eq!(r["stop"], "MessageBudget");
    assert_eq!(r["evidence"], json!([]));
    let r = trial(
        true,
        None,
        None,
        AgentLimits {
            max_conversation_bytes: 32,
            ..limits()
        },
        None,
    )
    .unwrap();
    assert_eq!(r["stop"], "ConversationBudget");
    assert_eq!(r["source_unchanged"], true);
}
