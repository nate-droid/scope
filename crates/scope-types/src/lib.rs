//! Semantic composition only: no execution, WIT interpretation, or authorization.
//! Types match by exact name and version. Capability names may reuse Cell interface
//! identifiers, but only the host can supply grants; Cell enforces actual operations.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod manifest;
pub use manifest::*;

/// Nominal identity. Versions are opaque strings, not compatibility ranges.
#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TypeId {
    pub name: String,
    pub version: String,
}

impl TypeId {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Port {
    pub name: String,
    pub ty: TypeId,
}

impl Port {
    pub fn new(name: impl Into<String>, ty: TypeId) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

/// A named, versioned declaration; requirements are never grants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transformation {
    pub id: TypeId,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub requires: BTreeSet<String>,
}

/// Host-supplied planning permissions, not runtime authorization tokens.
/// Never construct this from a transformation's requirements or advisory Cell metadata.
pub type Capabilities = BTreeSet<String>;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct Endpoint {
    pub node: String,
    pub port: String,
}

impl Endpoint {
    pub fn new(node: impl Into<String>, port: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            port: port.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Source {
    Output(Endpoint),
    /// A named external graph input, explicitly typed in `Graph::inputs`.
    Input(String),
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    pub from: Source,
    pub to: Endpoint,
}

/// Editable description that retains incomplete and invalid wiring for diagnostics.
/// Node names are graph-local; multiple nodes can use the same transformation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Graph {
    pub inputs: BTreeMap<String, TypeId>,
    pub nodes: BTreeMap<String, Transformation>,
    pub connections: Vec<Connection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Input,
    Output,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Diagnostic {
    DuplicatePort {
        node: String,
        direction: Direction,
        port: String,
    },
    UnknownNode {
        endpoint: Endpoint,
    },
    UnknownPort {
        endpoint: Endpoint,
        direction: Direction,
    },
    UnknownInput {
        name: String,
    },
    TypeMismatch {
        target: Endpoint,
        expected: TypeId,
        actual: TypeId,
    },
    MultipleSources {
        target: Endpoint,
    },
    MissingInput {
        target: Endpoint,
    },
    MissingCapability {
        node: String,
        capability: String,
    },
    Cycle,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePort {
                node,
                direction,
                port,
            } => write!(f, "duplicate {direction:?} port {node}.{port}"),
            Self::UnknownNode { endpoint } => write!(
                f,
                "unknown node {} at port {}",
                endpoint.node, endpoint.port
            ),
            Self::UnknownPort {
                endpoint,
                direction,
            } => write!(
                f,
                "unknown {direction:?} port {}.{}",
                endpoint.node, endpoint.port
            ),
            Self::UnknownInput { name } => write!(f, "unknown graph input {name}"),
            Self::TypeMismatch {
                target,
                expected,
                actual,
            } => write!(
                f,
                "{}.{} expects {expected}, got {actual}",
                target.node, target.port
            ),
            Self::MultipleSources { target } => {
                write!(f, "multiple sources for {}.{}", target.node, target.port)
            }
            Self::MissingInput { target } => {
                write!(f, "missing input {}.{}", target.node, target.port)
            }
            Self::MissingCapability { node, capability } => {
                write!(f, "{node} requires ungranted capability {capability}")
            }
            Self::Cycle => write!(f, "transformation graph contains a cycle"),
        }
    }
}

impl Graph {
    /// Look up a declared source type without validating the rest of the graph.
    #[expect(
        clippy::result_large_err,
        reason = "Keep structured diagnostics consistent with the public graph validation API"
    )]
    pub fn source_type(&self, source: &Source) -> Result<&TypeId, Diagnostic> {
        match source {
            Source::Input(name) => self
                .inputs
                .get(name)
                .ok_or_else(|| Diagnostic::UnknownInput { name: name.clone() }),
            Source::Output(endpoint) => self.port(endpoint, Direction::Output),
        }
    }

    #[expect(
        clippy::result_large_err,
        reason = "Return the same structured diagnostics collected by graph validation"
    )]
    fn port<'a>(
        &'a self,
        endpoint: &Endpoint,
        direction: Direction,
    ) -> Result<&'a TypeId, Diagnostic> {
        let node = self
            .nodes
            .get(&endpoint.node)
            .ok_or_else(|| Diagnostic::UnknownNode {
                endpoint: endpoint.clone(),
            })?;
        let ports = match direction {
            Direction::Input => &node.inputs,
            Direction::Output => &node.outputs,
        };
        ports
            .iter()
            .find(|port| port.name == endpoint.port)
            .map(|port| &port.ty)
            .ok_or_else(|| Diagnostic::UnknownPort {
                endpoint: endpoint.clone(),
                direction,
            })
    }

    /// All diagnostics, including missing inputs in otherwise incomplete graphs.
    /// Invalid connections do not satisfy required inputs. Outputs may be unused;
    /// fan-out is allowed, but every input needs exactly one compatible source.
    pub fn validate(&self, granted: &Capabilities) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for (name, node) in &self.nodes {
            for (direction, ports) in [
                (Direction::Input, &node.inputs),
                (Direction::Output, &node.outputs),
            ] {
                let mut seen = BTreeSet::new();
                for port in ports {
                    if !seen.insert(&port.name) {
                        diagnostics.push(Diagnostic::DuplicatePort {
                            node: name.clone(),
                            direction,
                            port: port.name.clone(),
                        });
                    }
                }
            }
            for capability in node.requires.difference(granted) {
                diagnostics.push(Diagnostic::MissingCapability {
                    node: name.clone(),
                    capability: capability.clone(),
                });
            }
        }
        let mut connected = BTreeSet::new();
        let mut targets = BTreeSet::new();
        let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for connection in &self.connections {
            let expected = self
                .port(&connection.to, Direction::Input)
                .map_err(|error| diagnostics.push(error))
                .ok();
            if !targets.insert(&connection.to) {
                diagnostics.push(Diagnostic::MultipleSources {
                    target: connection.to.clone(),
                });
            }
            let actual = self
                .source_type(&connection.from)
                .map_err(|error| diagnostics.push(error))
                .ok();
            if actual.is_some()
                && expected.is_some()
                && let Source::Output(endpoint) = &connection.from
            {
                edges
                    .entry(&endpoint.node)
                    .or_default()
                    .insert(&connection.to.node);
            }
            if let (Some(expected), Some(actual)) = (expected, actual) {
                if expected != actual {
                    diagnostics.push(Diagnostic::TypeMismatch {
                        target: connection.to.clone(),
                        expected: expected.clone(),
                        actual: actual.clone(),
                    });
                } else {
                    connected.insert(&connection.to);
                }
            }
        }
        for (name, node) in &self.nodes {
            for input in &node.inputs {
                let target = Endpoint::new(name, &input.name);
                if !connected.contains(&target) {
                    diagnostics.push(Diagnostic::MissingInput { target });
                }
            }
        }
        // Kahn's algorithm avoids recursion on arbitrarily deep editable graphs.
        let mut indegree: BTreeMap<&str, usize> =
            self.nodes.keys().map(|name| (name.as_str(), 0)).collect();
        for targets in edges.values() {
            for target in targets {
                *indegree.get_mut(target).unwrap() += 1;
            }
        }
        let mut ready: Vec<&str> = indegree
            .iter()
            .filter_map(|(node, count)| (*count == 0).then_some(*node))
            .collect();
        let mut visited = 0;
        while let Some(node) = ready.pop() {
            visited += 1;
            if let Some(targets) = edges.get(node) {
                for target in targets {
                    let count = indegree.get_mut(target).unwrap();
                    *count -= 1;
                    if *count == 0 {
                        ready.push(target);
                    }
                }
            }
        }
        if visited != self.nodes.len() {
            diagnostics.push(Diagnostic::Cycle);
        }
        diagnostics
    }
}

/// Registration rejects ambiguous declarations rather than silently replacing them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    DuplicateTransformation(TypeId),
    DuplicatePort { direction: Direction, name: String },
}

/// A discovered graph and its selected result (including identity pipelines).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pipeline {
    pub graph: Graph,
    pub output: Source,
}

#[derive(Clone, Debug, Default)]
pub struct Registry {
    transformations: BTreeMap<TypeId, Transformation>,
}

impl Registry {
    pub fn register(&mut self, transformation: Transformation) -> Result<(), RegistrationError> {
        if self.transformations.contains_key(&transformation.id) {
            return Err(RegistrationError::DuplicateTransformation(
                transformation.id,
            ));
        }
        for (direction, ports) in [
            (Direction::Input, &transformation.inputs),
            (Direction::Output, &transformation.outputs),
        ] {
            let mut seen = BTreeSet::new();
            for port in ports {
                if !seen.insert(&port.name) {
                    return Err(RegistrationError::DuplicatePort {
                        direction,
                        name: port.name.clone(),
                    });
                }
            }
        }
        self.transformations
            .insert(transformation.id.clone(), transformation);
        Ok(())
    }

    pub fn get(&self, id: &TypeId) -> Option<&Transformation> {
        self.transformations.get(id)
    }

    pub fn transformations(&self) -> impl Iterator<Item = &Transformation> {
        self.transformations.values()
    }

    /// Breadth-first search returns a shortest single-input chain. Multi-input
    /// transformations are deliberately excluded: additional values cannot be
    /// inferred from the query. Any output of a multi-output step may feed the next.
    /// `None` ignores requirements for discovery only; `Some(empty)` permits pure
    /// transformations only. Always validate the resulting graph against host grants.
    /// Visited nominal types bound search even with cycles. Identity is an empty plan.
    pub fn discover(
        &self,
        input: &TypeId,
        output: &TypeId,
        permitted: Option<&Capabilities>,
    ) -> Option<Pipeline> {
        let mut queue = std::collections::VecDeque::from([(
            input.clone(),
            Vec::<(Transformation, String)>::new(),
        )]);
        let mut seen = BTreeSet::from([input.clone()]);
        while let Some((ty, path)) = queue.pop_front() {
            if &ty == output {
                let mut graph = Graph::default();
                graph.inputs.insert("input".into(), input.clone());
                let mut source = Source::Input("input".into());
                for (index, (transformation, output_port)) in path.into_iter().enumerate() {
                    let node = format!("step-{index}");
                    graph.connections.push(Connection {
                        from: source,
                        to: Endpoint::new(&node, &transformation.inputs[0].name),
                    });
                    source = Source::Output(Endpoint::new(&node, output_port));
                    graph.nodes.insert(node, transformation);
                }
                return Some(Pipeline {
                    graph,
                    output: source,
                });
            }
            for transformation in self.transformations.values() {
                if transformation.inputs.len() != 1
                    || transformation.inputs[0].ty != ty
                    || permitted.is_some_and(|grants| !transformation.requires.is_subset(grants))
                {
                    continue;
                }
                for port in &transformation.outputs {
                    if seen.insert(port.ty.clone()) {
                        let mut next = path.clone();
                        next.push((transformation.clone(), port.name.clone()));
                        queue.push_back((port.ty.clone(), next));
                    }
                }
            }
        }
        None
    }
}
