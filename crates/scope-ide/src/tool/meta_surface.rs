use super::*;
use scope_meta::{MetaGraph, MetaNode, NodeRef, SourceRef, TypeId};

/// Explicit host authority. Implementations must return exact text for a requested
/// source revision, or fail; a locator alone must never grant access.
pub trait MetaSourceResolver {
    fn resolve(&self, node: &MetaNode, source: &SourceRef) -> Result<ResolvedMetaSource, String>;
}
pub struct ResolvedMetaSource {
    pub text: String,
    pub revision: String,
}

/// Explicit locator-to-path bindings inside an existing Cell-backed grant.
/// No locator parsing, ambient filesystem access, or historical disk fallback.
pub struct WorkspaceMetaSourceResolver {
    workspace: crate::workspace::Workspace,
    bindings: BTreeMap<String, crate::path::ScopePath>,
}
impl WorkspaceMetaSourceResolver {
    pub fn new(
        workspace: crate::workspace::Workspace,
        bindings: BTreeMap<String, crate::path::ScopePath>,
    ) -> Self {
        Self {
            workspace,
            bindings,
        }
    }
}
impl MetaSourceResolver for WorkspaceMetaSourceResolver {
    fn resolve(&self, _: &MetaNode, source: &SourceRef) -> Result<ResolvedMetaSource, String> {
        let path = self
            .bindings
            .get(&source.locator)
            .ok_or("source_unavailable: locator is not explicitly bound")?;
        let file = self
            .workspace
            .load_file_revision(path)
            .map_err(|e| format!("source_unavailable: {e}"))?;
        Ok(ResolvedMetaSource {
            text: file.contents,
            revision: file.revision,
        })
    }
}

pub(super) fn definitions(mut tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    for operation in ["find", "inspect", "related", "source"] {
        let mut tool = ToolDefinition::new(
            format!("scope.meta.{operation}"),
            "Read an explicitly installed Meta snapshot; locators confer no authority.",
        )
        .input(ToolInputDefinition::required(
            "provider",
            ToolInputKind::String,
            "Installed provider identity",
        ));
        if operation == "find" {
            tool = tool
                .input(ToolInputDefinition::required(
                    "kind",
                    ToolInputKind::Object,
                    "Exact {name, version} node type",
                ))
                .input(ToolInputDefinition::optional(
                    "label",
                    ToolInputKind::String,
                    "Case-sensitive label substring",
                ))
                .input(ToolInputDefinition::optional(
                    "revision",
                    ToolInputKind::String,
                    "Expected graph revision for subsequent pages",
                ));
        } else {
            tool = tool.input(ToolInputDefinition::required(
                "node",
                ToolInputKind::Object,
                "NodeRef {graph, revision, node}",
            ));
        }
        if operation == "related" {
            tool = tool.input(ToolInputDefinition::optional(
                "relation",
                ToolInputKind::Object,
                "Exact {name, version} relation type",
            ));
        }
        if operation != "inspect" {
            tool = tool
                .input(ToolInputDefinition::optional(
                    "offset",
                    ToolInputKind::Integer,
                    "Zero-based item or UTF-8 byte offset",
                ))
                .input(ToolInputDefinition::optional(
                    "limit",
                    ToolInputKind::Integer,
                    "1..100 items; source 1..16384 bytes",
                ));
        }
        tools.push(tool);
    }
    tools
}

impl MetaDiscoveryProvider {
    pub fn register_source_resolver(
        &mut self,
        provider: &str,
        resolver: impl MetaSourceResolver + 'static,
    ) -> Result<(), ToolError> {
        if !self.providers.iter().any(|(d, _)| d.id == provider) {
            return Err(ToolError::execution(
                "scope.meta.source",
                "unknown_provider",
            ));
        }
        self.resolvers.insert(provider.into(), Box::new(resolver));
        Ok(())
    }
    /// Host-only invalidation; no tool can refresh or replace snapshots.
    pub fn invalidate_provider(&mut self, provider: &str) -> Result<(), ToolError> {
        if !self.providers.iter().any(|(d, _)| d.id == provider) {
            return Err(ToolError::execution("scope.meta", "unknown_provider"));
        }
        self.invalidated.insert(provider.into());
        Ok(())
    }
    /// Replacements must change graph identity or revision, even after invalidation.
    /// Source authority is removed and must be explicitly registered again.
    pub fn replace_provider(
        &mut self,
        provider: Box<dyn scope_meta::MetaProvider>,
    ) -> Result<(), ToolError> {
        let description = provider.description();
        let entry = self
            .providers
            .iter_mut()
            .find(|(d, _)| d.id == description.id)
            .ok_or_else(|| ToolError::execution("scope.meta", "unknown_provider"))?;
        let new = provider
            .snapshot()
            .map_err(|e| ToolError::execution("scope.meta", e.to_string()))?;
        if let Ok(old) = entry.1.snapshot()
            && old.graph.graph_id() == new.graph.graph_id()
            && old.graph.revision() == new.graph.revision()
        {
            return Err(ToolError::execution(
                "scope.meta",
                "replacement_requires_new_revision",
            ));
        }
        self.resolvers.remove(&description.id);
        self.invalidated.remove(&description.id);
        *entry = (description, provider);
        Ok(())
    }
    pub(super) fn invoke_meta(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let op = call.name.as_str();
        let allowed: &[&str] = match op {
            "scope.meta.find" => &["provider", "kind", "label", "revision", "offset", "limit"],
            "scope.meta.inspect" => &["provider", "node"],
            "scope.meta.related" => &["provider", "node", "relation", "offset", "limit"],
            "scope.meta.source" => &["provider", "node", "offset", "limit"],
            _ => return Err(ToolError::unknown_tool(op)),
        };
        reject_unexpected_arguments(&call, allowed)?;
        let id = string(&call, "provider")?;
        let (_, provider) = self
            .providers
            .iter()
            .find(|(d, _)| d.id == id)
            .ok_or_else(|| ToolError::execution(op, "unknown_provider"))?;
        if self.invalidated.contains(id) {
            return Err(ToolError::execution(
                op,
                "snapshot_unavailable: invalidated",
            ));
        }
        let snapshot = provider
            .snapshot()
            .map_err(|e| ToolError::execution(op, format!("snapshot_unavailable: {e}")))?;
        let graph = snapshot.graph;
        let mut fields = vec![
            ("provider", id.into()),
            ("graph", graph.graph_id().as_str().into()),
            ("revision", graph.revision().as_str().into()),
        ];
        let completeness = match snapshot.completeness {
            scope_meta::SnapshotCompleteness::Complete => {
                ToolValue::object([("state", "complete".into())])
            }
            scope_meta::SnapshotCompleteness::Incomplete { warnings } => ToolValue::object([
                ("state", "incomplete".into()),
                ("warning_count", warnings.len().into()),
                (
                    "warnings",
                    ToolValue::array(warnings.iter().take(100).map(|w| {
                        ToolValue::object([
                            ("code", w.code.clone().into()),
                            ("message", w.message.clone().into()),
                            (
                                "locator",
                                w.locator.clone().map(Into::into).unwrap_or(ToolValue::Null),
                            ),
                        ])
                    })),
                ),
            ]),
        };
        fields.push(("completeness", completeness));
        let offset = number(&call, "offset", 0, usize::MAX)?;
        let source_op = op == "scope.meta.source";
        let limit = number(
            &call,
            "limit",
            if source_op { 4096 } else { 50 },
            if source_op { 16384 } else { 100 },
        )?;
        if limit == 0 {
            return Err(invalid(&call, "limit", "must be positive"));
        }
        let node = if op != "scope.meta.find" {
            let obj = object(&call, "node", &["graph", "revision", "node"])?;
            let reference = NodeRef {
                graph: scope_meta::GraphId::new(member(&call, obj, "graph")?),
                revision: scope_meta::RevisionId::new(member(&call, obj, "revision")?),
                node: scope_meta::NodeId::new(member(&call, obj, "node")?),
            };
            Some(
                graph
                    .resolve(&reference)
                    .map_err(|e| query_error(&call, graph, e))?,
            )
        } else {
            None
        };
        match op {
            "scope.meta.find" => {
                if let Some(value) = call.arguments.get("revision")
                    && value
                        .as_str()
                        .ok_or_else(|| invalid(&call, "revision", "expected string"))?
                        != graph.revision().as_str()
                {
                    return Err(conflict(&call, graph, "stale_revision"));
                }
                let kind = kind(&call, "kind")?;
                if !graph.registry().contains_node(&kind) {
                    return Err(invalid(&call, "kind", "unknown_node_type"));
                }
                let label = call
                    .arguments
                    .get("label")
                    .map(|v| {
                        v.as_str()
                            .ok_or_else(|| invalid(&call, "label", "expected string"))
                    })
                    .transpose()?;
                let nodes = graph
                    .nodes()
                    .iter()
                    .filter(|n| n.kind == kind && label.is_none_or(|s| n.label.contains(s)));
                page(&call, &mut fields, nodes.clone().count(), offset, limit)?;
                fields.push((
                    "nodes",
                    ToolValue::array(nodes.skip(offset).take(limit).map(|n| node_value(graph, n))),
                ));
            }
            "scope.meta.inspect" => {
                fields.push(("node", node_value(graph, node.unwrap())));
            }
            "scope.meta.related" => {
                let node = node.unwrap();
                let relation = if call.arguments.contains_key("relation") {
                    Some(kind(&call, "relation")?)
                } else {
                    None
                };
                if relation
                    .as_ref()
                    .is_some_and(|r| graph.registry().relation(r).is_none())
                {
                    return Err(invalid(&call, "relation", "unknown_relation_type"));
                }
                let edges = graph.edges().iter().filter(|e| {
                    e.from == node.id && relation.as_ref().is_none_or(|r| &e.relation == r)
                });
                page(&call, &mut fields, edges.clone().count(), offset, limit)?;
                fields.push((
                    "edges",
                    ToolValue::array(edges.skip(offset).take(limit).map(|e| {
                        ToolValue::object([
                            ("from", reference_value(graph, &e.from)),
                            ("to", reference_value(graph, &e.to)),
                            ("relation", meta_type_value(&e.relation)),
                            (
                                "provenance",
                                ToolValue::object([
                                    (
                                        "kind",
                                        match e.provenance {
                                            scope_meta::Provenance::Observed { .. } => "observed",
                                            scope_meta::Provenance::Inferred { .. } => "inferred",
                                            scope_meta::Provenance::Proposed { .. } => "proposed",
                                        }
                                        .into(),
                                    ),
                                    ("provider", e.provenance.provider().into()),
                                ]),
                            ),
                        ])
                    })),
                ));
            }
            _ => {
                let node = node.unwrap();
                let source = node.source.as_ref().ok_or_else(|| {
                    ToolError::execution(op, "source_unavailable: no source metadata")
                })?;
                let resolver = self.resolvers.get(id).ok_or_else(|| {
                    ToolError::execution(op, "source_unavailable: no registered authority")
                })?;
                let resolved = resolver
                    .resolve(node, source)
                    .map_err(|e| ToolError::execution(op, e))?;
                if source
                    .revision
                    .as_ref()
                    .is_some_and(|r| r != &resolved.revision)
                {
                    return Err(conflict(&call, graph, "source_revision_mismatch"));
                }
                let start = source.span.map_or(0, |s| s.start as usize);
                let end = source.span.map_or(resolved.text.len(), |s| s.end as usize);
                let text = resolved
                    .text
                    .get(start..end)
                    .ok_or_else(|| ToolError::execution(op, "invalid_source_span"))?;
                if offset > text.len() || !text.is_char_boundary(offset) {
                    return Err(invalid(&call, "offset", "invalid UTF-8 byte offset"));
                }
                let mut stop = offset.saturating_add(limit).min(text.len());
                while !text.is_char_boundary(stop) {
                    stop -= 1;
                }
                if stop == offset && offset < text.len() {
                    return Err(invalid(
                        &call,
                        "limit",
                        "too small for next UTF-8 character",
                    ));
                }
                fields.extend([
                    ("node", reference_value(graph, &node.id)),
                    ("source_revision", resolved.revision.into()),
                    ("text", text[offset..stop].into()),
                    ("total_bytes", text.len().into()),
                    ("offset", offset.into()),
                    (
                        "next_offset",
                        if stop < text.len() {
                            stop.into()
                        } else {
                            ToolValue::Null
                        },
                    ),
                ]);
            }
        }
        let value = ToolValue::object(fields);
        if value_size(&value) > 65536 {
            return Err(ToolError::execution(
                op,
                "output_limit: reduce page size; metadata may be oversized",
            ));
        }
        Ok(ToolResult::new(value))
    }
}
fn invalid(call: &ToolCall, argument: &str, message: &str) -> ToolError {
    ToolError::InvalidArgument {
        tool: call.name.clone(),
        argument: argument.into(),
        message: message.into(),
    }
}
fn string<'a>(call: &'a ToolCall, name: &str) -> Result<&'a str, ToolError> {
    call.arguments
        .get(name)
        .ok_or_else(|| ToolError::MissingArgument {
            tool: call.name.clone(),
            argument: name.into(),
        })?
        .as_str()
        .ok_or_else(|| invalid(call, name, "expected string"))
}
fn object<'a>(
    call: &'a ToolCall,
    name: &str,
    keys: &[&str],
) -> Result<&'a BTreeMap<String, ToolValue>, ToolError> {
    let Some(ToolValue::Object(obj)) = call.arguments.get(name) else {
        return Err(invalid(call, name, "expected object"));
    };
    if obj.len() != keys.len() || keys.iter().any(|k| !obj.contains_key(*k)) {
        return Err(invalid(call, name, "incorrect object fields"));
    }
    Ok(obj)
}
fn member<'a>(
    call: &ToolCall,
    obj: &'a BTreeMap<String, ToolValue>,
    key: &str,
) -> Result<&'a str, ToolError> {
    obj[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid(call, key, "expected nonempty string"))
}
fn kind(call: &ToolCall, key: &str) -> Result<TypeId, ToolError> {
    let obj = object(call, key, &["name", "version"])?;
    Ok(TypeId::new(
        member(call, obj, "name")?,
        member(call, obj, "version")?,
    ))
}
fn number(call: &ToolCall, key: &str, default: usize, max: usize) -> Result<usize, ToolError> {
    match call.arguments.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_i64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n <= max)
            .ok_or_else(|| invalid(call, key, "integer out of range")),
    }
}
fn conflict(call: &ToolCall, graph: &MetaGraph, message: &str) -> ToolError {
    ToolError::Conflict {
        tool: call.name.clone(),
        path: graph.graph_id().to_string(),
        current_revision: Some(graph.revision().to_string()),
        message: message.into(),
    }
}
fn query_error(call: &ToolCall, graph: &MetaGraph, error: scope_meta::Error) -> ToolError {
    match error {
        scope_meta::Error::StaleRevision { .. } => conflict(call, graph, "stale_revision"),
        _ => invalid(call, "node", &error.to_string()),
    }
}
fn reference_value(graph: &MetaGraph, id: &scope_meta::NodeId) -> ToolValue {
    ToolValue::object([
        ("graph", graph.graph_id().as_str().into()),
        ("revision", graph.revision().as_str().into()),
        ("node", id.as_str().into()),
    ])
}
fn node_value(graph: &MetaGraph, node: &MetaNode) -> ToolValue {
    ToolValue::object([
        ("ref", reference_value(graph, &node.id)),
        ("kind", meta_type_value(&node.kind)),
        ("label", node.label.clone().into()),
        (
            "source",
            node.source
                .as_ref()
                .map(|s| {
                    ToolValue::object([
                        ("locator", s.locator.clone().into()),
                        (
                            "revision",
                            s.revision
                                .clone()
                                .map(Into::into)
                                .unwrap_or(ToolValue::Null),
                        ),
                        (
                            "span",
                            s.span
                                .map(|r| {
                                    ToolValue::object([
                                        ("start", r.start.into()),
                                        ("end", r.end.into()),
                                    ])
                                })
                                .unwrap_or(ToolValue::Null),
                        ),
                    ])
                })
                .unwrap_or(ToolValue::Null),
        ),
    ])
}
fn page(
    call: &ToolCall,
    fields: &mut Vec<(&str, ToolValue)>,
    total: usize,
    offset: usize,
    limit: usize,
) -> Result<(), ToolError> {
    if offset > total {
        return Err(invalid(call, "offset", "beyond result count"));
    }
    fields.extend([
        ("total", total.into()),
        ("offset", offset.into()),
        (
            "next_offset",
            if total - offset > limit {
                (offset + limit).into()
            } else {
                ToolValue::Null
            },
        ),
    ]);
    Ok(())
}
// Conservative JSON-size upper bound, including worst-case string escaping.
fn value_size(value: &ToolValue) -> usize {
    match value {
        ToolValue::String(s) => s.len().saturating_mul(6).saturating_add(2),
        ToolValue::Array(a) => a.iter().fold(2usize, |n, v| {
            n.saturating_add(value_size(v)).saturating_add(1)
        }),
        ToolValue::Object(o) => o.iter().fold(2usize, |n, (k, v)| {
            n.saturating_add(k.len().saturating_mul(6))
                .saturating_add(value_size(v))
                .saturating_add(4)
        }),
        _ => 24,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{LanguageDocument, LanguageMeta, LanguagePath, SimpleLanguage};
    fn outline(revision: &str) -> LanguageMeta {
        LanguageMeta::new(
            scope_meta::GraphId::new("g"),
            scope_meta::RevisionId::new(revision),
            "opaque",
            "text-1",
            LanguageDocument::new(LanguagePath::new("a.scope"), "fn main() {}\n", 1),
            &SimpleLanguage,
        )
        .unwrap()
    }
    fn call(op: &str) -> ToolCall {
        ToolCall::new(format!("scope.meta.{op}")).with_argument("provider", "scope.simple".into())
    }
    fn reference(revision: &str) -> ToolValue {
        ToolValue::object([
            ("graph", "g".into()),
            ("revision", revision.into()),
            ("node", "document".into()),
        ])
    }
    struct Fixture {
        graph: MetaGraph,
        completeness: scope_meta::SnapshotCompleteness,
    }
    impl scope_meta::MetaProvider for Fixture {
        fn description(&self) -> scope_meta::ProviderDescription {
            scope_meta::ProviderDescription::new("scope.simple", "pagination fixture")
        }
        fn snapshot(&self) -> Result<scope_meta::MetaSnapshot<'_>, scope_meta::ProviderError> {
            Ok(scope_meta::MetaSnapshot::new(
                &self.graph,
                self.completeness.clone(),
            ))
        }
    }
    fn fixture(label: &str, provenance: &str, incomplete: bool) -> MetaDiscoveryProvider {
        use scope_meta::{
            GraphId, MetaEdge, NodeId, Provenance, RevisionId, SnapshotCompleteness,
            SnapshotWarning, TypeRegistry,
        };
        let kind = TypeId::new("test.node", "1");
        let relation = TypeId::new("test.edge", "1");
        let mut registry = TypeRegistry::new();
        registry.register_node(kind.clone()).unwrap();
        registry
            .register_relation(relation.clone(), kind.clone(), kind.clone())
            .unwrap();
        let nodes = ["document", "a", "b", "c"]
            .into_iter()
            .map(|id| MetaNode {
                id: NodeId::new(id),
                kind: kind.clone(),
                label: if id == "document" {
                    "root".into()
                } else {
                    label.into()
                },
                source: None,
            })
            .collect();
        let mut edges: Vec<_> = ["a", "b", "c"]
            .into_iter()
            .map(|id| MetaEdge {
                from: NodeId::new("document"),
                relation: relation.clone(),
                to: NodeId::new(id),
                provenance: Provenance::Observed {
                    provider: provenance.into(),
                },
            })
            .collect();
        // Incoming edge must not appear in outgoing related pages.
        edges.push(MetaEdge {
            from: NodeId::new("a"),
            relation,
            to: NodeId::new("document"),
            provenance: Provenance::Observed {
                provider: "fixture".into(),
            },
        });
        let graph = MetaGraph::new(
            GraphId::new("g"),
            RevisionId::new("r"),
            registry,
            nodes,
            edges,
        )
        .unwrap();
        let completeness = if incomplete {
            SnapshotCompleteness::Incomplete {
                warnings: (0..105)
                    .map(|i| {
                        SnapshotWarning::new(format!("warning:{i}"), "partial")
                            .with_locator("opaque")
                    })
                    .collect(),
            }
        } else {
            SnapshotCompleteness::Complete
        };
        MetaDiscoveryProvider::new(vec![Box::new(Fixture {
            graph,
            completeness,
        })])
        .unwrap()
    }
    fn find_call() -> ToolCall {
        call("find")
            .with_argument("kind", meta_type_value(&TypeId::new("test.node", "1")))
            .with_argument("label", "match".into())
    }
    fn related_call() -> ToolCall {
        call("related")
            .with_argument("node", reference("r"))
            .with_argument("relation", meta_type_value(&TypeId::new("test.edge", "1")))
    }
    fn result_object(result: Result<ToolResult, ToolError>) -> BTreeMap<String, ToolValue> {
        let ToolValue::Object(value) = result.unwrap().value else {
            panic!("expected object")
        };
        value
    }
    #[test]
    fn find_pages_preserve_order_totals_and_boundaries() {
        let mut p = fixture("match", "fixture", false);
        let first = result_object(p.invoke(find_call().with_argument("limit", 2usize.into())));
        assert_eq!(first["total"], 3usize.into());
        assert_eq!(first["next_offset"], 2usize.into());
        let ToolValue::Array(nodes) = &first["nodes"] else {
            panic!("nodes")
        };
        assert_eq!(nodes.len(), 2);
        for (node, expected) in nodes.iter().zip(["a", "b"]) {
            let ToolValue::Object(node) = node else {
                panic!("node")
            };
            assert_eq!(
                node["ref"],
                reference_value(
                    p.providers[0].1.snapshot().unwrap().graph,
                    &scope_meta::NodeId::new(expected)
                )
            );
        }
        let last = result_object(
            p.invoke(
                find_call()
                    .with_argument("revision", "r".into())
                    .with_argument("offset", 2usize.into())
                    .with_argument("limit", 2usize.into()),
            ),
        );
        assert_eq!(last["total"], 3usize.into());
        assert_eq!(last["next_offset"], ToolValue::Null);
        let ToolValue::Array(nodes) = &last["nodes"] else {
            panic!("nodes")
        };
        assert_eq!(nodes.len(), 1);
        let empty = result_object(p.invoke(find_call().with_argument("offset", 3usize.into())));
        assert_eq!(empty["nodes"], ToolValue::Array(vec![]));
        assert!(matches!(
            p.invoke(find_call().with_argument("offset", 4usize.into())),
            Err(ToolError::InvalidArgument { .. })
        ));
    }
    #[test]
    fn related_pages_are_outgoing_only_with_revisioned_endpoints() {
        let mut p = fixture("match", "fixture", false);
        for (offset, expected, next) in [
            (0usize, "a", Some(1usize)),
            (1, "b", Some(2)),
            (2, "c", None),
        ] {
            let page = result_object(
                p.invoke(
                    related_call()
                        .with_argument("offset", offset.into())
                        .with_argument("limit", 1usize.into()),
                ),
            );
            assert_eq!(page["total"], 3usize.into());
            assert_eq!(
                page["next_offset"],
                next.map(Into::into).unwrap_or(ToolValue::Null)
            );
            let ToolValue::Array(edges) = &page["edges"] else {
                panic!("edges")
            };
            assert_eq!(edges.len(), 1);
            let ToolValue::Object(edge) = &edges[0] else {
                panic!("edge")
            };
            assert_eq!(edge["from"], reference("r"));
            assert_eq!(
                edge["to"],
                ToolValue::object([
                    ("graph", "g".into()),
                    ("revision", "r".into()),
                    ("node", expected.into())
                ])
            );
            assert_eq!(
                edge["provenance"],
                ToolValue::object([("kind", "observed".into()), ("provider", "fixture".into())])
            );
        }
        let empty = result_object(p.invoke(related_call().with_argument("offset", 3usize.into())));
        assert_eq!(empty["edges"], ToolValue::Array(vec![]));
        assert!(matches!(
            p.invoke(related_call().with_argument("offset", 4usize.into())),
            Err(ToolError::InvalidArgument { .. })
        ));
    }
    #[test]
    fn find_and_related_reject_stale_page_revisions() {
        let mut p = fixture("match", "fixture", false);
        for call in [
            find_call().with_argument("revision", "old".into()),
            related_call().with_argument("node", reference("old")),
        ] {
            assert!(
                matches!(p.invoke(call.with_argument("offset", 1usize.into())), Err(ToolError::Conflict { current_revision: Some(revision), message, .. }) if revision == "r" && message == "stale_revision")
            );
        }
    }
    #[test]
    fn find_and_related_reject_unknown_types_and_providers() {
        let mut p = fixture("match", "fixture", false);
        for (call, argument) in [
            (
                find_call().with_argument("kind", meta_type_value(&TypeId::new("unknown", "1"))),
                "kind",
            ),
            (
                related_call()
                    .with_argument("relation", meta_type_value(&TypeId::new("unknown", "1"))),
                "relation",
            ),
        ] {
            assert!(
                matches!(p.invoke(call), Err(ToolError::InvalidArgument { argument: actual, .. }) if actual == argument)
            );
        }
        for call in [find_call(), related_call()] {
            assert!(
                matches!(p.invoke(call.with_argument("provider", "unknown".into())), Err(ToolError::Execution { message, .. }) if message == "unknown_provider")
            );
        }
    }
    #[test]
    fn find_rejects_oversized_output() {
        let mut p = fixture(&"match".repeat(3000), "fixture", false);
        assert!(
            matches!(p.invoke(find_call().with_argument("limit", 1usize.into())), Err(ToolError::Execution { message, .. }) if message.starts_with("output_limit:"))
        );
    }
    #[test]
    fn related_rejects_oversized_output() {
        let mut p = fixture("match", &"producer".repeat(2000), false);
        assert!(
            matches!(p.invoke(related_call().with_argument("limit", 1usize.into())), Err(ToolError::Execution { message, .. }) if message.starts_with("output_limit:"))
        );
    }
    #[test]
    fn incomplete_snapshot_metadata_is_independent_of_pagination() {
        let mut p = fixture("match", "fixture", true);
        for call in [find_call(), related_call()] {
            let page = result_object(p.invoke(call.with_argument("limit", 1usize.into())));
            assert_eq!(page["total"], 3usize.into());
            assert_eq!(page["next_offset"], 1usize.into());
            let ToolValue::Object(completeness) = &page["completeness"] else {
                panic!("completeness")
            };
            assert_eq!(completeness["state"], "incomplete".into());
            assert_eq!(completeness["warning_count"], 105usize.into());
            let ToolValue::Array(warnings) = &completeness["warnings"] else {
                panic!("warnings")
            };
            assert_eq!(warnings.len(), 100);
        }
    }
    struct Exact;
    impl MetaSourceResolver for Exact {
        fn resolve(&self, _: &MetaNode, _: &SourceRef) -> Result<ResolvedMetaSource, String> {
            Ok(ResolvedMetaSource {
                text: "fn main() {}\n".into(),
                revision: "text-1".into(),
            })
        }
    }
    #[test]
    fn navigation_authority_and_replacement() {
        let mut p = MetaDiscoveryProvider::new(vec![Box::new(outline("r1"))]).unwrap();
        let source = call("source").with_argument("node", reference("r1"));
        assert!(matches!(
            p.invoke(source.clone()),
            Err(ToolError::Execution { .. })
        ));
        p.register_source_resolver("scope.simple", Exact).unwrap();
        assert!(p.invoke(source.clone()).is_ok());
        let find = call("find")
            .with_argument(
                "kind",
                meta_type_value(&TypeId::new("scope.language.function", "1")),
            )
            .with_argument("limit", 1usize.into());
        assert!(p.invoke(find.clone()).is_ok());
        assert!(matches!(
            p.invoke(find.with_argument("limit", 101usize.into())),
            Err(ToolError::InvalidArgument { .. })
        ));
        assert!(
            p.invoke(call("related").with_argument("node", reference("r1")))
                .is_ok()
        );
        assert!(p.replace_provider(Box::new(outline("r1"))).is_err());
        p.replace_provider(Box::new(outline("r2"))).unwrap();
        assert!(matches!(p.invoke(source), Err(ToolError::Conflict { .. })));
        assert!(matches!(
            p.invoke(call("source").with_argument("node", reference("r2"))),
            Err(ToolError::Execution { .. })
        ));
        p.invalidate_provider("scope.simple").unwrap();
        assert!(matches!(
            p.invoke(call("inspect").with_argument("node", reference("r2"))),
            Err(ToolError::Execution { .. })
        ));
    }
    #[test]
    fn source_revision_and_page_bounds() {
        struct WrongRevision;
        impl MetaSourceResolver for WrongRevision {
            fn resolve(&self, _: &MetaNode, _: &SourceRef) -> Result<ResolvedMetaSource, String> {
                Ok(ResolvedMetaSource {
                    text: "fn main() {}\n".into(),
                    revision: "wrong".into(),
                })
            }
        }
        let mut p = MetaDiscoveryProvider::new(vec![Box::new(outline("r"))]).unwrap();
        let source = call("source").with_argument("node", reference("r"));
        p.register_source_resolver("scope.simple", WrongRevision)
            .unwrap();
        assert!(matches!(
            p.invoke(source.clone()),
            Err(ToolError::Conflict { .. })
        ));
        p.register_source_resolver("scope.simple", Exact).unwrap();
        let result = p
            .invoke(source.clone().with_argument("limit", 2usize.into()))
            .unwrap();
        let ToolValue::Object(value) = result.value else {
            panic!("object")
        };
        assert_eq!(value["text"].as_str(), Some("fn"));
        assert_eq!(value["next_offset"].as_i64(), Some(2));
        assert!(matches!(
            p.invoke(source.with_argument("offset", 999usize.into())),
            Err(ToolError::InvalidArgument { .. })
        ));
        let find = call("find").with_argument(
            "kind",
            meta_type_value(&TypeId::new("scope.language.document", "1")),
        );
        assert!(matches!(
            p.invoke(find.clone().with_argument("revision", "old".into())),
            Err(ToolError::Conflict { .. })
        ));
        let result = p
            .invoke(find.with_argument("offset", 1usize.into()))
            .unwrap();
        let ToolValue::Object(value) = result.value else {
            panic!("object")
        };
        assert_eq!(value["nodes"], ToolValue::Array(vec![]));
    }
    #[test]
    fn strict_arguments_and_wrong_graph() {
        let mut p = MetaDiscoveryProvider::new(vec![Box::new(outline("r"))]).unwrap();
        assert!(matches!(
            p.invoke(call("inspect").with_argument(
                "node",
                ToolValue::object([
                    ("graph", "wrong".into()),
                    ("revision", "r".into()),
                    ("node", "document".into())
                ])
            )),
            Err(ToolError::InvalidArgument { .. })
        ));
        assert!(matches!(
            p.invoke(
                call("find").with_argument("kind", meta_type_value(&TypeId::new("unknown", "1")))
            ),
            Err(ToolError::InvalidArgument { .. })
        ));
    }
}
