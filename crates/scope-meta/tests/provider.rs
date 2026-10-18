use scope_meta::*;
use std::error::Error as _;

struct Provider {
    graph: MetaGraph,
    warnings: Vec<SnapshotWarning>,
}

impl MetaProvider for Provider {
    fn description(&self) -> ProviderDescription {
        ProviderDescription::new("test.provider", "In-memory test graph")
    }

    fn snapshot(&self) -> Result<MetaSnapshot<'_>, ProviderError> {
        Ok(if self.warnings.is_empty() {
            MetaSnapshot::complete(&self.graph)
        } else {
            MetaSnapshot::incomplete(&self.graph, self.warnings.clone())
        })
    }
}

fn provider() -> Provider {
    Provider {
        graph: MetaGraph::new(
            "test".into(),
            "1".into(),
            TypeRegistry::new(),
            vec![],
            vec![],
        )
        .unwrap(),
        warnings: vec![],
    }
}

#[test]
fn trait_object_borrows_existing_graph_repeatedly() {
    let provider = provider();
    let object: &dyn MetaProvider = &provider;
    assert_eq!(
        object.description(),
        ProviderDescription::new("test.provider", "In-memory test graph")
    );
    let first = object.snapshot().unwrap();
    let second = object.snapshot().unwrap();
    assert!(std::ptr::eq(first.graph, &provider.graph));
    assert!(std::ptr::eq(first.graph, second.graph));
    assert_eq!(first.completeness, SnapshotCompleteness::Complete);
    assert_eq!(first.graph.revision().as_str(), "1");
}

#[test]
fn incomplete_snapshots_preserve_generic_diagnostics() {
    let warning =
        SnapshotWarning::new("truncated", "Provider limit reached").with_locator("logical/subtree");
    let mut provider = provider();
    provider.warnings.push(warning.clone());
    let object: &dyn MetaProvider = &provider;
    let snapshot = object.snapshot().unwrap();
    assert!(std::ptr::eq(snapshot.graph, &provider.graph));
    assert_eq!(
        snapshot.completeness,
        SnapshotCompleteness::Incomplete {
            warnings: vec![warning]
        }
    );
    assert_eq!(
        MetaSnapshot::incomplete(&provider.graph, vec![]).completeness,
        SnapshotCompleteness::Incomplete { warnings: vec![] }
    );
    assert_eq!(
        MetaSnapshot::new(&provider.graph, SnapshotCompleteness::default()),
        MetaSnapshot::complete(&provider.graph)
    );
}

#[test]
fn provider_errors_retain_downcastable_graph_errors() {
    let original = Error::EmptyId { field: "graph_id" };
    let error = ProviderError::with_source(
        ProviderErrorKind::InvalidSnapshot,
        "Cannot publish graph",
        original.clone(),
    );
    assert_eq!(error.kind, ProviderErrorKind::InvalidSnapshot);
    assert_eq!(
        error.source().unwrap().downcast_ref::<Error>(),
        Some(&original)
    );
    assert!(error.to_string().contains("Cannot publish graph"));
    let error = ProviderError::new(ProviderErrorKind::Unavailable, "Not ready");
    assert!(error.source().is_none());
    assert_eq!(error.message, "Not ready");
}

#[test]
fn failures_work_through_trait_objects() {
    struct Failing;
    impl MetaProvider for Failing {
        fn description(&self) -> ProviderDescription {
            ProviderDescription::new("test.failing", "Unavailable")
        }
        fn snapshot(&self) -> Result<MetaSnapshot<'_>, ProviderError> {
            Err(ProviderError::new(ProviderErrorKind::Other, "Failed"))
        }
    }
    let object: Box<dyn MetaProvider> = Box::new(Failing);
    assert_eq!(
        object.snapshot().unwrap_err().kind,
        ProviderErrorKind::Other
    );
}
