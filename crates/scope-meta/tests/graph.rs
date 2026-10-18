use scope_meta::*;

fn ty(name: &str) -> TypeId {
    TypeId::new(name, "1")
}
fn registry() -> TypeRegistry {
    let mut r = TypeRegistry::new();
    r.register_node(ty("demo.group")).unwrap();
    r.register_node(ty("demo.item")).unwrap();
    r.register_relation(ty("demo.contains"), ty("demo.group"), ty("demo.item"))
        .unwrap();
    r
}
fn nodes() -> Vec<MetaNode> {
    vec![
        MetaNode {
            id: "b".into(),
            kind: ty("demo.item"),
            label: "Item".into(),
            source: Some(SourceRef {
                locator: "project/item.txt".into(),
                span: Some(ByteRange { start: 0, end: 4 }),
                revision: Some("source-1".into()),
            }),
        },
        MetaNode {
            id: "a".into(),
            kind: ty("demo.group"),
            label: "Group".into(),
            source: None,
        },
    ]
}
fn edge() -> MetaEdge {
    MetaEdge {
        from: "a".into(),
        relation: ty("demo.contains"),
        to: "b".into(),
        provenance: Provenance::Observed {
            provider: "demo.fixture".into(),
        },
    }
}
fn graph(revision: &str) -> MetaGraph {
    MetaGraph::new(
        "demo".into(),
        revision.into(),
        registry(),
        nodes(),
        vec![edge()],
    )
    .unwrap()
}

#[test]
fn multiple_endpoint_pairs_are_exact_canonical_and_validated() {
    let mut r = registry();
    r.register_relation_pair(ty("demo.contains"), ty("demo.item"), ty("demo.group"))
        .unwrap();
    assert!(matches!(
        r.register_relation_pair(ty("demo.contains"), ty("demo.item"), ty("demo.group")),
        Err(Error::DuplicateRelationPair { .. })
    ));
    assert!(matches!(
        r.register_relation_pair(ty("missing.relation"), ty("demo.item"), ty("demo.group")),
        Err(Error::UnknownRelationType(_))
    ));
    let mut reverse = edge();
    reverse.from = "b".into();
    reverse.to = "a".into();
    let g = MetaGraph::new(
        "demo".into(),
        "1".into(),
        r.clone(),
        nodes(),
        vec![edge(), reverse],
    )
    .unwrap();
    let restored: MetaGraph = serde_json::from_value(serde_json::to_value(&g).unwrap()).unwrap();
    assert_eq!(restored, g);
    let mut same_kind = edge();
    same_kind.to = "a".into();
    assert!(matches!(
        MetaGraph::new(
            "demo".into(),
            "1".into(),
            r.clone(),
            nodes(),
            vec![same_kind]
        ),
        Err(Error::RelationMismatch { .. })
    ));
    let mut other = TypeRegistry::new();
    other.register_node(ty("demo.group")).unwrap();
    other.register_node(ty("demo.item")).unwrap();
    other
        .register_relation(ty("demo.contains"), ty("demo.item"), ty("demo.group"))
        .unwrap();
    other
        .register_relation_pair(ty("demo.contains"), ty("demo.group"), ty("demo.item"))
        .unwrap();
    assert_eq!(other, r);
    let mut value = serde_json::to_value(&r).unwrap();
    value["relations"][0]["additional_pairs"] =
        serde_json::json!([[ty("missing.kind"), ty("demo.group")]]);
    assert!(serde_json::from_value::<TypeRegistry>(value).is_err());
    let mut value = serde_json::to_value(&r).unwrap();
    value["relations"][0]["additional_pairs"] =
        serde_json::json!([[ty("demo.group"), ty("demo.item")]]);
    assert!(serde_json::from_value::<TypeRegistry>(value).is_err());
}

#[test]
fn construction_and_queries() {
    let g = graph("1");
    assert_eq!(
        g.nodes().iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(g.find(&ty("demo.item"), Some("It")).unwrap().len(), 1);
    assert!(
        g.find(&ty("demo.item"), Some("missing"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        g.related_by(&"a".into(), &ty("demo.contains")).unwrap(),
        vec![&edge()]
    );
    assert!(g.related(&"b".into()).unwrap().is_empty());
    assert_eq!(g.describe(&"b".into()).unwrap().incoming.len(), 1);
    assert_eq!(
        g.source(&"b".into()).unwrap().unwrap().span,
        Some(ByteRange { start: 0, end: 4 })
    );
    assert!(g.source(&"a".into()).unwrap().is_none());
}

#[test]
fn predictable_query_errors() {
    let g = graph("1");
    let id = NodeId::new("missing");
    assert_eq!(g.get(&id), Err(Error::UnknownNode(id.clone())));
    assert!(matches!(g.related(&id), Err(Error::UnknownNode(_))));
    assert!(matches!(g.source(&id), Err(Error::UnknownNode(_))));
    assert!(matches!(g.describe(&id), Err(Error::UnknownNode(_))));
    assert!(matches!(
        g.find(&ty("other.item"), None),
        Err(Error::UnknownNodeType(_))
    ));
    assert!(matches!(
        g.related_by(&"a".into(), &ty("other.edge")),
        Err(Error::UnknownRelationType(_))
    ));
}

#[test]
fn registry_rejects_invalid_and_duplicate_types() {
    let mut r = registry();
    assert!(matches!(
        r.register_node(ty("demo.item")),
        Err(Error::DuplicateNodeType(_))
    ));
    assert!(matches!(
        r.register_node(ty("unnamespaced")),
        Err(Error::InvalidType(_))
    ));
    assert!(matches!(
        r.register_relation(ty("other.edge"), ty("missing.type"), ty("demo.item")),
        Err(Error::UnknownNodeType(_))
    ));
    assert!(matches!(
        r.register_relation(ty("demo.contains"), ty("demo.group"), ty("demo.item")),
        Err(Error::DuplicateRelationType(_))
    ));
    r.register_node(TypeId::new("custom:item", "2")).unwrap();
}

#[test]
fn invalid_graphs_are_rejected() {
    let build = |ns, es| MetaGraph::new("demo".into(), "1".into(), registry(), ns, es);
    let mut ns = nodes();
    ns.push(ns[0].clone());
    assert!(matches!(build(ns, vec![]), Err(Error::DuplicateNode(_))));
    let mut e = edge();
    e.to = "missing".into();
    assert!(matches!(
        build(nodes(), vec![e]),
        Err(Error::UnknownNode(_))
    ));
    let mut e = edge();
    e.relation = ty("missing.edge");
    assert!(matches!(
        build(nodes(), vec![e]),
        Err(Error::UnknownRelationType(_))
    ));
    let mut e = edge();
    e.to = "a".into();
    assert!(matches!(
        build(nodes(), vec![e]),
        Err(Error::RelationMismatch { .. })
    ));
    assert!(matches!(
        build(nodes(), vec![edge(), edge()]),
        Err(Error::DuplicateEdge(_))
    ));
    let mut ns = nodes();
    ns[0].kind = ty("missing.type");
    assert!(matches!(build(ns, vec![]), Err(Error::UnknownNodeType(_))));
    let mut ns = nodes();
    ns[0].source.as_mut().unwrap().span = Some(ByteRange { start: 5, end: 1 });
    assert!(matches!(
        build(ns, vec![]),
        Err(Error::InvalidByteRange { .. })
    ));
    let mut e = edge();
    e.provenance = Provenance::Proposed {
        provider: "".into(),
    };
    assert!(matches!(
        build(nodes(), vec![e]),
        Err(Error::EmptyProvider(_))
    ));
}

#[test]
fn references_are_graph_and_revision_aware() {
    let g = graph("1");
    let reference = g.node_ref(&"b".into()).unwrap();
    assert_eq!(g.resolve(&reference).unwrap().id, NodeId::new("b"));
    assert!(matches!(
        graph("2").resolve(&reference),
        Err(Error::StaleRevision { .. })
    ));
    let mut wrong = reference;
    wrong.graph = "other".into();
    assert!(matches!(g.resolve(&wrong), Err(Error::WrongGraph { .. })));
    assert!(matches!(
        MetaGraph::new("demo".into(), "".into(), registry(), vec![], vec![]),
        Err(Error::EmptyId { .. })
    ));
}

#[test]
fn serialization_roundtrip_and_validation() {
    let g = graph("1");
    let json = serde_json::to_string(&g).unwrap();
    let restored: MetaGraph = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, g);
    assert_eq!(serde_json::to_string(&restored).unwrap(), json);
    let mut value = serde_json::to_value(&g).unwrap();
    value["edges"][0]["to"] = "missing".into();
    assert!(serde_json::from_value::<MetaGraph>(value).is_err());
    let mut value = serde_json::to_value(&g).unwrap();
    value["registry"]["nodes"] = serde_json::json!([]);
    assert!(serde_json::from_value::<MetaGraph>(value).is_err());
}

#[test]
fn provenance_and_order_are_preserved() {
    let observed = edge();
    let mut inferred = edge();
    inferred.provenance = Provenance::Inferred {
        provider: "demo.model".into(),
    };
    let mut proposed = edge();
    proposed.provenance = Provenance::Proposed {
        provider: "demo.user".into(),
    };
    let es = vec![proposed, observed, inferred];
    let g = MetaGraph::new("demo".into(), "1".into(), registry(), nodes(), es.clone()).unwrap();
    let mut reversed = es;
    reversed.reverse();
    let mut ns = nodes();
    ns.reverse();
    let other = MetaGraph::new("demo".into(), "1".into(), registry(), ns, reversed).unwrap();
    assert_eq!(g, other);
    assert_eq!(g.related(&"a".into()).unwrap().len(), 3);
    let restored: MetaGraph = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
    assert_eq!(restored.edges(), g.edges());
}
