#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod language;

use scope_ide::{
    document::{Document, DocumentId},
    source_edit::{DocumentSource, PermittedEditScope, SourceEditController, SourceTarget},
    text::{Position, TextRange},
    tool::{AgentSession, semantic::SemanticToolProvider},
    workspace::Workspace,
};
use scope_script::{Interpreter, Value, semantic::SemanticHost};

pub const SCRIPTS: [(&str, &str); 3] = [
    ("e1", include_str!("semantic-explorer.scope")),
    ("e2", include_str!("capability-inspector.scope")),
    ("e3", include_str!("structured-report.scope")),
];

/// Example-level S9 wiring; all target selection and authority remain host-owned.
pub fn run(root: &str, script: &str) -> Result<(Value, Vec<Value>), Box<dyn std::error::Error>> {
    let workspace = Workspace::read_only(root)?;
    let file = workspace.load_file_revision("example.decl")?;
    let mut document = Document::from_revisioned_workspace_file(DocumentId::new(1), file);
    let before = document.clone();
    let service = language::DeclarationLanguage::default();
    let mut controller = SourceEditController::default();
    let evaluated = {
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
        let result = interpreter.eval(script, &mut host);
        result.map(|value| (value, interpreter.take_output()))
    };
    // Check even when script evaluation fails (for example, an absent metadata field).
    assert_eq!(document, before);
    assert_eq!(controller.list().count(), 0);
    let after = workspace.load_file_revision("example.decl")?;
    assert_eq!(Some(after.revision.as_str()), before.disk_revision());
    assert_eq!(after.contents, before.text());
    Ok(evaluated?)
}

#[allow(dead_code)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 || args[1] != "--allow-read" {
        return Err("usage: exploration e1|e2|e3 --allow-read ROOT (opens example.decl)".into());
    }
    let script = SCRIPTS
        .iter()
        .find(|(name, _)| *name == args[0])
        .ok_or("expected e1, e2, or e3")?
        .1;
    let (_, output) = run(&args[2], script)?;
    for value in output {
        println!("{value:?}");
    }
    Ok(())
}
