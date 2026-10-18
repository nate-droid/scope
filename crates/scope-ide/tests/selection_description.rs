#![cfg(feature = "tools")]
#[path = "../examples/selection_description_support/mod.rs"]
mod fixture;
use fixture::{DeclarationLanguage, SOURCE};
use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceId, SourceTarget},
    text::{Position, TextRange},
    tool::{
        AgentSession, ToolCall, ToolEffect, ToolError, ToolValue, semantic::SemanticToolProvider,
    },
};
use std::sync::atomic::Ordering;

fn selection(line: usize, end: usize) -> TextRange {
    TextRange::new(Position::new(line, 0), Position::new(line, end))
}
fn field<'a>(value: &'a ToolValue, key: &str) -> &'a ToolValue {
    let ToolValue::Object(fields) = value else {
        panic!("expected structured object: {value:?}")
    };
    fields.get(key).unwrap()
}

#[test]
fn discovery_routing_and_selection_specific_partial_description() {
    let service = DeclarationLanguage::default();
    let mut buffers = ScratchBuffers::default();
    buffers.create("example.declarations", SOURCE);
    let target = buffers.selected().unwrap();
    let before = (
        target.source_id(),
        target.version(),
        target.text().to_owned(),
        target.is_dirty(),
    );
    let mut controller = SourceEditController::default();
    for (line, end, name) in [(0, 15, "answer"), (1, 13, "count")] {
        let provider = SemanticToolProvider::new(
            &service,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(selection(line, end)),
            |authorization| {
                assert_eq!(authorization.source.text, SOURCE);
                assert_eq!(authorization.source.version, 0);
                assert_eq!(authorization.arguments, &serde_json::json!({}));
                true
            },
        )
        .unwrap();
        let mut session = AgentSession::builder().provider(provider).build().unwrap();
        let catalog = session.catalog();
        assert_eq!(catalog.tools.len(), 1);
        let entry = &catalog.tools[0];
        assert_eq!(entry.definition.effect, ToolEffect::Read);
        assert!(
            entry
                .definition
                .description
                .contains("Read-only partial description")
        );
        let metadata = entry.metadata.as_ref().unwrap();
        assert_eq!(field(metadata, "target").as_str(), Some("Selection"));
        assert_eq!(field(metadata, "purpose").as_str(), Some("Exploration"));
        assert!(
            field(metadata, "constraints_json")
                .as_str()
                .unwrap()
                .contains("unsupported")
        );
        let properties = field(field(metadata, "invocation_schema"), "properties");
        field(properties, "expected_source");
        field(properties, "expected_version");
        field(properties, "arguments");
        let result = session
            .invoke(
                ToolCall::new(&entry.definition.name)
                    .with_argument("expected_version", "0".into())
                    .with_argument("expected_source", SOURCE.into())
                    .with_argument("arguments", ToolValue::object([] as [(&str, ToolValue); 0])),
            )
            .unwrap();
        assert_eq!(field(&result.value, "status").as_str(), Some("read_only"));
        let value = field(&result.value, "value");
        assert_eq!(field(value, "name").as_str(), Some(name));
        assert_eq!(
            field(value, "kind").as_str(),
            Some("integer_literal_binding")
        );
        assert_eq!(field(value, "partial"), &ToolValue::Boolean(true));
        assert_eq!(
            field(field(value, "references"), "status").as_str(),
            Some("unsupported")
        );
        assert_eq!(
            field(field(value, "inferred_type"), "status").as_str(),
            Some("unknown")
        );
        assert_eq!(
            field(field(value, "span"), "line"),
            &ToolValue::Integer(line as i64)
        );
    }
    assert_eq!(service.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        (
            target.source_id(),
            target.version(),
            target.text().to_owned(),
            target.is_dirty()
        ),
        before
    );
    assert_eq!(controller.list().count(), 0);
}

#[test]
fn invalid_targets_authorization_and_preconditions_do_not_mutate_or_propose() {
    // Each case exercises the real session dispatcher and semantic adapter.
    for case in [
        "missing",
        "out_of_bounds",
        "partial",
        "caret",
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
            "print answer\n"
        } else {
            SOURCE
        };
        let service = DeclarationLanguage::default();
        let mut buffers = ScratchBuffers::default();
        buffers.create("example.declarations", source);
        let target = buffers.selected().unwrap();
        let dirty_before = target.is_dirty();
        let mut controller = SourceEditController::default();
        {
            let selected = match case {
                "missing" => None,
                "out_of_bounds" => Some(selection(99, 1)),
                "partial" => Some(selection(0, 3)),
                "caret" => Some(selection(0, 0)),
                "unsupported" => Some(selection(0, 12)),
                _ => Some(selection(0, 15)),
            };
            let scope = PermittedEditScope::Document(if case == "scope" {
                SourceId::Scratch(target.id() + 1)
            } else {
                target.source_id()
            });
            let provider = SemanticToolProvider::new(
                &service,
                target,
                &mut controller,
                scope,
                selected,
                |_| case != "policy",
            )
            .unwrap();
            let mut session = AgentSession::builder().provider(provider).build().unwrap();
            let name = session.catalog().tools[0].definition.name.clone();
            let mut call = ToolCall::new(name)
                .with_argument(
                    "expected_version",
                    if case == "version" { "1" } else { "0" }.into(),
                )
                .with_argument(
                    "expected_source",
                    if case == "source" {
                        "different"
                    } else {
                        source
                    }
                    .into(),
                )
                .with_argument(
                    "arguments",
                    if case == "arguments" {
                        ToolValue::object([("unknown", true.into())])
                    } else {
                        ToolValue::object([] as [(&str, ToolValue); 0])
                    },
                );
            if case == "retarget" {
                call = call.with_argument("source_id", "other".into());
            }
            let result = session.invoke_with_policy(call, |_| case != "session_policy");
            if case == "session_policy" {
                assert!(matches!(result, Err(ToolError::Denied { .. })));
            } else {
                let expected = match case {
                    "scope" | "policy" => "unauthorized",
                    "version" | "source" => "revision_conflict",
                    "unsupported" => "unsupported_operation",
                    "missing" => "ambiguous_target",
                    _ => "invalid_arguments",
                };
                assert_eq!(
                    field(&result.unwrap().value, "status").as_str(),
                    Some(expected),
                    "{case}"
                );
            }
        }
        let dispatched = matches!(case, "partial" | "caret" | "unsupported");
        assert_eq!(
            service.calls.load(Ordering::SeqCst),
            usize::from(dispatched),
            "{case}"
        );
        assert_eq!(target.text(), source, "{case}");
        assert_eq!(target.version(), 0, "{case}");
        assert_eq!(target.is_dirty(), dirty_before, "{case}");
        assert_eq!(controller.list().count(), 0, "{case}");
    }
}
