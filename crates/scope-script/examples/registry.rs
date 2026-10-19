//! Run with `cargo run -p scope-script --example registry --offline`.
//! Installs a downstream demonstration provider, not a production Scope operation.
mod support;

use scope_ide::{
    document::{DocumentId, session::DocumentSession},
    tool::{ToolEffect, operations::*},
    workspace::Workspace,
};
use scope_script::{Interpreter, host::RegistryHost};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("document.txt"), "Hello, Scope! 🦀\n")?;
    let mut session = DocumentSession::open(
        Workspace::read_only(directory.path().to_string_lossy())?,
        DocumentId::new(1),
        "document.txt",
    )?;
    let snapshot = session.snapshot()?;
    let mut registry = OperationRegistry::new(OperationLimits::default());
    registry.register(Box::new(support::InspectionProvider::new()))?;
    let policy = |context: OperationAuthorization<'_>| {
        context.call.operation.provider_id == "example_inspection"
            && context.call.operation.action_id == "inspect"
            && context.descriptor.effect == ToolEffect::Read
            && context.descriptor.authorization == ["inspect_source".into()].into()
    };
    let mut host = RegistryHost {
        registry: &registry,
        session: &mut session,
        expected: snapshot.revision,
        policy: Some(&policy),
        results: vec![],
    };
    let mut interpreter = Interpreter::new();
    interpreter.eval(include_str!("registry.scope"), &mut host)?;
    for value in interpreter.take_output() {
        println!("{value:?}");
    }
    println!("Host-retained results: {:?}", host.results);
    Ok(())
}
