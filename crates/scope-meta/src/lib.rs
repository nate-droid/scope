//! Language-neutral, immutable metadata snapshots. No host access or source loading.
//!
//! Provenance records an assertion's origin, not proof, trust, or authorization.
//! IDs and source revisions are opaque strings; type versions match exactly.
//! Construction and deserialization validate the same invariants. Cycles and
//! self-edges are allowed. Empty byte ranges are valid half-open spans.
//!
//! ```
//! use scope_meta::*;
//! let kind = TypeId::new("example.symbol", "1");
//! let mut registry = TypeRegistry::new();
//! registry.register_node(kind.clone())?;
//! let graph = MetaGraph::new(GraphId::new("demo"), RevisionId::new("1"), registry,
//!     vec![MetaNode { id: "main".into(), kind: kind.clone(), label: "main".into(), source: None }],
//!     vec![])?;
//! assert_eq!(graph.find(&kind, Some("mai"))?.len(), 1);
//! assert_eq!(graph.resolve(&graph.node_ref(&NodeId::new("main"))?)?.label, "main");
//! # Ok::<(), scope_meta::Error>(())
//! ```

mod provider;
pub use provider::{
    MetaProvider, MetaSnapshot, ProviderDescription, ProviderError, ProviderErrorKind,
    SnapshotCompleteness, SnapshotWarning,
};
pub use scope_types::TypeId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

macro_rules! string_id {
    ($name:ident) => {
        #[doc = "Opaque string identity. Empty/blank identities are rejected by graph construction."]
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
string_id!(GraphId);
string_id!(RevisionId);
string_id!(NodeId);

/// Half-open byte span `[start, end)`; no source-length or encoding assumptions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

/// Text locator only: never read, resolved, or interpreted by this crate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceRef {
    pub locator: String,
    pub span: Option<ByteRange>,
    /// Opaque source revision, independent of the graph revision.
    pub revision: Option<String>,
}

/// Editable input DTO; a published graph exposes only shared references.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MetaNode {
    pub id: NodeId,
    pub kind: TypeId,
    pub label: String,
    pub source: Option<SourceRef>,
}

/// Advisory assertion origin, never proof or a capability grant.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Provenance {
    Observed { provider: String },
    Inferred { provider: String },
    Proposed { provider: String },
}
impl Provenance {
    pub fn provider(&self) -> &str {
        match self {
            Self::Observed { provider }
            | Self::Inferred { provider }
            | Self::Proposed { provider } => provider,
        }
    }
}

/// Sorted lexicographically by `(from, relation, to, provenance)`.
/// Different provenance makes otherwise identical edges distinct.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct MetaEdge {
    pub from: NodeId,
    pub relation: TypeId,
    pub to: NodeId,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeRef {
    pub graph: GraphId,
    pub revision: RevisionId,
    pub node: NodeId,
}

/// Exact allowed endpoint-kind pairs for one relation type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RelationType {
    pub relation: TypeId,
    pub from: TypeId,
    pub to: TypeId,
    /// Additional allowed pairs, stored in canonical order by the registry.
    #[serde(default)]
    pub additional_pairs: Vec<(TypeId, TypeId)>,
}

impl RelationType {
    pub fn accepts(&self, from: &TypeId, to: &TypeId) -> bool {
        (&self.from == from && &self.to == to)
            || self
                .additional_pairs
                .iter()
                .any(|(f, t)| f == from && t == to)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct RegistryDto {
    nodes: Vec<TypeId>,
    relations: Vec<RelationType>,
}

/// Mutable registration builder. Graphs own a snapshot and expose it read-only.
/// Register endpoint kinds before relations. Node and relation namespaces are
/// separate; each exact type may be registered once in each role.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RegistryDto", into = "RegistryDto")]
pub struct TypeRegistry {
    nodes: BTreeSet<TypeId>,
    relations: BTreeMap<TypeId, RelationType>,
}
impl TypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_node(&mut self, kind: TypeId) -> Result<(), Error> {
        validate_type(&kind)?;
        if !self.nodes.insert(kind.clone()) {
            return Err(Error::DuplicateNodeType(kind));
        }
        Ok(())
    }

    pub fn register_relation(
        &mut self,
        relation: TypeId,
        from: TypeId,
        to: TypeId,
    ) -> Result<(), Error> {
        validate_type(&relation)?;
        validate_type(&from)?;
        validate_type(&to)?;
        if self.relations.contains_key(&relation) {
            return Err(Error::DuplicateRelationType(relation));
        }
        self.require_node(&from)?;
        self.require_node(&to)?;
        self.relations.insert(
            relation.clone(),
            RelationType {
                relation,
                from,
                to,
                additional_pairs: Vec::new(),
            },
        );
        Ok(())
    }

    /// Add an exact endpoint pair to an already registered relation type.
    pub fn register_relation_pair(
        &mut self,
        relation: TypeId,
        from: TypeId,
        to: TypeId,
    ) -> Result<(), Error> {
        self.require_node(&from)?;
        self.require_node(&to)?;
        let declaration = self
            .relations
            .get_mut(&relation)
            .ok_or_else(|| Error::UnknownRelationType(relation.clone()))?;
        if declaration.accepts(&from, &to) {
            return Err(Error::DuplicateRelationPair {
                relation,
                from: Box::new(from),
                to: Box::new(to),
            });
        }
        let mut pairs = std::mem::take(&mut declaration.additional_pairs);
        pairs.push((declaration.from.clone(), declaration.to.clone()));
        pairs.push((from, to));
        pairs.sort();
        let (from, to) = pairs.remove(0);
        declaration.from = from;
        declaration.to = to;
        declaration.additional_pairs = pairs;
        Ok(())
    }

    pub fn node_types(&self) -> impl Iterator<Item = &TypeId> {
        self.nodes.iter()
    }
    pub fn relation_types(&self) -> impl Iterator<Item = &RelationType> {
        self.relations.values()
    }
    pub fn contains_node(&self, kind: &TypeId) -> bool {
        self.nodes.contains(kind)
    }
    pub fn relation(&self, kind: &TypeId) -> Option<&RelationType> {
        self.relations.get(kind)
    }

    fn require_node(&self, kind: &TypeId) -> Result<(), Error> {
        if self.nodes.contains(kind) {
            Ok(())
        } else {
            Err(Error::UnknownNodeType(kind.clone()))
        }
    }
}
impl TryFrom<RegistryDto> for TypeRegistry {
    type Error = Error;
    fn try_from(dto: RegistryDto) -> Result<Self, Error> {
        let mut registry = Self::new();
        for kind in dto.nodes {
            registry.register_node(kind)?;
        }
        for relation in dto.relations {
            registry.register_relation(relation.relation.clone(), relation.from, relation.to)?;
            for (from, to) in relation.additional_pairs {
                registry.register_relation_pair(relation.relation.clone(), from, to)?;
            }
        }
        Ok(registry)
    }
}
impl From<TypeRegistry> for RegistryDto {
    fn from(registry: TypeRegistry) -> Self {
        Self {
            nodes: registry.nodes.into_iter().collect(),
            relations: registry.relations.into_values().collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GraphDto {
    graph_id: GraphId,
    revision: RevisionId,
    registry: TypeRegistry,
    nodes: Vec<MetaNode>,
    edges: Vec<MetaEdge>,
}

/// Validated immutable snapshot. Serialization is canonical by type/node/edge
/// ordering; deserialization rejects invalid registries and graphs, including duplicates.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "GraphDto", into = "GraphDto")]
pub struct MetaGraph {
    graph_id: GraphId,
    revision: RevisionId,
    registry: TypeRegistry,
    nodes: Vec<MetaNode>,
    edges: Vec<MetaEdge>,
}

/// Borrowed node description. Edge lists follow the graph's canonical ordering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeDescription<'a> {
    pub node: &'a MetaNode,
    pub outgoing: Vec<&'a MetaEdge>,
    pub incoming: Vec<&'a MetaEdge>,
}

impl MetaGraph {
    pub fn new(
        graph_id: GraphId,
        revision: RevisionId,
        registry: TypeRegistry,
        mut nodes: Vec<MetaNode>,
        mut edges: Vec<MetaEdge>,
    ) -> Result<Self, Error> {
        nonempty(graph_id.as_str(), "graph_id")?;
        nonempty(revision.as_str(), "revision")?;
        // Revalidate the complete registry through the same path as deserialization.
        let registry = TypeRegistry::try_from(RegistryDto::from(registry))?;
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        for pair in nodes.windows(2) {
            if pair[0].id == pair[1].id {
                return Err(Error::DuplicateNode(pair[0].id.clone()));
            }
        }
        for node in &nodes {
            nonempty(node.id.as_str(), "node_id")?;
            registry.require_node(&node.kind)?;
            if let Some(source) = &node.source {
                if source.locator.trim().is_empty() {
                    return Err(Error::EmptySourceLocator(node.id.clone()));
                }
                if let Some(span) = source.span
                    && span.start > span.end
                {
                    return Err(Error::InvalidByteRange {
                        node: node.id.clone(),
                        range: span,
                    });
                }
            }
        }
        edges.sort();
        for pair in edges.windows(2) {
            if pair[0] == pair[1] {
                return Err(Error::DuplicateEdge(Box::new(pair[0].clone())));
            }
        }
        let graph = Self {
            graph_id,
            revision,
            registry,
            nodes,
            edges,
        };
        for edge in &graph.edges {
            let from = graph.get(&edge.from)?;
            let to = graph.get(&edge.to)?;
            let relation = graph
                .registry
                .relation(&edge.relation)
                .ok_or_else(|| Error::UnknownRelationType(edge.relation.clone()))?;
            if !relation.accepts(&from.kind, &to.kind) {
                return Err(Error::RelationMismatch {
                    relation: edge.relation.clone(),
                    expected_from: Box::new(relation.from.clone()),
                    expected_to: Box::new(relation.to.clone()),
                    actual_from: Box::new(from.kind.clone()),
                    actual_to: Box::new(to.kind.clone()),
                });
            }
            if edge.provenance.provider().trim().is_empty() {
                return Err(Error::EmptyProvider(Box::new(edge.clone())));
            }
        }
        Ok(graph)
    }

    pub fn graph_id(&self) -> &GraphId {
        &self.graph_id
    }
    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }
    pub fn registry(&self) -> &TypeRegistry {
        &self.registry
    }
    pub fn nodes(&self) -> &[MetaNode] {
        &self.nodes
    }
    pub fn edges(&self) -> &[MetaEdge] {
        &self.edges
    }

    pub fn get(&self, id: &NodeId) -> Result<&MetaNode, Error> {
        self.nodes
            .binary_search_by(|node| node.id.cmp(id))
            .map(|i| &self.nodes[i])
            .map_err(|_| Error::UnknownNode(id.clone()))
    }

    /// Exact kind and optional case-sensitive label substring; sorted by node ID.
    /// An unknown kind is an error, whereas a known kind without matches is empty.
    pub fn find(&self, kind: &TypeId, label: Option<&str>) -> Result<Vec<&MetaNode>, Error> {
        self.registry.require_node(kind)?;
        Ok(self
            .nodes
            .iter()
            .filter(|node| &node.kind == kind && label.is_none_or(|text| node.label.contains(text)))
            .collect())
    }

    /// All outgoing edges, including self-edges, in canonical order.
    pub fn related(&self, id: &NodeId) -> Result<Vec<&MetaEdge>, Error> {
        self.get(id)?;
        Ok(self.edges.iter().filter(|edge| &edge.from == id).collect())
    }

    /// Outgoing edges of a registered relation type, in canonical order.
    pub fn related_by(&self, id: &NodeId, relation: &TypeId) -> Result<Vec<&MetaEdge>, Error> {
        self.get(id)?;
        if self.registry.relation(relation).is_none() {
            return Err(Error::UnknownRelationType(relation.clone()));
        }
        Ok(self
            .edges
            .iter()
            .filter(|edge| &edge.from == id && &edge.relation == relation)
            .collect())
    }

    pub fn source(&self, id: &NodeId) -> Result<Option<&SourceRef>, Error> {
        Ok(self.get(id)?.source.as_ref())
    }

    pub fn describe(&self, id: &NodeId) -> Result<NodeDescription<'_>, Error> {
        Ok(NodeDescription {
            node: self.get(id)?,
            outgoing: self.related(id)?,
            incoming: self.edges.iter().filter(|edge| &edge.to == id).collect(),
        })
    }

    pub fn node_ref(&self, id: &NodeId) -> Result<NodeRef, Error> {
        self.get(id)?;
        Ok(NodeRef {
            graph: self.graph_id.clone(),
            revision: self.revision.clone(),
            node: id.clone(),
        })
    }

    /// Check graph identity, then revision, then node existence (in that order).
    pub fn resolve(&self, reference: &NodeRef) -> Result<&MetaNode, Error> {
        if reference.graph != self.graph_id {
            return Err(Error::WrongGraph {
                expected: self.graph_id.clone(),
                actual: reference.graph.clone(),
            });
        }
        if reference.revision != self.revision {
            return Err(Error::StaleRevision {
                expected: self.revision.clone(),
                actual: reference.revision.clone(),
            });
        }
        self.get(&reference.node)
    }
}
impl TryFrom<GraphDto> for MetaGraph {
    type Error = Error;
    fn try_from(dto: GraphDto) -> Result<Self, Error> {
        Self::new(
            dto.graph_id,
            dto.revision,
            dto.registry,
            dto.nodes,
            dto.edges,
        )
    }
}
impl From<MetaGraph> for GraphDto {
    fn from(graph: MetaGraph) -> Self {
        Self {
            graph_id: graph.graph_id,
            revision: graph.revision,
            registry: graph.registry,
            nodes: graph.nodes,
            edges: graph.edges,
        }
    }
}

/// Structured validation/query failures. `expected` identifies the current graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    EmptyId {
        field: &'static str,
    },
    InvalidType(TypeId),
    DuplicateNodeType(TypeId),
    DuplicateRelationType(TypeId),
    DuplicateRelationPair {
        relation: TypeId,
        from: Box<TypeId>,
        to: Box<TypeId>,
    },
    UnknownNodeType(TypeId),
    UnknownRelationType(TypeId),
    DuplicateNode(NodeId),
    UnknownNode(NodeId),
    DuplicateEdge(Box<MetaEdge>),
    EmptySourceLocator(NodeId),
    InvalidByteRange {
        node: NodeId,
        range: ByteRange,
    },
    EmptyProvider(Box<MetaEdge>),
    RelationMismatch {
        relation: TypeId,
        expected_from: Box<TypeId>,
        expected_to: Box<TypeId>,
        actual_from: Box<TypeId>,
        actual_to: Box<TypeId>,
    },
    WrongGraph {
        expected: GraphId,
        actual: GraphId,
    },
    StaleRevision {
        expected: RevisionId,
        actual: RevisionId,
    },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId { field } => write!(f, "{field} must not be blank"),
            Self::InvalidType(ty) => write!(
                f,
                "invalid type {ty}: require nonempty dotted/colon namespace segments and a nonblank version"
            ),
            Self::DuplicateNodeType(ty) => write!(f, "duplicate node type {ty}"),
            Self::DuplicateRelationType(ty) => write!(f, "duplicate relation type {ty}"),
            Self::DuplicateRelationPair { relation, from, to } => {
                write!(f, "duplicate endpoint pair for {relation}: {from} -> {to}")
            }
            Self::UnknownNodeType(ty) => write!(f, "unknown node type {ty}"),
            Self::UnknownRelationType(ty) => write!(f, "unknown relation type {ty}"),
            Self::DuplicateNode(id) => write!(f, "duplicate node {id}"),
            Self::UnknownNode(id) => write!(f, "unknown node {id}"),
            Self::DuplicateEdge(edge) => write!(f, "duplicate edge {edge:?}"),
            Self::EmptySourceLocator(id) => write!(f, "node {id} has a blank source locator"),
            Self::InvalidByteRange { node, range } => write!(
                f,
                "node {node} has reversed byte range {}..{}",
                range.start, range.end
            ),
            Self::EmptyProvider(edge) => write!(f, "edge {edge:?} has a blank provenance provider"),
            Self::RelationMismatch {
                relation,
                expected_from,
                expected_to,
                actual_from,
                actual_to,
            } => write!(
                f,
                "relation {relation} does not accept {actual_from} -> {actual_to}; primary declared pair is {expected_from} -> {expected_to}"
            ),
            Self::WrongGraph { expected, actual } => {
                write!(f, "wrong graph: expected {expected}, got {actual}")
            }
            Self::StaleRevision { expected, actual } => {
                write!(f, "stale revision: expected {expected}, got {actual}")
            }
        }
    }
}
impl std::error::Error for Error {}

fn nonempty(value: &str, field: &'static str) -> Result<(), Error> {
    if value.trim().is_empty() {
        Err(Error::EmptyId { field })
    } else {
        Ok(())
    }
}

fn validate_type(ty: &TypeId) -> Result<(), Error> {
    if !ty.name.contains(['.', ':'])
        || ty.name.split(['.', ':']).any(|part| part.trim().is_empty())
        || ty.name.chars().any(char::is_whitespace)
        || ty.version.trim().is_empty()
    {
        Err(Error::InvalidType(ty.clone()))
    } else {
        Ok(())
    }
}
