#![cfg(feature = "tools")]
#[path = "../examples/selection_description_support/mod.rs"]
mod declarations;
#[path = "../examples/state_description_support/mod.rs"]
mod states;

use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceId, SourceTarget},
    text::{Position, TextRange},
    tool::{
        AgentSession, ToolCall, ToolEffect, ToolError, ToolValue, semantic::SemanticToolProvider,
    },
};
use scope_language::LanguageService;
use std::sync::atomic::Ordering;

fn field<'a>(value: &'a ToolValue, key: &str) -> &'a ToolValue {
    let ToolValue::Object(fields) = value else {
        panic!("expected object: {value:?}")
    };
    fields
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {value:?}"))
}
fn selection(line: usize, end: usize) -> TextRange {
    TextRange::new(Position::new(line, 0), Position::new(line, end))
}
fn call(name: &str, source: &str) -> ToolCall {
    ToolCall::new(name)
        .with_argument("expected_version", "0".into())
        .with_argument("expected_source", source.into())
        .with_argument("arguments", ToolValue::object([] as [(&str, ToolValue); 0]))
}

#[test]
fn both_models_coexist_and_are_inspectable_through_catalog_dispatch() {
    let declarations = declarations::DeclarationLanguage::default();
    let states = states::StateMachineLanguage::default();
    let mut declaration_buffers = ScratchBuffers::default();
    declaration_buffers.create(declarations.language_id(), declarations::SOURCE);
    let declaration_target = declaration_buffers.selected().unwrap();
    let mut state_buffers = ScratchBuffers::default();
    state_buffers.create(states.language_id(), states::SOURCE);
    let state_target = state_buffers.selected().unwrap();
    let mut declaration_controller = SourceEditController::default();
    let mut state_controller = SourceEditController::default();
    {
        let declaration_provider = SemanticToolProvider::new(
            &declarations,
            declaration_target,
            &mut declaration_controller,
            PermittedEditScope::Document(declaration_target.source_id()),
            Some(selection(0, 15)),
            |_| true,
        )
        .unwrap();
        let state_provider = SemanticToolProvider::new(
            &states,
            state_target,
            &mut state_controller,
            PermittedEditScope::Document(state_target.source_id()),
            Some(selection(0, 24)),
            |_| true,
        )
        .unwrap();
        let mut session = AgentSession::builder()
            .provider(declaration_provider)
            .provider(state_provider)
            .build()
            .unwrap();
        let catalog = session.catalog();
        assert_eq!(catalog.tools.len(), 2);
        assert_ne!(
            catalog.tools[0].definition.name,
            catalog.tools[1].definition.name
        );
        // The envelope and dispatch loop do not branch on language or operation names.
        let mut results = Vec::new();
        for (entry, source) in catalog
            .tools
            .iter()
            .zip([declarations::SOURCE, states::SOURCE])
        {
            assert_eq!(entry.definition.effect, ToolEffect::Read);
            let metadata = entry.metadata.as_ref().unwrap();
            assert_eq!(field(metadata, "target").as_str(), Some("Selection"));
            assert_eq!(field(metadata, "purpose").as_str(), Some("Exploration"));
            field(
                field(field(metadata, "invocation_schema"), "properties"),
                "arguments",
            );
            let result = session
                .invoke(call(&entry.definition.name, source))
                .unwrap();
            assert_eq!(field(&result.value, "status").as_str(), Some("read_only"));
            results.push(field(&result.value, "value").clone());
        }
        // Only the consumer assertions understand either provider's opaque schema.
        assert_eq!(field(&results[0], "name").as_str(), Some("answer"));
        assert_eq!(
            field(field(&results[0], "inferred_type"), "status").as_str(),
            Some("unknown")
        );
        assert_eq!(
            field(field(&results[0], "references"), "status").as_str(),
            Some("unsupported")
        );
        assert_eq!(
            field(field(&results[1], "transition"), "event").as_str(),
            Some("start")
        );
        assert_eq!(
            field(field(&results[1], "topology"), "states"),
            &ToolValue::Array(vec!["idle".into(), "running".into()])
        );
        assert_eq!(
            field(field(&results[1], "topology"), "outgoing"),
            &ToolValue::Array(vec![ToolValue::object([
                ("from", "idle".into()),
                ("to", "running".into()),
                ("event", "start".into())
            ])])
        );
        let capabilities = field(&results[1], "capabilities");
        assert_eq!(
            field(field(capabilities, "topology"), "status").as_str(),
            Some("supported")
        );
        assert_eq!(
            field(field(capabilities, "reachability"), "status").as_str(),
            Some("unknown")
        );
        assert_eq!(
            field(field(&results[1], "inferred_type"), "status").as_str(),
            Some("unsupported")
        );
        assert!(
            matches!(&results[0], ToolValue::Object(fields) if !fields.contains_key("topology"))
        );
    }
    assert_eq!(declarations.calls.load(Ordering::SeqCst), 1);
    assert_eq!(states.calls.load(Ordering::SeqCst), 1);
    assert_eq!(declaration_target.text(), declarations::SOURCE);
    assert_eq!(state_target.text(), states::SOURCE);
    assert_eq!(declaration_target.version(), 0);
    assert_eq!(state_target.version(), 0);
    assert_eq!(declaration_controller.list().count(), 0);
    assert_eq!(state_controller.list().count(), 0);
}

#[test]
fn identical_selection_authorization_and_revision_boundaries_for_both_models() {
    let declarations = declarations::DeclarationLanguage::default();
    let states = states::StateMachineLanguage::default();
    for (service, source, counter) in [
        (
            &declarations as &dyn LanguageService,
            declarations::SOURCE,
            &declarations.calls,
        ),
        (
            &states as &dyn LanguageService,
            states::SOURCE,
            &states.calls,
        ),
    ] {
        for case in [
            "second",
            "missing",
            "partial",
            "bounds",
            "scope",
            "policy",
            "session_policy",
            "version",
            "source",
            "retarget",
            "arguments",
            "unsupported",
        ] {
            let source = if case == "unsupported" {
                "unsupported construct\n"
            } else {
                source
            };
            let mut buffers = ScratchBuffers::default();
            buffers.create(service.language_id(), source);
            let target = buffers.selected().unwrap();
            let before = (
                target.source_id(),
                target.version(),
                target.text().to_owned(),
                target.is_dirty(),
            );
            let calls_before = counter.load(Ordering::SeqCst);
            let mut controller = SourceEditController::default();
            {
                let selected = match case {
                    "missing" => None,
                    "partial" => Some(selection(0, 1)),
                    "bounds" => Some(selection(99, 1)),
                    "second" => Some(selection(1, source.lines().nth(1).unwrap().len())),
                    _ => Some(selection(0, source.lines().next().unwrap().len())),
                };
                let scope = PermittedEditScope::Document(if case == "scope" {
                    SourceId::Scratch(target.id() + 1)
                } else {
                    target.source_id()
                });
                let provider = SemanticToolProvider::new(
                    service,
                    target,
                    &mut controller,
                    scope,
                    selected,
                    |_| case != "policy",
                )
                .unwrap();
                let mut session = AgentSession::builder().provider(provider).build().unwrap();
                let name = session.catalog().tools[0].definition.name.clone();
                let mut invocation = call(&name, source);
                match case {
                    "version" => {
                        invocation = invocation.with_argument("expected_version", "1".into())
                    }
                    "source" => {
                        invocation = invocation.with_argument("expected_source", "different".into())
                    }
                    "retarget" => {
                        invocation = invocation.with_argument("source_id", "other".into())
                    }
                    "arguments" => {
                        invocation = invocation.with_argument(
                            "arguments",
                            ToolValue::object([("unknown", true.into())]),
                        )
                    }
                    _ => {}
                }
                let result = session.invoke_with_policy(invocation, |_| case != "session_policy");
                if case == "session_policy" {
                    assert!(matches!(result, Err(ToolError::Denied { .. })));
                } else {
                    let expected = match case {
                        "second" => "read_only",
                        "missing" => "ambiguous_target",
                        "scope" | "policy" => "unauthorized",
                        "version" | "source" => "revision_conflict",
                        "unsupported" => "unsupported_operation",
                        _ => "invalid_arguments",
                    };
                    let result = result.unwrap();
                    assert_eq!(
                        field(&result.value, "status").as_str(),
                        Some(expected),
                        "{case}"
                    );
                    if case == "second" {
                        let value = field(&result.value, "value");
                        // Either structure is inspectable without converting it to a common model.
                        let ToolValue::Object(fields) = value else {
                            panic!("not structured")
                        };
                        assert!(
                            fields.values().any(|v| v.as_str() == Some("count"))
                                || fields
                                    .get("transition")
                                    .is_some_and(|v| field(v, "event").as_str() == Some("stop"))
                        );
                    }
                }
            }
            assert_eq!(
                counter.load(Ordering::SeqCst) - calls_before,
                usize::from(matches!(case, "second" | "partial" | "unsupported")),
                "{case}"
            );
            assert_eq!(
                (
                    target.source_id(),
                    target.version(),
                    target.text().to_owned(),
                    target.is_dirty()
                ),
                before,
                "{case}"
            );
            assert_eq!(controller.list().count(), 0, "{case}");
        }
    }
}
