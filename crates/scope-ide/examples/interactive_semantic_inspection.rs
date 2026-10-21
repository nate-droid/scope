//! cargo run -p scope-ide --features tui --example interactive_semantic_inspection
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use scope_ide::{
    app::App,
    text::{Position, TextEdit},
    tui::Tui,
    workspace::Workspace,
};
use scope_language::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static LANGUAGE: InspectionLanguage = InspectionLanguage;
static CALLS: AtomicUsize = AtomicUsize::new(0);
struct InspectionLanguage;
impl LanguageService for InspectionLanguage {
    fn language_id(&self) -> &'static str {
        "example.inspection"
    }
    fn language_name(&self) -> &'static str {
        "Inspection fixture"
    }
    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.as_str().ends_with(".inspect")
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        vec![SemanticOperationDescription {
            id: "inspect".into(),
            description: "Inspect the current buffer".into(),
            arguments: vec![],
            effect: SemanticOperationEffect::Read,
            target: SemanticTargetRequirement::Document,
            constraints: None,
        }]
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        assert_eq!(request.document.path().as_str(), "sample.inspect");
        assert_eq!(request.arguments, serde_json::json!({}));
        assert!(request.selection.is_none());
        CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(SemanticOperationResult::ReadOnly {
            value: serde_json::json!({
                "text": request.document.text(),
                "revision": request.document.version(),
                "fraction": 1.25,
                "large_unsigned": u64::MAX,
            }),
            rationale: "Native JSON from the live editor buffer".into(),
        })
    }
}

struct TempWorkspace(PathBuf);
impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
impl TempWorkspace {
    fn new() -> std::io::Result<Self> {
        let base = std::env::temp_dir();
        for index in 0..1000 {
            let path = base.join(format!("scope-inspection-{}-{index}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary workspace names exhausted",
        ))
    }
}

fn key(tui: &mut Tui, app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    tui.handle_key(KeyEvent::new(code, modifiers), app);
}
fn invoke(tui: &mut Tui, app: &mut App) {
    key(tui, app, KeyCode::Enter, KeyModifiers::NONE);
    key(tui, app, KeyCode::Enter, KeyModifiers::NONE);
}
fn print_frame(
    terminal: &mut Terminal<TestBackend>,
    tui: &mut Tui,
    app: &App,
    label: &str,
) -> std::io::Result<String> {
    terminal.draw(|frame| tui.render(frame, app))?;
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    println!("\n--- {label} ---\n{text}");
    Ok(text)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = TempWorkspace::new()?;
    std::fs::write(root.0.join("sample.inspect"), "saved source")?;
    let workspace = Workspace::new(root.0.to_string_lossy().into_owned())?;
    let mut app = App::builder(workspace).language(&LANGUAGE).build()?;
    app.open_path("sample.inspect")?;
    let mut tui = Tui::default();
    let mut terminal = Terminal::new(TestBackend::new(110, 36))?;
    key(&mut tui, &mut app, KeyCode::F(8), KeyModifiers::NONE);
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    invoke(&mut tui, &mut app);
    let frame = print_frame(&mut terminal, &mut tui, &app, "initial read")?;
    assert!(frame.contains("CURRENT"));
    assert!(frame.contains("18446744073709551615"));
    app.open_document_mut()
        .unwrap()
        .apply_edit(TextEdit::insert(Position::new(0, 0), "unsaved "))?;
    let frame = print_frame(
        &mut terminal,
        &mut tui,
        &app,
        "old result after buffer edit",
    )?;
    assert!(frame.contains("STALE"));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    invoke(&mut tui, &mut app);
    let frame = print_frame(
        &mut terminal,
        &mut tui,
        &app,
        "fresh read after reauthorization",
    )?;
    assert!(frame.contains("CURRENT"));
    assert!(frame.contains("unsaved saved source"));
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    assert!(app.open_document().unwrap().is_dirty());
    assert_eq!(
        std::fs::read_to_string(root.0.join("sample.inspect"))?,
        "saved source"
    );
    Ok(())
}
