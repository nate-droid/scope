//! Run with `cargo run -p scope-ide --features tools --example selection_description`.
#[cfg(feature = "tools")]
mod selection_description_support;

#[cfg(feature = "tools")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use scope_ide::{
        scratch::ScratchBuffers,
        source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
        text::{Position, TextRange},
        tool::{AgentSession, ToolCall, ToolEffect, ToolValue, semantic::SemanticToolProvider},
    };
    use selection_description_support::{DeclarationLanguage, SOURCE};
    let service = DeclarationLanguage::default();
    let mut buffers = ScratchBuffers::default();
    buffers.create("example.declarations", SOURCE);
    let target = buffers.selected()?;
    let mut controller = SourceEditController::default();
    {
        let provider = SemanticToolProvider::new(
            &service,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(TextRange::new(Position::new(0, 0), Position::new(0, 15))),
            |authorization| {
                authorization.operation.effect == scope_language::SemanticOperationEffect::Read
            },
        )?;
        let mut session = AgentSession::builder().provider(provider).build()?;
        let catalog = session.catalog();
        println!("Discovered catalog: {catalog:#?}");
        // No language-specific tool name: use the installed catalog's read tool.
        let entry = catalog
            .tools
            .iter()
            .find(|entry| entry.definition.effect == ToolEffect::Read)
            .ok_or("no read operation discovered")?;
        let result = session.invoke(
            ToolCall::new(&entry.definition.name)
                .with_argument("expected_version", target.version().to_string().into())
                .with_argument("expected_source", target.text().into())
                .with_argument("arguments", ToolValue::object([] as [(&str, ToolValue); 0])),
        )?;
        println!("Opaque structured result: {:#?}", result.value);
    }
    assert_eq!(target.text(), SOURCE);
    assert_eq!(target.version(), 0);
    assert_eq!(controller.list().count(), 0);
    println!("Source unchanged; no proposals registered.");
    Ok(())
}

#[cfg(not(feature = "tools"))]
fn main() {
    eprintln!("Enable --features tools to run this example.");
}
