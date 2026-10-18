//! In-memory outline; no workspace, filesystem, process, or network access.
use scope_ide::language::{LanguageDocument, LanguageMeta, LanguagePath, SimpleLanguage};
use scope_meta::{GraphId, MetaProvider, NodeId, RevisionId, TypeId};

fn inspect(provider: &dyn MetaProvider) -> Result<(), Box<dyn std::error::Error>> {
    let description = provider.description();
    let snapshot = provider.snapshot()?;
    println!("{}: {:?}", description.id, snapshot.completeness);
    let graph = snapshot.graph;
    let root = NodeId::new("document");
    println!("document: {}", graph.get(&root)?.label);
    let functions = graph.find(&TypeId::new("scope.language.function", "1"), Some("main"))?;
    assert_eq!(functions.len(), 1);
    for edge in graph.related_by(&root, &TypeId::new("scope.language.declares", "1"))? {
        let node = graph.get(&edge.to)?;
        let source = graph.source(&node.id)?.expect("outline source");
        println!(
            "{}: {} {:?} revision={:?}",
            node.label, source.locator, source.span, source.revision
        );
    }
    let detail = graph.describe(&root)?;
    assert_eq!(detail.outgoing.len(), 2);
    assert!(detail.incoming.is_empty());
    let reference = graph.node_ref(&root)?;
    assert_eq!(graph.resolve(&reference)?.id, root);
    Ok(())
}

fn outline(
    text: &str,
    graph_revision: &str,
    content_revision: &str,
) -> Result<LanguageMeta, scope_meta::ProviderError> {
    LanguageMeta::new(
        GraphId::new("example:outline"),
        RevisionId::new(graph_revision),
        "memory/main.scope",
        content_revision,
        LanguageDocument::new(LanguagePath::new("main.scope"), text, 7),
        &SimpleLanguage,
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let original = outline("fn main() {\n  let answer = 42\n}\n", "outline-1", "text-1")?;
    inspect(&original)?;
    let reference = original.graph().node_ref(&NodeId::new("document"))?;
    let replacement = outline("fn main() {\n  let answer = 43\n}\n", "outline-2", "text-2")?;
    assert!(replacement.graph().resolve(&reference).is_err());
    assert!(original.graph().resolve(&reference).is_ok());
    assert_eq!(
        original
            .graph()
            .source(&NodeId::new("document"))?
            .unwrap()
            .revision
            .as_deref(),
        Some("text-1")
    );
    println!("Rebuilt outline rejects old references; original remains historical.");

    #[cfg(feature = "tools")]
    {
        use scope_ide::tool::{AgentSession, MetaDiscoveryProvider, ToolCall};
        let discovery = MetaDiscoveryProvider::new(vec![Box::new(replacement)])?;
        let mut session = AgentSession::builder().provider(discovery).build()?;
        let result = session.invoke(ToolCall::new("scope.meta.list_providers"))?;
        println!("Discovery: {:?}", result.value);
    }
    Ok(())
}
