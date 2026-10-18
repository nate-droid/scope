use crate::{Capabilities, Connection, Diagnostic, Graph, Pipeline, Registry, Source, TypeId};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

pub const MANIFEST_VERSION: u32 = 1;

/// Portable references only. Local implementations and host permissions are never serialized.
/// Unknown fields are reserved for optional extensions; required semantics need a new version.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub inputs: BTreeMap<String, TypeId>,
    pub nodes: BTreeMap<String, TypeId>,
    pub connections: Vec<Connection>,
    pub outputs: BTreeMap<String, Source>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            inputs: BTreeMap::new(),
            nodes: BTreeMap::new(),
            connections: Vec::new(),
            outputs: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub enum ManifestError {
    Json(serde_json::Error),
    UnsupportedVersion(u32),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid composition manifest: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported composition manifest version {version}")
            }
        }
    }
}
impl std::error::Error for ManifestError {}

/// Missing registry declarations are distinct from invalid wiring and ungranted requirements.
/// Registry declarations do not establish that runtime component bindings are installed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompositionDiagnostic {
    UnsupportedVersion(u32),
    MissingTransformation {
        node: String,
        id: TypeId,
    },
    MissingImplementation {
        node: String,
        id: TypeId,
    },
    Graph(Diagnostic),
    Output {
        name: String,
        diagnostic: Diagnostic,
    },
    UnknownOutput(String),
}

impl fmt::Display for CompositionDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported composition manifest version {version}")
            }
            Self::MissingTransformation { node, id } => {
                write!(f, "node {node}: transformation {id} is not registered")
            }
            Self::MissingImplementation { node, id } => {
                write!(f, "node {node}: no local implementation for {id}")
            }
            Self::Graph(diagnostic) => diagnostic.fmt(f),
            Self::Output { name, diagnostic } => write!(f, "graph output {name}: {diagnostic}"),
            Self::UnknownOutput(name) => write!(f, "unknown graph output {name}"),
        }
    }
}

impl Manifest {
    pub fn from_graph(graph: &Graph, outputs: BTreeMap<String, Source>) -> Self {
        Self {
            inputs: graph.inputs.clone(),
            nodes: graph
                .nodes
                .iter()
                .map(|(name, node)| (name.clone(), node.id.clone()))
                .collect(),
            connections: graph.connections.clone(),
            outputs,
            ..Self::default()
        }
    }

    pub fn from_pipeline(pipeline: &Pipeline, output_name: impl Into<String>) -> Self {
        Self::from_graph(
            &pipeline.graph,
            BTreeMap::from([(output_name.into(), pipeline.output.clone())]),
        )
    }

    pub fn from_json(json: &str) -> Result<Self, ManifestError> {
        let manifest: Self = serde_json::from_str(json).map_err(ManifestError::Json)?;
        manifest.check_version()?;
        Ok(manifest)
    }

    pub fn to_json(&self) -> Result<String, ManifestError> {
        self.check_version()?;
        serde_json::to_string_pretty(self).map_err(ManifestError::Json)
    }

    fn check_version(&self) -> Result<(), ManifestError> {
        if self.version == MANIFEST_VERSION {
            Ok(())
        } else {
            Err(ManifestError::UnsupportedVersion(self.version))
        }
    }

    /// Resolve against local declarations, never against data embedded in the document.
    /// The original manifest remains editable, including all unresolved references.
    pub fn resolve(&self, registry: &Registry, granted: &Capabilities) -> Resolution {
        let mut graph = Graph {
            inputs: self.inputs.clone(),
            connections: self.connections.clone(),
            ..Graph::default()
        };
        let mut diagnostics = Vec::new();
        if self.version != MANIFEST_VERSION {
            diagnostics.push(CompositionDiagnostic::UnsupportedVersion(self.version));
            return Resolution {
                graph,
                outputs: self.outputs.clone(),
                diagnostics,
            };
        }
        for (name, id) in &self.nodes {
            match registry.get(id) {
                Some(node) => {
                    graph.nodes.insert(name.clone(), node.clone());
                }
                None => diagnostics.push(CompositionDiagnostic::MissingTransformation {
                    node: name.clone(),
                    id: id.clone(),
                }),
            }
        }
        diagnostics.extend(
            graph
                .validate(granted)
                .into_iter()
                .map(CompositionDiagnostic::Graph),
        );
        for (name, source) in &self.outputs {
            if let Err(diagnostic) = graph.source_type(source) {
                diagnostics.push(CompositionDiagnostic::Output {
                    name: name.clone(),
                    diagnostic,
                });
            }
        }
        Resolution {
            graph,
            outputs: self.outputs.clone(),
            diagnostics,
        }
    }
}

/// An inspection result, not runtime authorization. Cell still enforces every operation.
#[derive(Clone, Debug)]
pub struct Resolution {
    graph: Graph,
    outputs: BTreeMap<String, Source>,
    pub diagnostics: Vec<CompositionDiagnostic>,
}

impl Resolution {
    pub fn graph(&self) -> &Graph {
        &self.graph
    }
    pub fn outputs(&self) -> &BTreeMap<String, Source> {
        &self.outputs
    }

    /// Optionally check host binding identities without loading or executing components.
    /// For Scope's executor, pass `bindings.keys()`. Presence does not certify ABI or authority.
    /// Each call replaces the previous implementation-presence diagnostics.
    pub fn with_implementations<'a>(
        mut self,
        available: impl IntoIterator<Item = &'a TypeId>,
    ) -> Self {
        let available: std::collections::BTreeSet<_> = available.into_iter().collect();
        self.diagnostics.retain(|diagnostic| {
            !matches!(
                diagnostic,
                CompositionDiagnostic::MissingImplementation { .. }
            )
        });
        for (name, node) in &self.graph.nodes {
            if !available.contains(&node.id) {
                self.diagnostics
                    .push(CompositionDiagnostic::MissingImplementation {
                        node: name.clone(),
                        id: node.id.clone(),
                    });
            }
        }
        self
    }

    /// Adapt a selected named result to Scope's existing executor, which checks local bindings.
    pub fn pipeline(&self, output: &str) -> Result<Pipeline, Vec<CompositionDiagnostic>> {
        let mut diagnostics = self.diagnostics.clone();
        let source = self.outputs.get(output);
        if source.is_none() {
            diagnostics.push(CompositionDiagnostic::UnknownOutput(output.into()));
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        Ok(Pipeline {
            graph: self.graph.clone(),
            output: source.unwrap().clone(),
        })
    }
}
