#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod language;
use scope_ide::{
    scratch::ScratchBuffers,
    source_edit::{PermittedEditScope, SourceEditController, SourceTarget},
    text::{Position, TextRange},
    tool::{AgentSession, semantic::SemanticToolProvider},
};
use scope_script::{Interpreter, semantic::SemanticHost};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = language::DeclarationLanguage::default();
    let mut buffers = ScratchBuffers::default();
    buffers.create("example.declarations", language::SOURCE);
    let target = buffers.selected().unwrap();
    let before = (target.text().to_owned(), target.version());
    let mut controller = SourceEditController::default();
    {
        let provider = SemanticToolProvider::new(
            &service,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(TextRange::new(Position::new(0, 0), Position::new(0, 15))),
            |_| true,
        )?;
        let session = AgentSession::builder().provider(provider).build()?;
        let mut host = SemanticHost::new(
            session,
            "language.example.declarations.describe_selection",
            target.text(),
            target.version(),
            Some(true),
        );
        let mut interpreter = Interpreter::new();
        interpreter.eval(include_str!("semantic.scope"), &mut host)?;
        for value in interpreter.take_output() {
            println!("{value:?}");
        }
    }
    assert_eq!((target.text().to_owned(), target.version()), before);
    assert_eq!(controller.list().count(), 0);
    Ok(())
}
