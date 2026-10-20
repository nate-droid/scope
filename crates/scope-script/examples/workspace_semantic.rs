#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod language;

use scope_ide::{
    document::{Document, DocumentId},
    source_edit::{DocumentSource, PermittedEditScope, SourceEditController, SourceTarget},
    text::{Position, TextRange},
    tool::{AgentSession, semantic::SemanticToolProvider},
    workspace::Workspace,
};
use scope_script::{Interpreter, semantic::SemanticHost};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--allow-read" {
        return Err("usage: workspace_semantic --allow-read ROOT (opens example.decl)".into());
    }
    let workspace = Workspace::read_only(&args[1])?;
    let file = workspace.load_file_revision("example.decl")?;
    let mut document = Document::from_revisioned_workspace_file(DocumentId::new(1), file);
    let before = document.clone();
    let service = language::DeclarationLanguage::default();
    let mut controller = SourceEditController::default();
    {
        let target = DocumentSource::new(&mut document, "example.declarations");
        let provider = SemanticToolProvider::new(
            &service,
            &target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(TextRange::new(Position::new(0, 0), Position::new(0, 15))),
            |authorization| authorization.source.id == target.source_id(),
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
        interpreter.eval(include_str!("workspace_semantic.scope"), &mut host)?;
        for value in interpreter.take_output() {
            println!("{value:?}");
        }
    }
    assert_eq!(document, before);
    assert_eq!(controller.list().count(), 0);
    let after = workspace.load_file_revision("example.decl")?;
    assert_eq!(Some(after.revision.as_str()), before.disk_revision());
    assert_eq!(after.contents, before.text());
    Ok(())
}
