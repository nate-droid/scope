use scope_types::{
    Capabilities, Connection, Diagnostic, Direction, Endpoint, Graph, Port, RegistrationError,
    Registry, Source, Transformation, TypeId,
};

fn ty(name: &str) -> TypeId {
    TypeId::new(name, "1")
}

fn step(name: &str, input: &str, output: &str) -> Transformation {
    Transformation {
        id: ty(name),
        inputs: vec![Port::new("in", ty(input))],
        outputs: vec![Port::new("out", ty(output))],
        requires: Capabilities::new(),
    }
}

fn wire(from: Source, node: &str, port: &str) -> Connection {
    Connection {
        from,
        to: Endpoint::new(node, port),
    }
}

fn output(node: &str, port: &str) -> Source {
    Source::Output(Endpoint::new(node, port))
}

fn single(actual: TypeId) -> Graph {
    Graph {
        inputs: [("value".into(), actual)].into(),
        nodes: [("node".into(), step("convert", "A", "B"))].into(),
        connections: vec![wire(Source::Input("value".into()), "node", "in")],
    }
}

fn registry(steps: Vec<Transformation>) -> Registry {
    let mut registry = Registry::default();
    for step in steps {
        registry.register(step).unwrap();
    }
    registry
}

#[test]
fn source_type_borrows_declarations_and_only_checks_the_source() {
    let mut graph = single(TypeId::new("A", "opaque"));
    graph.connections.clear();
    let node = graph.nodes.get_mut("node").unwrap();
    node.outputs.push(Port::new("out", ty("Other")));
    node.outputs
        .push(Port::new("selected", TypeId::new("C", "2")));
    node.requires.insert("network".into());

    for (source, expected) in [
        (Source::Input("value".into()), &graph.inputs["value"]),
        (output("node", "out"), &graph.nodes["node"].outputs[0].ty),
        (
            output("node", "selected"),
            &graph.nodes["node"].outputs[2].ty,
        ),
    ] {
        assert!(std::ptr::eq(graph.source_type(&source).unwrap(), expected));
    }
    assert!(!graph.validate(&Capabilities::new()).is_empty());
    for (source, diagnostic) in [
        (
            Source::Input("absent".into()),
            Diagnostic::UnknownInput {
                name: "absent".into(),
            },
        ),
        (
            output("absent", "out"),
            Diagnostic::UnknownNode {
                endpoint: Endpoint::new("absent", "out"),
            },
        ),
        (
            output("node", "in"),
            Diagnostic::UnknownPort {
                endpoint: Endpoint::new("node", "in"),
                direction: Direction::Output,
            },
        ),
    ] {
        assert_eq!(graph.source_type(&source), Err(diagnostic));
    }
}

#[test]
fn source_errors_follow_target_and_multiple_source_errors() {
    let mut graph = single(ty("A"));
    graph.connections = vec![
        wire(Source::Input("absent".into()), "node", "out"),
        wire(output("absent", "out"), "node", "out"),
    ];
    let target = Endpoint::new("node", "out");
    assert_eq!(
        graph.validate(&Capabilities::new()),
        vec![
            Diagnostic::UnknownPort {
                endpoint: target.clone(),
                direction: Direction::Input
            },
            Diagnostic::UnknownInput {
                name: "absent".into()
            },
            Diagnostic::UnknownPort {
                endpoint: target.clone(),
                direction: Direction::Input
            },
            Diagnostic::MultipleSources { target },
            Diagnostic::UnknownNode {
                endpoint: Endpoint::new("absent", "out")
            },
            Diagnostic::MissingInput {
                target: Endpoint::new("node", "in")
            },
        ]
    );
}

#[test]
fn manifest_source_errors_follow_whole_graph_diagnostics_in_output_name_order() {
    use scope_types::{CompositionDiagnostic, Manifest};

    let mut graph = single(ty("A"));
    graph.connections.clear();
    let manifest = Manifest::from_graph(
        &graph,
        [
            ("valid".into(), Source::Input("value".into())),
            ("c-port".into(), output("node", "in")),
            ("b-node".into(), output("absent", "out")),
            ("a-input".into(), Source::Input("absent".into())),
        ]
        .into(),
    );
    let resolution = manifest.resolve(
        &registry(vec![step("convert", "A", "B")]),
        &Capabilities::new(),
    );
    let expected = vec![
        CompositionDiagnostic::Graph(Diagnostic::MissingInput {
            target: Endpoint::new("node", "in"),
        }),
        CompositionDiagnostic::Output {
            name: "a-input".into(),
            diagnostic: Diagnostic::UnknownInput {
                name: "absent".into(),
            },
        },
        CompositionDiagnostic::Output {
            name: "b-node".into(),
            diagnostic: Diagnostic::UnknownNode {
                endpoint: Endpoint::new("absent", "out"),
            },
        },
        CompositionDiagnostic::Output {
            name: "c-port".into(),
            diagnostic: Diagnostic::UnknownPort {
                endpoint: Endpoint::new("node", "in"),
                direction: Direction::Output,
            },
        },
    ];
    assert_eq!(resolution.diagnostics, expected);
    assert_eq!(resolution.pipeline("valid"), Err(expected));
}

#[test]
fn nominal_names_and_opaque_versions_must_match_exactly() {
    for actual in [ty("OtherA"), TypeId::new("A", "2"), TypeId::new("A", "1.0")] {
        assert_eq!(
            single(actual.clone()).validate(&Capabilities::new()),
            vec![
                Diagnostic::TypeMismatch {
                    target: Endpoint::new("node", "in"),
                    expected: ty("A"),
                    actual
                },
                Diagnostic::MissingInput {
                    target: Endpoint::new("node", "in")
                },
            ]
        );
    }
    assert!(single(ty("A")).validate(&Capabilities::new()).is_empty());
}

#[test]
fn incomplete_graph_reports_every_missing_input() {
    let mut graph = single(ty("A"));
    graph.connections.clear();
    graph
        .nodes
        .get_mut("node")
        .unwrap()
        .inputs
        .push(Port::new("second", ty("C")));
    assert_eq!(
        graph.validate(&Capabilities::new()),
        vec![
            Diagnostic::MissingInput {
                target: Endpoint::new("node", "in")
            },
            Diagnostic::MissingInput {
                target: Endpoint::new("node", "second")
            },
        ]
    );
}

#[test]
fn unknown_sources_do_not_satisfy_inputs() {
    let cases = [
        (
            output("absent", "out"),
            Diagnostic::UnknownNode {
                endpoint: Endpoint::new("absent", "out"),
            },
        ),
        (
            output("node", "absent"),
            Diagnostic::UnknownPort {
                endpoint: Endpoint::new("node", "absent"),
                direction: Direction::Output,
            },
        ),
        (
            Source::Input("absent".into()),
            Diagnostic::UnknownInput {
                name: "absent".into(),
            },
        ),
    ];
    for (source, diagnostic) in cases {
        let mut graph = single(ty("A"));
        graph.connections[0].from = source;
        assert_eq!(
            graph.validate(&Capabilities::new()),
            vec![
                diagnostic,
                Diagnostic::MissingInput {
                    target: Endpoint::new("node", "in")
                },
            ]
        );
    }
}

#[test]
fn unknown_target_nodes_and_input_ports_are_reported() {
    for (target, diagnostic) in [
        (
            Endpoint::new("absent", "in"),
            Diagnostic::UnknownNode {
                endpoint: Endpoint::new("absent", "in"),
            },
        ),
        (
            Endpoint::new("node", "out"),
            Diagnostic::UnknownPort {
                endpoint: Endpoint::new("node", "out"),
                direction: Direction::Input,
            },
        ),
    ] {
        let mut graph = single(ty("A"));
        graph.connections[0].to = target;
        assert_eq!(
            graph.validate(&Capabilities::new()),
            vec![
                diagnostic,
                Diagnostic::MissingInput {
                    target: Endpoint::new("node", "in")
                },
            ]
        );
    }
}

#[test]
fn duplicate_sources_include_repeated_and_distinct_connections() {
    for second in ["value", "other"] {
        let mut graph = single(ty("A"));
        graph.inputs.insert("other".into(), ty("A"));
        graph
            .connections
            .push(wire(Source::Input(second.into()), "node", "in"));
        assert_eq!(
            graph.validate(&Capabilities::new()),
            vec![Diagnostic::MultipleSources {
                target: Endpoint::new("node", "in"),
            }]
        );
    }
}

#[test]
fn registration_rejects_duplicate_ports_without_inserting() {
    for direction in [Direction::Input, Direction::Output] {
        let mut declaration = step("convert", "A", "B");
        let ports = match direction {
            Direction::Input => &mut declaration.inputs,
            Direction::Output => &mut declaration.outputs,
        };
        ports.push(ports[0].clone());
        let mut registry = Registry::default();
        assert_eq!(
            registry.register(declaration),
            Err(RegistrationError::DuplicatePort {
                direction,
                name: if direction == Direction::Input {
                    "in"
                } else {
                    "out"
                }
                .into(),
            })
        );
        assert_eq!(registry.transformations().count(), 0);
        registry.register(step("convert", "A", "B")).unwrap();
    }
}

#[test]
fn duplicate_transformation_does_not_replace_original_and_versions_are_distinct() {
    let original = step("convert", "A", "B");
    let mut registry = registry(vec![original.clone()]);
    assert_eq!(
        registry.register(step("convert", "A", "C")),
        Err(RegistrationError::DuplicateTransformation(ty("convert")))
    );
    assert_eq!(
        registry.transformations().cloned().collect::<Vec<_>>(),
        vec![original]
    );
    let mut versioned = step("convert", "A", "C");
    versioned.id.version = "2".into();
    registry.register(versioned).unwrap();
    assert_eq!(registry.transformations().count(), 2);
}

#[test]
fn editable_graph_reports_duplicate_input_and_output_ports() {
    let mut graph = single(ty("A"));
    let node = graph.nodes.get_mut("node").unwrap();
    node.inputs.push(node.inputs[0].clone());
    node.outputs.push(node.outputs[0].clone());
    assert_eq!(
        graph.validate(&Capabilities::new()),
        vec![
            Diagnostic::DuplicatePort {
                node: "node".into(),
                direction: Direction::Input,
                port: "in".into()
            },
            Diagnostic::DuplicatePort {
                node: "node".into(),
                direction: Direction::Output,
                port: "out".into()
            },
        ]
    );
}

#[test]
fn cycles_include_self_loops_and_disconnected_components() {
    let mut graph = Graph::default();
    graph.nodes.insert("a".into(), step("identity", "A", "A"));
    graph.connections.push(wire(output("a", "out"), "a", "in"));
    assert_eq!(
        graph.validate(&Capabilities::new()),
        vec![Diagnostic::Cycle]
    );
    graph.nodes.insert("b".into(), step("identity", "A", "A"));
    graph.connections = vec![
        wire(output("a", "out"), "b", "in"),
        wire(output("b", "out"), "a", "in"),
    ];
    let mut independent = step("independent", "A", "B");
    independent.inputs.clear();
    graph.nodes.insert("independent".into(), independent);
    assert_eq!(
        graph.validate(&Capabilities::new()),
        vec![Diagnostic::Cycle]
    );
}

#[test]
fn fanout_multiinput_and_unused_outputs_are_valid() {
    let mut join = step("join", "B", "C");
    join.inputs.push(Port::new("right", ty("B")));
    let graph = Graph {
        inputs: [("value".into(), ty("A"))].into(),
        nodes: [
            ("producer".into(), step("produce", "A", "B")),
            ("join".into(), join),
            ("consumer".into(), step("consume", "B", "D")),
        ]
        .into(),
        connections: vec![
            wire(Source::Input("value".into()), "producer", "in"),
            wire(output("producer", "out"), "join", "in"),
            wire(output("producer", "out"), "join", "right"),
            wire(output("producer", "out"), "consumer", "in"),
        ],
    };
    assert!(graph.validate(&Capabilities::new()).is_empty());
}

#[test]
fn multiinput_graph_accepts_independently_typed_external_inputs() {
    let mut join = step("join", "A", "C");
    join.inputs.push(Port::new("right", ty("B")));
    let graph = Graph {
        inputs: [("left".into(), ty("A")), ("right".into(), ty("B"))].into(),
        nodes: [("join".into(), join)].into(),
        connections: vec![
            wire(Source::Input("left".into()), "join", "in"),
            wire(Source::Input("right".into()), "join", "right"),
        ],
    };
    assert!(graph.validate(&Capabilities::new()).is_empty());
}

#[test]
fn requirements_are_not_grants_and_partial_grants_report_the_remainder() {
    let mut graph = single(ty("A"));
    graph.nodes.get_mut("node").unwrap().requires = ["network".into(), "storage".into()].into();
    let grants = Capabilities::new();
    assert_eq!(
        graph.validate(&grants),
        vec![
            Diagnostic::MissingCapability {
                node: "node".into(),
                capability: "network".into()
            },
            Diagnostic::MissingCapability {
                node: "node".into(),
                capability: "storage".into()
            },
        ]
    );
    assert!(grants.is_empty());
    assert_eq!(
        graph.validate(&["network".into()].into()),
        vec![Diagnostic::MissingCapability {
            node: "node".into(),
            capability: "storage".into()
        }]
    );
    assert!(
        graph
            .validate(&["network".into(), "storage".into()].into())
            .is_empty()
    );
}

#[test]
fn discovery_chooses_shortest_chain_not_registration_or_name_order() {
    let registry = registry(vec![
        step("a-first", "A", "B"),
        step("b-second", "B", "C"),
        step("z-direct", "A", "C"),
    ]);
    let plan = registry
        .discover(&ty("A"), &ty("C"), Some(&Capabilities::new()))
        .unwrap();
    assert_eq!(plan.graph.nodes.len(), 1);
    assert_eq!(plan.graph.nodes["step-0"].id, ty("z-direct"));
    assert_eq!(plan.output, output("step-0", "out"));
    assert!(plan.graph.validate(&Capabilities::new()).is_empty());
}

#[test]
fn discovery_filters_capabilities_but_unfiltered_plans_still_need_grants() {
    let mut direct = step("direct", "A", "C");
    direct.requires.insert("network".into());
    let registry = registry(vec![
        direct,
        step("first", "A", "B"),
        step("second", "B", "C"),
    ]);
    let empty = Capabilities::new();
    let unfiltered = registry.discover(&ty("A"), &ty("C"), None).unwrap();
    assert_eq!(unfiltered.graph.nodes.len(), 1);
    assert_eq!(
        unfiltered.graph.validate(&empty),
        vec![Diagnostic::MissingCapability {
            node: "step-0".into(),
            capability: "network".into()
        }]
    );
    let pure = registry.discover(&ty("A"), &ty("C"), Some(&empty)).unwrap();
    assert_eq!(pure.graph.nodes.len(), 2);
    assert!(pure.graph.validate(&empty).is_empty());
    let granted = ["network".into()].into();
    let permitted = registry
        .discover(&ty("A"), &ty("C"), Some(&granted))
        .unwrap();
    assert_eq!(permitted.graph.nodes.len(), 1);
    assert!(permitted.graph.validate(&granted).is_empty());
    let mut only = step("only", "A", "D");
    only.requires.insert("network".into());
    let only = self::registry(vec![only]);
    assert!(only.discover(&ty("A"), &ty("D"), Some(&empty)).is_none());
}

#[test]
fn discovery_terminates_on_cycles_and_requires_exact_reachable_types() {
    let registry = registry(vec![
        step("forward", "A", "B"),
        step("back", "B", "A"),
        step("exit", "B", "C"),
    ]);
    assert!(registry.discover(&ty("A"), &ty("Absent"), None).is_none());
    assert!(
        registry
            .discover(&TypeId::new("A", "2"), &ty("C"), None)
            .is_none()
    );
    assert!(
        registry
            .discover(&ty("A"), &TypeId::new("C", "2"), None)
            .is_none()
    );
    let plan = registry.discover(&ty("A"), &ty("C"), None).unwrap();
    assert_eq!(plan.graph.nodes.len(), 2);
    assert!(plan.graph.validate(&Capabilities::new()).is_empty());
}

#[test]
fn identity_discovery_returns_external_input_even_without_transformations() {
    let plan = Registry::default()
        .discover(&ty("A"), &ty("A"), Some(&Capabilities::new()))
        .unwrap();
    assert_eq!(plan.output, Source::Input("input".into()));
    assert_eq!(plan.graph.inputs, [("input".into(), ty("A"))].into());
    assert!(plan.graph.nodes.is_empty());
    assert!(plan.graph.connections.is_empty());
    assert!(plan.graph.validate(&Capabilities::new()).is_empty());
}

#[test]
fn discovery_preserves_selected_multioutput_ports_in_wiring_and_result() {
    let mut split = step("split", "A", "Unused");
    split.outputs.push(Port::new("selected", ty("B")));
    let registry = registry(vec![split, step("finish", "B", "C")]);
    let intermediate = registry.discover(&ty("A"), &ty("B"), None).unwrap();
    assert_eq!(intermediate.output, output("step-0", "selected"));
    let plan = registry.discover(&ty("A"), &ty("C"), None).unwrap();
    assert_eq!(
        plan.graph.connections[1],
        wire(output("step-0", "selected"), "step-1", "in")
    );
    assert_eq!(plan.output, output("step-1", "out"));
    assert!(plan.graph.validate(&Capabilities::new()).is_empty());
}

#[test]
fn discovery_excludes_multiinput_and_zero_input_transformations() {
    let mut join = step("join", "A", "C");
    join.inputs.push(Port::new("other", ty("A")));
    let mut generate = step("generate", "A", "D");
    generate.inputs.clear();
    let registry = registry(vec![join, generate]);
    assert!(registry.discover(&ty("A"), &ty("C"), None).is_none());
    assert!(registry.discover(&ty("A"), &ty("D"), None).is_none());
}
