//! Deterministic discovery of two independently owned semantic models.
#[cfg(feature = "tools")]
mod selection_description_support;
#[cfg(feature = "tools")]
mod state_description_support;

#[cfg(feature = "tools")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use scope_ide::{
        scratch::ScratchBuffers,
        source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
        text::{Position, TextRange},
        tool::{AgentSession, ToolCall, ToolEffect, ToolValue, semantic::SemanticToolProvider},
    };
    use scope_language::LanguageService;
    let declarations = selection_description_support::DeclarationLanguage::default();
    let states = state_description_support::StateMachineLanguage::default();
    for (service, source) in [
        (
            &declarations as &dyn LanguageService,
            selection_description_support::SOURCE,
        ),
        (
            &states as &dyn LanguageService,
            state_description_support::SOURCE,
        ),
    ] {
        let mut buffers = ScratchBuffers::default();
        buffers.create(service.language_id(), source);
        let target = buffers.selected()?;
        let mut controller = SourceEditController::default();
        {
            let provider = SemanticToolProvider::new(
                service,
                target,
                &mut controller,
                PermittedEditScope::Document(target.source_id()),
                Some(TextRange::new(
                    Position::new(0, 0),
                    Position::new(0, source.lines().next().unwrap().len()),
                )),
                |authorization| {
                    authorization.operation.effect == scope_language::SemanticOperationEffect::Read
                },
            )?;
            let mut session = AgentSession::builder().provider(provider).build()?;
            for entry in session.catalog().tools {
                assert_eq!(entry.definition.effect, ToolEffect::Read);
                println!("Discovered: {entry:#?}");
                let result = session.invoke(
                    ToolCall::new(entry.definition.name)
                        .with_argument("expected_version", target.version().to_string().into())
                        .with_argument("expected_source", target.text().into())
                        .with_argument(
                            "arguments",
                            ToolValue::object([] as [(&str, ToolValue); 0]),
                        ),
                )?;
                println!("Provider-owned result: {:#?}", result.value);
            }
        }
        assert_eq!(target.text(), source);
        assert_eq!(target.version(), 0);
        assert_eq!(controller.list().count(), 0);
    }
    println!("Both models used the same catalog-driven invocation; no edits or proposals.");
    Ok(())
}

#[cfg(not(feature = "tools"))]
fn main() {
    eprintln!("Enable --features tools to run this example.");
}
