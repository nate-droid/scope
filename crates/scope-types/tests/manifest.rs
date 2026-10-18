use scope_types::{
    Capabilities, CompositionDiagnostic, Connection, Diagnostic, Direction, Endpoint, Graph,
    MANIFEST_VERSION, Manifest, ManifestError, Port, Registry, Source, Transformation, TypeId,
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

fn output(node: &str, port: &str) -> Source {
    Source::Output(Endpoint::new(node, port))
}

fn wire(from: Source, node: &str, port: &str) -> Connection {
    Connection {
        from,
        to: Endpoint::new(node, port),
    }
}

fn registry(steps: Vec<Transformation>) -> Registry {
    let mut registry = Registry::default();
    for step in steps {
        registry.register(step).unwrap();
    }
    registry
}

fn single() -> Manifest {
    Manifest {
        inputs: [("value".into(), ty("A"))].into(),
        nodes: [("node".into(), ty("convert"))].into(),
        connections: vec![wire(Source::Input("value".into()), "node", "in")],
        outputs: [("result".into(), output("node", "out"))].into(),
        ..Manifest::default()
    }
}

#[test]
fn incomplete_and_unresolved_graphs_roundtrip_without_losing_references() {
    let local = registry(vec![step("convert", "A", "B")]);
    let mut incomplete = single();
    incomplete.connections.clear();
    let mut unresolved = single();
    unresolved.nodes.insert("uninstalled".into(), ty("remote"));
    unresolved.connections.push(wire(
        output("uninstalled", "unknown-port"),
        "absent-node",
        "in",
    ));
    unresolved
        .outputs
        .insert("unfinished".into(), Source::Input("absent-input".into()));

    for manifest in [incomplete, unresolved] {
        let json = manifest.to_json().unwrap();
        let imported = Manifest::from_json(&json).unwrap();
        assert_eq!(imported, manifest);
        let resolution = imported.resolve(&local, &Capabilities::new());
        assert!(!resolution.diagnostics.is_empty());
        assert_eq!(resolution.outputs(), &manifest.outputs);
        assert!(resolution.pipeline("result").is_err());
        assert_eq!(imported.to_json().unwrap(), json);
    }
}

#[test]
fn multi_node_multi_input_multi_output_graph_resolves_and_adapts() {
    let mut split = step("split", "A", "B");
    split.outputs.push(Port::new("other", ty("C")));
    let mut join = step("join", "B", "D");
    join.inputs.push(Port::new("right", ty("C")));
    let graph = Graph {
        inputs: [("left".into(), ty("A")), ("right".into(), ty("C"))].into(),
        nodes: [
            ("split".into(), split.clone()),
            ("join".into(), join.clone()),
        ]
        .into(),
        connections: vec![
            wire(Source::Input("left".into()), "split", "in"),
            wire(output("split", "out"), "join", "in"),
            wire(Source::Input("right".into()), "join", "right"),
        ],
    };
    let outputs = [
        ("result".into(), output("join", "out")),
        ("other".into(), output("split", "other")),
        ("original".into(), Source::Input("left".into())),
    ]
    .into();
    let manifest = Manifest::from_graph(&graph, outputs);
    let imported = Manifest::from_json(&manifest.to_json().unwrap()).unwrap();
    let resolution = imported.resolve(&registry(vec![split, join]), &Capabilities::new());
    assert!(resolution.diagnostics.is_empty());
    assert_eq!(resolution.graph(), &graph);
    assert_eq!(resolution.outputs(), &manifest.outputs);
    for (name, source) in &manifest.outputs {
        let pipeline = resolution.pipeline(name).unwrap();
        assert_eq!(pipeline.graph, graph);
        assert_eq!(&pipeline.output, source);
        assert_eq!(
            Manifest::from_pipeline(&pipeline, name).outputs,
            [(name.clone(), source.clone())].into()
        );
    }
    assert_eq!(
        resolution.pipeline("absent").unwrap_err(),
        vec![CompositionDiagnostic::UnknownOutput("absent".into())]
    );
}

#[test]
fn missing_transformations_types_and_capabilities_are_distinct() {
    let manifest = single();
    let missing = manifest.resolve(&Registry::default(), &Capabilities::new());
    assert!(
        missing
            .diagnostics
            .contains(&CompositionDiagnostic::MissingTransformation {
                node: "node".into(),
                id: ty("convert"),
            })
    );
    assert!(missing.graph().nodes.is_empty());
    assert_eq!(missing.pipeline("result").unwrap_err(), missing.diagnostics);

    let local = registry(vec![step("convert", "A", "B")]);
    for actual in [ty("Other"), TypeId::new("A", "2"), TypeId::new("A", "1.0")] {
        let mut invalid = manifest.clone();
        invalid.inputs.insert("value".into(), actual.clone());
        let resolution = invalid.resolve(&local, &Capabilities::new());
        assert_eq!(
            resolution.diagnostics,
            vec![
                CompositionDiagnostic::Graph(Diagnostic::TypeMismatch {
                    target: Endpoint::new("node", "in"),
                    expected: ty("A"),
                    actual,
                }),
                CompositionDiagnostic::Graph(Diagnostic::MissingInput {
                    target: Endpoint::new("node", "in"),
                }),
            ]
        );
        assert_eq!(
            resolution.pipeline("result").unwrap_err(),
            resolution.diagnostics
        );
    }

    let mut required = step("convert", "A", "B");
    required.requires.insert("host:network".into());
    let local = registry(vec![required]);
    let grants = Capabilities::new();
    let denied = manifest.resolve(&local, &grants);
    assert_eq!(
        denied.diagnostics,
        vec![CompositionDiagnostic::Graph(
            Diagnostic::MissingCapability {
                node: "node".into(),
                capability: "host:network".into()
            }
        )]
    );
    assert!(grants.is_empty());
    assert_eq!(denied.pipeline("result").unwrap_err(), denied.diagnostics);
    assert!(
        manifest
            .resolve(&local, &["host:network".into()].into())
            .pipeline("result")
            .is_ok()
    );
}

#[test]
fn transformation_versions_require_exact_local_registration() {
    let mut manifest = single();
    manifest
        .nodes
        .insert("node".into(), TypeId::new("convert", "2"));
    let resolution = manifest.resolve(
        &registry(vec![step("convert", "A", "B")]),
        &Capabilities::new(),
    );
    assert!(
        resolution
            .diagnostics
            .contains(&CompositionDiagnostic::MissingTransformation {
                node: "node".into(),
                id: TypeId::new("convert", "2"),
            })
    );
}

#[test]
fn invalid_named_outputs_report_the_name_and_reference_error() {
    let local = registry(vec![step("convert", "A", "B")]);
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
        let mut manifest = single();
        manifest.outputs.insert("bad".into(), source);
        let resolution = manifest.resolve(&local, &Capabilities::new());
        assert_eq!(
            resolution.diagnostics,
            vec![CompositionDiagnostic::Output {
                name: "bad".into(),
                diagnostic,
            }]
        );
        // A bad unselected output still blocks adaptation of the entire composition.
        assert_eq!(
            resolution.pipeline("result").unwrap_err(),
            resolution.diagnostics
        );
        let mut expected = resolution.diagnostics.clone();
        expected.push(CompositionDiagnostic::UnknownOutput("absent".into()));
        assert_eq!(resolution.pipeline("absent").unwrap_err(), expected);
    }
}

#[test]
fn schema_versions_and_malformed_json_are_rejected() {
    assert_eq!(Manifest::default().version, MANIFEST_VERSION);
    for version in [0, MANIFEST_VERSION + 1] {
        let mut manifest = single();
        manifest.version = version;
        assert!(
            matches!(manifest.to_json(), Err(ManifestError::UnsupportedVersion(v)) if v == version)
        );
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(
            matches!(Manifest::from_json(&json), Err(ManifestError::UnsupportedVersion(v)) if v == version)
        );
        let resolution = manifest.resolve(&Registry::default(), &Capabilities::new());
        assert_eq!(
            resolution.diagnostics,
            vec![CompositionDiagnostic::UnsupportedVersion(version)]
        );
        assert_eq!(resolution.graph().inputs, manifest.inputs);
        assert_eq!(resolution.graph().connections, manifest.connections);
        assert!(resolution.graph().nodes.is_empty());
        assert_eq!(resolution.outputs(), &manifest.outputs);
        assert!(resolution.pipeline("result").is_err());
    }
    for json in ["not json", "{}", r#"{"version":"1"}"#] {
        assert!(matches!(
            Manifest::from_json(json),
            Err(ManifestError::Json(_))
        ));
    }
}

#[test]
fn serialized_format_contains_references_not_local_implementations_or_authority() {
    let mut declaration = step("convert", "A", "B");
    declaration.requires.insert("private:requirement".into());
    let graph = Graph {
        inputs: single().inputs,
        nodes: [("node".into(), declaration.clone())].into(),
        connections: single().connections,
    };
    let manifest = Manifest::from_graph(&graph, single().outputs);
    let json = manifest.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "version": MANIFEST_VERSION,
            "inputs": {"value": {"name": "A", "version": "1"}},
            "nodes": {"node": {"name": "convert", "version": "1"}},
            "connections": [{
                "from": {"kind": "input", "value": "value"},
                "to": {"node": "node", "port": "in"}
            }],
            "outputs": {"result": {"kind": "output", "value": {"node": "node", "port": "out"}}}
        })
    );
    assert!(!json.contains("private:requirement"));

    // Optional extensions cannot install implementations, replace requirements, or grant access.
    let mut extended = value;
    extended["implementations"] = serde_json::json!({"convert": "untrusted-component"});
    extended["grants"] = serde_json::json!(["private:requirement"]);
    extended["requirements"] = serde_json::json!([]);
    let imported = Manifest::from_json(&extended.to_string()).unwrap();
    assert_eq!(imported, manifest);
    let resolution = imported.resolve(&registry(vec![declaration]), &Capabilities::new());
    assert_eq!(
        resolution.diagnostics,
        vec![CompositionDiagnostic::Graph(
            Diagnostic::MissingCapability {
                node: "node".into(),
                capability: "private:requirement".into()
            }
        )]
    );
    assert!(resolution.pipeline("result").is_err());
    assert_eq!(imported.to_json().unwrap(), json);
}

#[test]
fn implementation_checks_replace_previous_results() {
    let local = registry(vec![step("convert", "A", "B")]);
    let available = [ty("convert")];
    let resolution = single().resolve(&local, &Capabilities::new());
    let missing = resolution.with_implementations(std::iter::empty());
    let expected = vec![CompositionDiagnostic::MissingImplementation {
        node: "node".into(),
        id: ty("convert"),
    }];
    assert_eq!(missing.diagnostics, expected);
    assert!(missing.pipeline("result").is_err());

    let repeated = missing.with_implementations(std::iter::empty());
    assert_eq!(repeated.diagnostics, expected);
    let installed = repeated.with_implementations(&available);
    assert!(installed.diagnostics.is_empty());
    assert!(installed.pipeline("result").is_ok());
    let removed = installed.with_implementations(std::iter::empty());
    assert_eq!(removed.diagnostics, expected);
}

#[test]
fn implementation_checks_preserve_other_diagnostics() {
    let local = registry(vec![step("convert", "A", "B")]);
    let mut manifest = single();
    manifest.connections.clear();
    let resolution = manifest.resolve(&local, &Capabilities::new());
    let expected = resolution.diagnostics.clone();
    assert!(!expected.is_empty());

    let resolution = resolution
        .with_implementations(std::iter::empty())
        .with_implementations(&[ty("convert")]);
    assert_eq!(resolution.diagnostics, expected);
    assert!(resolution.pipeline("result").is_err());
}

#[test]
fn identity_pipeline_roundtrips_without_nodes_or_bindings() {
    let local = Registry::default();
    let pipeline = local
        .discover(&ty("A"), &ty("A"), Some(&Capabilities::new()))
        .unwrap();
    let manifest = Manifest::from_pipeline(&pipeline, "identity");
    let imported = Manifest::from_json(&manifest.to_json().unwrap()).unwrap();
    assert_eq!(imported, manifest);
    assert!(imported.nodes.is_empty());
    assert!(imported.connections.is_empty());
    let resolution = imported.resolve(&local, &Capabilities::new());
    assert!(resolution.diagnostics.is_empty());
    let adapted = resolution.pipeline("identity").unwrap();
    assert_eq!(adapted.graph, pipeline.graph);
    assert_eq!(adapted.output, Source::Input("input".into()));
}
