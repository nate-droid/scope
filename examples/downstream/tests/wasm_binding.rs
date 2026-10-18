use cell::Cell;
use scope_execution::{Binding, ExecutionError, Value, execute};
use scope_types::{
    Capabilities, Connection, Endpoint, Graph, Pipeline, Port, Source, Transformation, TypeId,
};
use std::collections::BTreeMap;

fn pipeline() -> Pipeline {
    let bytes = TypeId::new("consumer:Bytes", "v7");
    Pipeline {
        graph: Graph {
            inputs: BTreeMap::from([("source".into(), bytes.clone())]),
            nodes: BTreeMap::from([(
                "reverse".into(),
                Transformation {
                    id: TypeId::new("consumer:ReverseBytes", "2026-10"),
                    inputs: vec![Port::new("original", bytes.clone())],
                    outputs: vec![Port::new("reversed", bytes)],
                    requires: Capabilities::new(),
                },
            )]),
            connections: vec![Connection {
                from: Source::Input("source".into()),
                to: Endpoint::new("reverse", "original"),
            }],
        },
        output: Source::Output(Endpoint::new("reverse", "reversed")),
    }
}

#[test]
fn downstream_binds_its_own_component_using_only_public_apis() {
    let plan = pipeline();
    let bindings = BTreeMap::from([(
        plan.graph.nodes["reverse"].id.clone(),
        Binding {
            component: wat::parse_str(include_str!("../fixtures/reverse.wat")).unwrap(),
            export: "reverse-bytes".into(),
        },
    )]);
    let cell = Cell::new().unwrap();
    for payload in [b"".as_slice(), b"x", b"abcde", b"abcdef", &[0, 255, 128, 1]] {
        let result = execute(
            &cell,
            &plan,
            &bindings,
            BTreeMap::from([(
                "source".into(),
                Value {
                    ty: TypeId::new("consumer:Bytes", "v7"),
                    bytes: payload.to_vec(),
                },
            )]),
            &Capabilities::new(),
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.completed, ["reverse"]);
        let output = result.output.unwrap();
        assert_eq!(output.ty, TypeId::new("consumer:Bytes", "v7"));
        assert_eq!(
            output.bytes,
            payload.iter().rev().copied().collect::<Vec<_>>()
        );
        assert_eq!(
            result.artifacts[&Endpoint::new("reverse", "reversed")],
            output
        );
    }

    // A compatible ABI does not bypass nominal output validation.
    let mut incompatible = plan.clone();
    incompatible.graph.nodes.get_mut("reverse").unwrap().outputs[0].ty =
        TypeId::new("consumer:Bytes", "v8");
    let result = execute(
        &cell,
        &incompatible,
        &bindings,
        BTreeMap::from([(
            "source".into(),
            Value {
                ty: TypeId::new("consumer:Bytes", "v7"),
                bytes: b"abc".to_vec(),
            },
        )]),
        &Capabilities::new(),
    );
    assert!(!result.errors.is_empty());
    assert!(
        result.errors.iter().all(|error| matches!(
            error, ExecutionError::Output { node, .. } if node == "reverse"
        )),
        "{:?}",
        result.errors
    );
    assert!(result.output.is_none());
    assert!(result.completed.is_empty());
    assert!(result.artifacts.is_empty());
}
