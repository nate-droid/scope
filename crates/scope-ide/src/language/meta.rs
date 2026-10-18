//! Immutable metadata from one explicitly supplied language-service outline.
//! Completeness covers only that outline, not all semantics of the document.

use super::{LanguageDocument, LanguageService};
use scope_language::LineIndex;
use scope_meta::{
    ByteRange, GraphId, MetaEdge, MetaGraph, MetaNode, MetaProvider, MetaSnapshot, NodeId,
    Provenance, ProviderDescription, ProviderError, ProviderErrorKind, RevisionId, SourceRef,
    TypeId, TypeRegistry,
};

/// Owns a validated graph; retains neither the service nor the source text.
/// Node identities are snapshot-local, not stable symbol identities across edits.
#[derive(Debug)]
pub struct LanguageMeta {
    graph: MetaGraph,
    description: ProviderDescription,
}

impl LanguageMeta {
    /// Calls `service.outline` exactly once against the supplied source snapshot.
    /// Graph and content revisions are independent caller-assigned identities.
    /// Invalid service ranges reject the entire snapshot, never clamp or omit symbols.
    pub fn new(
        graph_id: GraphId,
        revision: RevisionId,
        source_locator: impl Into<String>,
        content_revision: impl Into<String>,
        document: LanguageDocument<'_>,
        service: &dyn LanguageService,
    ) -> Result<Self, ProviderError> {
        let locator = source_locator.into();
        let content_revision = content_revision.into();
        let description = ProviderDescription::new(
            service.language_id(),
            format!("{} document outline", service.language_name()),
        );
        let index = LineIndex::new(document.text());
        let symbols = service.outline(document.clone());
        let mut registry = TypeRegistry::new();
        registry
            .register_node(kind("document"))
            .map_err(invalid_graph)?;
        let mut nodes = vec![MetaNode {
            id: NodeId::new("document"),
            kind: kind("document"),
            label: document.path().as_str().into(),
            source: Some(SourceRef {
                locator: locator.clone(),
                span: Some(ByteRange {
                    start: 0,
                    end: document.text().len() as u64,
                }),
                revision: Some(content_revision.clone()),
            }),
        }];
        let mut edges = Vec::new();
        for (ordinal, symbol) in symbols.into_iter().enumerate() {
            let span = index.span_for_range(symbol.range).ok_or_else(|| {
                let error = InvalidOutlineRange {
                    ordinal,
                    range: symbol.range,
                };
                ProviderError::with_source(
                    ProviderErrorKind::InvalidSnapshot,
                    error.to_string(),
                    error,
                )
            })?;
            let symbol_kind = kind(&symbol.kind.to_string().replace(' ', "_"));
            if !registry.contains_node(&symbol_kind) {
                registry
                    .register_node(symbol_kind.clone())
                    .map_err(invalid_graph)?;
                if registry.relation(&kind("declares")).is_none() {
                    registry
                        .register_relation(kind("declares"), kind("document"), symbol_kind.clone())
                        .map_err(invalid_graph)?;
                } else {
                    registry
                        .register_relation_pair(
                            kind("declares"),
                            kind("document"),
                            symbol_kind.clone(),
                        )
                        .map_err(invalid_graph)?;
                }
            }
            // Ordinal distinguishes even identical duplicate entries from the service.
            let id = NodeId::new(format!("symbol:{ordinal}:{}:{}", span.start, span.end));
            nodes.push(MetaNode {
                id: id.clone(),
                kind: symbol_kind,
                label: symbol.name,
                source: Some(SourceRef {
                    locator: locator.clone(),
                    span: Some(ByteRange {
                        start: span.start as u64,
                        end: span.end as u64,
                    }),
                    revision: Some(content_revision.clone()),
                }),
            });
            edges.push(MetaEdge {
                from: NodeId::new("document"),
                relation: kind("declares"),
                to: id,
                provenance: Provenance::Observed {
                    provider: service.language_id().into(),
                },
            });
        }
        let graph =
            MetaGraph::new(graph_id, revision, registry, nodes, edges).map_err(invalid_graph)?;
        Ok(Self { graph, description })
    }

    pub fn graph(&self) -> &MetaGraph {
        &self.graph
    }
}

impl MetaProvider for LanguageMeta {
    fn description(&self) -> ProviderDescription {
        self.description.clone()
    }

    fn snapshot(&self) -> Result<MetaSnapshot<'_>, ProviderError> {
        Ok(MetaSnapshot::complete(&self.graph))
    }
}

fn kind(name: &str) -> TypeId {
    TypeId::new(format!("scope.language.{name}"), "1")
}

fn invalid_graph(error: scope_meta::Error) -> ProviderError {
    ProviderError::with_source(ProviderErrorKind::InvalidSnapshot, error.to_string(), error)
}

#[derive(Debug)]
struct InvalidOutlineRange {
    ordinal: usize,
    range: super::TextRange,
}

impl std::fmt::Display for InvalidOutlineRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "outline symbol {} has an invalid source range {:?}",
            self.ordinal, self.range
        )
    }
}
impl std::error::Error for InvalidOutlineRange {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{
        LanguageAnalysis, LanguagePath, Position, Symbol, SymbolKind, TextRange,
    };
    use scope_meta::SnapshotCompleteness;
    use std::error::Error;

    struct Custom(Vec<TextRange>);
    impl LanguageService for Custom {
        fn language_id(&self) -> &'static str {
            "example.custom"
        }
        fn language_name(&self) -> &'static str {
            "Custom"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            false
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            panic!("must call outline override")
        }
        fn outline(&self, _: LanguageDocument<'_>) -> Vec<Symbol> {
            self.0
                .iter()
                .map(|&range| Symbol {
                    name: "same".into(),
                    kind: SymbolKind::Function,
                    range,
                })
                .collect()
        }
    }
    fn build(service: &dyn LanguageService) -> Result<LanguageMeta, ProviderError> {
        LanguageMeta::new(
            GraphId::new("graph"),
            RevisionId::new("graph-rev"),
            "logical/source",
            "content-rev",
            LanguageDocument::new(LanguagePath::new("custom.any"), "é\n🦀", 99),
            service,
        )
    }
    #[test]
    fn custom_outline_duplicates_unicode_and_common_interface() {
        let range = TextRange::new(Position::new(0, 0), Position::new(1, 4));
        let meta = build(&Custom(vec![range, range])).unwrap();
        let provider: &dyn MetaProvider = &meta;
        assert_eq!(provider.description().id, "example.custom");
        let snapshot = provider.snapshot().unwrap();
        assert!(std::ptr::eq(snapshot.graph, meta.graph()));
        assert_eq!(snapshot.completeness, SnapshotCompleteness::Complete);
        let symbols = snapshot.graph.find(&kind("function"), None).unwrap();
        assert_eq!(symbols.len(), 2);
        assert_ne!(symbols[0].id, symbols[1].id);
        for symbol in symbols {
            let source = symbol.source.as_ref().unwrap();
            assert_eq!(source.span, Some(ByteRange { start: 0, end: 7 }));
            assert_eq!(source.locator, "logical/source");
            assert_eq!(source.revision.as_deref(), Some("content-rev"));
            let incoming = snapshot.graph.describe(&symbol.id).unwrap().incoming;
            assert_eq!(incoming.len(), 1);
            assert_eq!(incoming[0].relation, kind("declares"));
            assert_eq!(incoming[0].provenance.provider(), "example.custom");
        }
    }
    #[test]
    fn rejects_invalid_ranges_without_clamping() {
        for (start, end) in [
            ((0, 1), (0, 2)),
            ((1, 0), (1, 1)),
            ((0, 0), (2, 0)),
            ((0, 0), (0, 3)),
            ((1, 4), (0, 0)),
            ((0, 0), (1, usize::MAX)),
        ] {
            let range =
                TextRange::new(Position::new(start.0, start.1), Position::new(end.0, end.1));
            let error = build(&Custom(vec![range])).unwrap_err();
            assert_eq!(error.kind, ProviderErrorKind::InvalidSnapshot);
            assert!(error.source().is_some());
        }
    }
    #[test]
    fn empty_outline_is_complete_and_graph_identity_is_validated() {
        assert!(
            build(&Custom(vec![]))
                .unwrap()
                .graph()
                .get(&NodeId::new("document"))
                .is_ok()
        );
        let error = LanguageMeta::new(
            GraphId::new(""),
            RevisionId::new("r"),
            "source",
            "c",
            LanguageDocument::new(LanguagePath::new("empty"), "", 0),
            &Custom(vec![]),
        )
        .unwrap_err();
        assert_eq!(error.kind, ProviderErrorKind::InvalidSnapshot);
        assert!(error.source().is_some());
    }
}
