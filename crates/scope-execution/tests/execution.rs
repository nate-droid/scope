mod support;

use cell::Cell;
use scope_execution::{ExecutionError, execute};
use scope_types::{Capabilities, Diagnostic, Endpoint, Graph, Pipeline, Source};
use std::collections::BTreeMap;
use support::*;

#[test]
fn portable_composition_resolves_locally_and_executes_in_cell() {
    use scope_types::{Manifest, Registry};

    let original = pipeline();
    let json = Manifest::from_pipeline(&original, "result")
        .to_json()
        .unwrap();
    let imported = Manifest::from_json(&json).unwrap();
    let mut registry = Registry::default();
    for declaration in original.graph.nodes.values() {
        registry.register(declaration.clone()).unwrap();
    }
    let grants = Capabilities::new();
    let missing = imported
        .resolve(&registry, &grants)
        .with_implementations(std::iter::empty());
    assert_eq!(missing.diagnostics.len(), 2);
    assert!(missing.diagnostics.iter().all(|diagnostic| matches!(
        diagnostic,
        scope_types::CompositionDiagnostic::MissingImplementation { .. }
    )));
    assert!(missing.pipeline("result").is_err());
    let local_bindings = bindings();
    let resolved = imported
        .resolve(&registry, &grants)
        .with_implementations(local_bindings.keys());
    assert!(resolved.diagnostics.is_empty());
    let plan = resolved.pipeline("result").unwrap();
    let result = execute(
        &Cell::new().unwrap(),
        &plan,
        &local_bindings,
        inputs("portable typed composition"),
        &grants,
    );
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.output.unwrap().bytes, 3_u64.to_le_bytes());
    assert_eq!(result.completed, ["z-uppercase", "a-count"]);
}

#[test]
fn identity_returns_the_external_value_without_bindings() {
    let plan = Pipeline {
        graph: Graph {
            inputs: BTreeMap::from([("input".into(), ty("Text"))]),
            ..Graph::default()
        },
        output: Source::Input("input".into()),
    };
    let result = execute(
        &Cell::new().unwrap(),
        &plan,
        &BTreeMap::new(),
        inputs("unchanged é"),
        &Capabilities::new(),
    );
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.output, inputs("unchanged é").remove("input"));
    assert!(result.completed.is_empty());
    assert!(result.artifacts.is_empty());
}

#[test]
fn cycles_are_rejected_before_guest_starts() {
    let mut plan = pipeline();
    plan.graph.nodes.get_mut("a-count").unwrap().outputs[0].ty = ty("Text");
    plan.graph.connections[0].from = Source::Output(Endpoint::new("a-count", "out"));
    let mut bound = bindings();
    bound.insert(ty("Uppercase"), binding(Guest::StartTrap));
    let result = execute(
        &Cell::new().unwrap(),
        &plan,
        &bound,
        inputs("hello"),
        &Capabilities::new(),
    );
    assert!(
        result
            .errors
            .contains(&ExecutionError::Validation(Diagnostic::Cycle))
    );
    assert!(result.completed.is_empty());
    assert!(result.artifacts.is_empty());
    assert!(result.output.is_none());
    assert!(
        result
            .errors
            .iter()
            .all(|error| matches!(error, ExecutionError::Validation(_)))
    );
}

#[test]
fn wrong_export_abi_is_rejected_without_instantiating_any_guest() {
    let wrong_abi = wat::parse_str(
        r#"(component
        (core module $m (func (export "run")))
        (core instance $i (instantiate $m))
        (func (export "transform") (canon lift (core func $i "run"))))"#,
    )
    .unwrap();
    let mut bound = bindings();
    bound.insert(ty("Uppercase"), binding(Guest::StartTrap));
    bound.get_mut(&ty("WordCount")).unwrap().component = wrong_abi;
    let result = execute(
        &Cell::new().unwrap(),
        &pipeline(),
        &bound,
        inputs("hello"),
        &Capabilities::new(),
    );
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
    assert!(matches!(&result.errors[0], ExecutionError::Binding { node, .. } if node == "a-count"));
    assert!(result.completed.is_empty());
    assert!(result.artifacts.is_empty());
    assert!(result.output.is_none());
}

#[test]
fn guest_starts_are_deferred_until_dependencies_complete() {
    let mut bound = bindings();
    bound.insert(ty("WordCount"), binding(Guest::StartTrap));
    let result = execute(
        &Cell::new().unwrap(),
        &pipeline(),
        &bound,
        inputs("hello"),
        &Capabilities::new(),
    );
    assert_eq!(result.completed, ["z-uppercase"]);
    assert_eq!(
        result.artifacts[&Endpoint::new("z-uppercase", "out")].bytes,
        b"HELLO"
    );
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.errors.len(), 1);
    assert!(matches!(&result.errors[0], ExecutionError::Runtime { node, .. } if node == "a-count"));
    assert!(result.output.is_none());
}

#[test]
fn unknown_selected_outputs_are_rejected_before_execution() {
    for source in [
        Source::Input("missing".into()),
        Source::Output(Endpoint::new("missing", "out")),
        Source::Output(Endpoint::new("a-count", "missing")),
    ] {
        let mut plan = pipeline();
        plan.output = source;
        let result = execute(
            &Cell::new().unwrap(),
            &plan,
            &bindings(),
            inputs("hello"),
            &Capabilities::new(),
        );
        assert_eq!(
            result.errors,
            vec![ExecutionError::Output {
                node: String::new(),
                message: "unknown selected pipeline output".into(),
            }]
        );
        assert!(result.completed.is_empty());
        assert!(result.artifacts.is_empty());
        assert!(result.output.is_none());
    }
}

#[test]
fn selected_output_validation_preserves_whole_plan_and_error_order() {
    let cell = Cell::new().unwrap();
    for source in [
        Source::Input("input".into()),
        Source::Input("absent".into()),
    ] {
        let mut plan = pipeline();
        plan.output = source.clone();
        plan.graph
            .nodes
            .get_mut("a-count")
            .unwrap()
            .requires
            .insert("network".into());
        let result = execute(
            &cell,
            &plan,
            &BTreeMap::new(),
            BTreeMap::new(),
            &Capabilities::new(),
        );
        let mut expected = vec![
            ExecutionError::Validation(Diagnostic::MissingCapability {
                node: "a-count".into(),
                capability: "network".into(),
            }),
            ExecutionError::Input {
                name: "input".into(),
                message: "missing graph input".into(),
            },
        ];
        if source == Source::Input("absent".into()) {
            expected.push(ExecutionError::Output {
                node: String::new(),
                message: "unknown selected pipeline output".into(),
            });
        }
        assert_eq!(result.errors, expected);
        assert!(result.completed.is_empty());
        assert!(result.artifacts.is_empty());
        assert!(result.output.is_none());
    }
}

#[test]
fn dynamic_guests_transform_and_run_in_dependency_order() {
    let cell = Cell::new().unwrap();
    for (text, uppercase, count) in [
        ("hello wasm world", "HELLO WASM WORLD", 3_u64),
        (
            " \tOne\n two\r\nTHREE\x0b four\x0c",
            " \tONE\n TWO\r\nTHREE\x0b FOUR\x0c",
            4,
        ),
        ("", "", 0),
        (" \t\n\r\x0b\x0c", " \t\n\r\x0b\x0c", 0),
        ("a,b 123 élan", "A,B 123 éLAN", 3),
    ] {
        let result = execute(
            &cell,
            &pipeline(),
            &bindings(),
            inputs(text),
            &Capabilities::new(),
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.completed, ["z-uppercase", "a-count"]);
        let intermediate = &result.artifacts[&Endpoint::new("z-uppercase", "out")];
        assert_eq!(intermediate.ty, ty("Text"));
        assert_eq!(intermediate.bytes, uppercase.as_bytes());
        let output = result.output.as_ref().expect("selected result");
        assert_eq!(output.ty, ty("Number"));
        assert_eq!(output.bytes, count.to_le_bytes());
        let artifact = &result.artifacts[&Endpoint::new("a-count", "out")];
        assert_eq!(artifact.ty, output.ty);
        assert_eq!(artifact.bytes, output.bytes);
    }
}

#[test]
fn incompatible_graph_types_are_rejected_before_execution() {
    let mut plan = pipeline();
    plan.graph.nodes.get_mut("a-count").unwrap().inputs[0].ty = ty("Number");
    let result = execute(
        &Cell::new().unwrap(),
        &plan,
        &bindings(),
        inputs("hello"),
        &Capabilities::new(),
    );
    assert!(result.errors.iter().any(|error| matches!(error,
        ExecutionError::Validation(Diagnostic::TypeMismatch { target, expected, actual })
            if target == &Endpoint::new("a-count", "in") && expected == &ty("Number") && actual == &ty("Text"))));
    assert!(result.completed.is_empty());
    assert!(result.artifacts.is_empty());
    assert!(result.output.is_none());
}

#[test]
fn declared_requirements_are_not_grants() {
    let mut plan = pipeline();
    plan.graph
        .nodes
        .get_mut("z-uppercase")
        .unwrap()
        .requires
        .insert("uninstalled".into());
    let result = execute(
        &Cell::new().unwrap(),
        &plan,
        &bindings(),
        inputs("hello"),
        &Capabilities::new(),
    );
    assert!(result.errors.iter().any(|error| matches!(error,
        ExecutionError::Validation(Diagnostic::MissingCapability { node, capability })
            if node == "z-uppercase" && capability == "uninstalled")));
    assert!(result.completed.is_empty());
    assert!(result.artifacts.is_empty());
    assert!(result.output.is_none());
}

#[test]
fn planning_grant_cannot_install_a_real_guest_import() {
    let mut plan = pipeline();
    plan.graph
        .nodes
        .get_mut("z-uppercase")
        .unwrap()
        .requires
        .insert("uninstalled".into());
    let granted = Capabilities::from(["uninstalled".into()]);
    assert!(plan.graph.validate(&granted).is_empty());
    let mut bound = bindings();
    bound.insert(ty("Uppercase"), binding(Guest::UninstalledImport));
    let result = execute(
        &Cell::new().unwrap(),
        &plan,
        &bound,
        inputs("hello"),
        &granted,
    );
    assert!(result.errors.iter().any(|error| matches!(error,
        ExecutionError::Binding { node, .. } if node == "z-uppercase")));
    assert!(result.completed.is_empty());
    assert!(result.artifacts.is_empty());
    assert!(result.output.is_none());
}

#[test]
fn invalid_guest_outputs_are_not_published_or_consumed() {
    for guest in [
        Guest::WrongType,
        Guest::WrongVersion,
        Guest::WrongPort,
        Guest::MissingOutput,
        Guest::DuplicateOutput,
    ] {
        let mut bound = bindings();
        bound.insert(ty("Uppercase"), binding(guest));
        let result = execute(
            &Cell::new().unwrap(),
            &pipeline(),
            &bound,
            inputs("hello"),
            &Capabilities::new(),
        );
        assert!(
            result.errors.iter().any(|error| matches!(error,
            ExecutionError::Output { node, .. } if node == "z-uppercase")),
            "{:?}",
            result.errors
        );
        assert!(result.completed.is_empty());
        assert!(result.artifacts.is_empty());
        assert!(result.output.is_none());
    }
}

#[test]
fn traps_stop_dependent_nodes() {
    let mut bound = bindings();
    bound.insert(ty("Uppercase"), binding(Guest::Trap));
    let result = execute(
        &Cell::new().unwrap(),
        &pipeline(),
        &bound,
        inputs("hello"),
        &Capabilities::new(),
    );
    assert!(result.errors.iter().any(|error| matches!(error,
        ExecutionError::Runtime { node, .. } if node == "z-uppercase")));
    assert!(result.completed.is_empty());
    assert!(result.output.is_none());
}

#[test]
fn missing_binding_and_invalid_input_are_reported() {
    let mut bound = bindings();
    bound.remove(&ty("Uppercase"));
    let result = execute(
        &Cell::new().unwrap(),
        &pipeline(),
        &bound,
        inputs("hello"),
        &Capabilities::new(),
    );
    assert!(result.errors.iter().any(|error| matches!(error,
        ExecutionError::Binding { node, .. } if node == "z-uppercase")));
    assert!(result.output.is_none());

    for supplied in [Default::default(), {
        let mut values = inputs("hello");
        values.get_mut("input").unwrap().ty = ty("Number");
        values
    }] {
        let result = execute(
            &Cell::new().unwrap(),
            &pipeline(),
            &bindings(),
            supplied,
            &Capabilities::new(),
        );
        assert!(result.errors.iter().any(|error| matches!(error,
            ExecutionError::Input { name, .. } if name == "input")));
        assert!(result.completed.is_empty());
        assert!(result.output.is_none());
    }
}
