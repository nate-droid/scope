//! Deterministic scripted repository exploration, not an autonomous agent.
#[cfg(feature = "tools")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use scope_ide::path::ScopePath;
    use scope_ide::tool::{
        AgentSession, MetaDiscoveryProvider, ToolCall, ToolValue, WorkspaceMetaSourceResolver,
    };
    use scope_ide::workspace::{SnapshotOptions, Workspace, WorkspaceMeta};
    use std::collections::BTreeMap;
    let workspace = Workspace::read_only(".")?;
    let meta = WorkspaceMeta::new(
        workspace.clone(),
        SnapshotOptions {
            max_entries: 10000,
            max_depth: 4,
        },
    )?;
    let mut provider = MetaDiscoveryProvider::new(vec![Box::new(meta)])?;
    provider.register_source_resolver(
        "scope.workspace@1",
        WorkspaceMetaSourceResolver::new(
            workspace,
            BTreeMap::from([(
                "project/Cargo.toml".into(),
                ScopePath::new("Cargo.toml").expect("valid static workspace path"),
            )]),
        ),
    )?;
    let mut session = AgentSession::builder().provider(provider).build()?;
    println!("Scripted tools-only exploration (no model or autonomous loop)");
    println!(
        "{:?}",
        session
            .invoke(ToolCall::new("scope.meta.list_providers"))?
            .value
    );
    let found = session
        .invoke(
            ToolCall::new("scope.meta.find")
                .with_argument("provider", "scope.workspace@1".into())
                .with_argument(
                    "kind",
                    ToolValue::object([("name", "scope.file".into()), ("version", "1".into())]),
                )
                .with_argument("label", "Cargo.toml".into()),
        )?
        .value;
    let ToolValue::Object(result) = found else {
        panic!("object result")
    };
    let ToolValue::Array(nodes) = &result["nodes"] else {
        panic!("nodes")
    };
    let reference = nodes
        .iter()
        .find_map(|n| {
            let ToolValue::Object(n) = n else { return None };
            let ToolValue::Object(r) = &n["ref"] else {
                return None;
            };
            (r["node"].as_str() == Some("Cargo.toml")).then(|| n["ref"].clone())
        })
        .expect("repository manifest discovered");
    let root = session
        .invoke(
            ToolCall::new("scope.meta.find")
                .with_argument("provider", "scope.workspace@1".into())
                .with_argument(
                    "kind",
                    ToolValue::object([
                        ("name", "scope.workspace".into()),
                        ("version", "1".into()),
                    ]),
                ),
        )?
        .value;
    let ToolValue::Object(root) = root else {
        panic!("object")
    };
    let ToolValue::Array(roots) = &root["nodes"] else {
        panic!("nodes")
    };
    let ToolValue::Object(root) = &roots[0] else {
        panic!("node")
    };
    let children = session.invoke(
        ToolCall::new("scope.meta.related")
            .with_argument("provider", "scope.workspace@1".into())
            .with_argument("node", root["ref"].clone())
            .with_argument("limit", 3usize.into()),
    )?;
    println!("root containment: {:?}", children.value);
    for op in ["inspect", "source"] {
        let result = session.invoke(
            ToolCall::new(format!("scope.meta.{op}"))
                .with_argument("provider", "scope.workspace@1".into())
                .with_argument("node", reference.clone()),
        )?;
        println!("{op}: {:?}", result.value);
    }
    Ok(())
}
#[cfg(not(feature = "tools"))]
fn main() {
    eprintln!("Run with --features tools");
}
