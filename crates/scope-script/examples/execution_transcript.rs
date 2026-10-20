#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod fixture;

use scope_ide::{
    document::{Document, DocumentId},
    source_edit::{DocumentSource, PermittedEditScope, SourceEditController, SourceTarget},
    text::{Position, TextRange},
    tool::{AgentSession, semantic::SemanticToolProvider},
    workspace::Workspace,
};
use scope_script::{Error, Interpreter, Value, semantic::SemanticHost};
use std::{fmt::Write, sync::atomic::Ordering};

const ACTION: &str = "language.example.declarations.describe_selection";
pub const CASES: [&str; 6] = [
    "success",
    "rejection",
    "denied",
    "missing_authorization",
    "unavailable",
    "field",
];

/// Example-owned capture, not a new Script result or error contract.
#[derive(Debug)]
pub struct Transcript {
    pub output: Vec<Value>,
    pub result: Result<Value, Error>,
}

impl Transcript {
    pub fn render(&self) -> String {
        let mut text = String::new();
        for (index, value) in self.output.iter().enumerate() {
            writeln!(text, "output[{}]: {value:?}", index + 1).unwrap();
        }
        match &self.result {
            Ok(value) => writeln!(text, "result: {value:?}").unwrap(),
            Err(error) => {
                writeln!(text, "error: {:?} at byte {}", error.kind, error.offset).unwrap()
            }
        }
        text
    }
}

pub fn run(root: &str, case: &str) -> Result<Transcript, Box<dyn std::error::Error>> {
    if !CASES.contains(&case) {
        return Err("unknown transcript case".into());
    }
    let workspace = Workspace::read_only(root)?;
    let file = workspace.load_file_revision("example.decl")?;
    let mut document = Document::from_revisioned_workspace_file(DocumentId::new(1), file);
    assert_eq!(
        document.text(),
        fixture::SOURCE,
        "expected deterministic example fixture"
    );
    let before = document.clone();
    let service = fixture::DeclarationLanguage::default();
    let mut controller = SourceEditController::default();
    let mut policy_calls = 0;
    let transcript = {
        let target = DocumentSource::new(&mut document, "example.declarations");
        let provider = SemanticToolProvider::new(
            &service,
            &target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(TextRange::new(Position::new(0, 0), Position::new(0, 15))),
            |_| {
                policy_calls += 1;
                true
            },
        )?;
        let session = AgentSession::builder().provider(provider).build()?;
        let action = if case == "unavailable" {
            "language.missing.inspect"
        } else {
            ACTION
        };
        let authorization = match case {
            "denied" => Some(false),
            "missing_authorization" => None,
            _ => Some(true),
        };
        let mut host = SemanticHost::new(
            session,
            action,
            target.text(),
            target.version(),
            authorization,
        );
        let arguments = if case == "rejection" {
            "{unexpected: true}"
        } else {
            "{}"
        };
        let ending = if case == "field" {
            "outcome.absent"
        } else {
            "outcome"
        };
        let script = format!(
            "print(\"starting\"); print({{phase: 2}}); let catalog = scope.operations(); let outcome = scope.invoke(\"semantic\", \"{action}\", {arguments}); print(outcome); {ending}; print(\"finished\"); outcome"
        );
        let mut interpreter = Interpreter::new();
        let result = interpreter.eval(&script, &mut host);
        // Drain regardless of success; do not use `?` on evaluation first.
        let output = interpreter.take_output();
        assert!(interpreter.take_output().is_empty());
        Transcript { output, result }
    };
    let dispatched = usize::from(matches!(case, "success" | "field"));
    assert_eq!(service.calls.load(Ordering::SeqCst), dispatched);
    assert_eq!(policy_calls, dispatched);
    assert_eq!(controller.list().count(), 0);
    assert_eq!(document, before);
    let after = workspace.load_file_revision("example.decl")?;
    assert_eq!(Some(after.revision.as_str()), before.disk_revision());
    assert_eq!(after.contents, before.text());
    assert_eq!(
        std::fs::read(std::path::Path::new(root).join("example.decl"))?,
        before.text().as_bytes()
    );
    Ok(transcript)
}

#[allow(dead_code)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--allow-read" {
        return Err("usage: execution_transcript --allow-read ROOT (opens example.decl)".into());
    }
    for case in CASES {
        println!("=== {case} ===");
        print!("{}", run(&args[1], case)?.render());
    }
    Ok(())
}
