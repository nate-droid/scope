use scope_meta::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entity = TypeId::new("example.entity", "1");
    let contains = TypeId::new("example.contains", "1");
    let mut registry = TypeRegistry::new();
    registry.register_node(entity.clone())?;
    registry.register_relation(contains.clone(), entity.clone(), entity.clone())?;
    let graph = MetaGraph::new(
        "example.snapshot".into(),
        "revision-1".into(),
        registry,
        vec![
            MetaNode {
                id: "root".into(),
                kind: entity.clone(),
                label: "Root".into(),
                source: None,
            },
            MetaNode {
                id: "item".into(),
                kind: entity,
                label: "Item".into(),
                source: Some(SourceRef {
                    locator: "project/item.txt".into(),
                    span: None,
                    revision: None,
                }),
            },
        ],
        vec![MetaEdge {
            from: "root".into(),
            relation: contains.clone(),
            to: "item".into(),
            provenance: Provenance::Observed {
                provider: "example.fixture".into(),
            },
        }],
    )?;
    for edge in graph.related_by(&"root".into(), &contains)? {
        let node = graph.get(&edge.to)?;
        println!(
            "{} --{}--> {} ({:?})",
            edge.from, edge.relation, node.label, edge.provenance
        );
        println!("source: {:?}", graph.source(&node.id)?);
    }
    let reference = graph.node_ref(&"item".into())?;
    println!("resolved: {}", graph.resolve(&reference)?.label);
    Ok(())
}
