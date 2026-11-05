#![cfg(feature = "tui")]

//! Exercise the shipped TUI, not a replacement modal or direct bridge invocation.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use scope_ide::{
    app::App,
    text::{Position, Span, TextEdit},
    tui::{InputOutcome, Tui},
    workspace::Workspace,
};
use scope_language::{
    BlockerCategory, LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService,
    SemanticBlocker, SemanticOperationDescription, SemanticOperationEffect, SemanticOperationError,
    SemanticOperationRequest, SemanticOperationResult, SemanticTargetRequirement,
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug, PartialEq, Eq)]
struct Invocation {
    operation: String,
    text: String,
    version: u64,
}

struct Inspector {
    language: &'static str,
    extension: &'static str,
    calls: Mutex<Vec<Invocation>>,
    outcome: Option<Result<SemanticOperationResult, SemanticOperationError>>,
}

impl Inspector {
    fn new(language: &'static str, extension: &'static str) -> &'static Self {
        Self::with_outcome(language, extension, None)
    }

    fn with_outcome(
        language: &'static str,
        extension: &'static str,
        outcome: Option<Result<SemanticOperationResult, SemanticOperationError>>,
    ) -> &'static Self {
        // App's public registry accepts static services. Each test owns a distinct
        // fixture, so parallel tests cannot share invocation state.
        Box::leak(Box::new(Self {
            language,
            extension,
            calls: Mutex::new(Vec::new()),
            outcome,
        }))
    }

    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl LanguageService for Inspector {
    fn language_id(&self) -> &'static str {
        self.language
    }

    fn language_name(&self) -> &'static str {
        self.language
    }

    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some(self.extension)
    }

    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }

    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        ["alpha", "beta"]
            .into_iter()
            .map(|name| SemanticOperationDescription {
                id: format!("{}.{name}", self.language),
                description: format!("Inspect {name} snapshot"),
                arguments: vec![],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            })
            .collect()
    }

    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        assert_eq!(request.arguments, json!({}));
        assert!(request.selection.is_none());
        assert!(
            self.semantic_operations()
                .iter()
                .any(|op| op.id == request.operation_id)
        );
        self.calls.lock().unwrap().push(Invocation {
            operation: request.operation_id.into(),
            text: request.document.text().into(),
            version: request.document.version(),
        });
        if let Some(outcome) = &self.outcome {
            return outcome.clone();
        }
        Ok(SemanticOperationResult::ReadOnly {
            value: json!({
                "nested": {"items": ["snapshot-marker", 7]},
                "validation": false,
                "float": 1.25,
                "unsigned": u64::MAX,
            }),
            rationale: "Read the supplied unsaved snapshot only".into(),
        })
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "scope-interactive-inspection-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("first.inspectone"), "saved first").unwrap();
        std::fs::write(root.join("second.inspecttwo"), "saved second").unwrap();
        Self(root)
    }

    fn app(&self, services: &[&'static Inspector]) -> App {
        App::builder(Workspace::read_only(self.0.to_str().unwrap()).unwrap())
            .languages(
                services
                    .iter()
                    .map(|service| *service as &'static dyn LanguageService),
            )
            .build()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn press(tui: &mut Tui, app: &mut App, code: KeyCode) {
    assert_eq!(
        tui.handle_key(KeyEvent::new(code, KeyModifiers::NONE), app),
        InputOutcome::Handled,
    );
}

fn open_inspection(tui: &mut Tui, app: &mut App) {
    press(tui, app, KeyCode::F(8));
}

fn open_inspection_alias(tui: &mut Tui, app: &mut App) {
    assert_eq!(
        tui.handle_key(
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL),
            app
        ),
        InputOutcome::Handled,
    );
}

fn screen(tui: &mut Tui, app: &App, terminal: &mut Terminal<TestBackend>) -> String {
    terminal.draw(|frame| tui.render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn replace(app: &mut App, text: &str) {
    let document = app.open_document_mut().unwrap();
    let range = document
        .buffer()
        .range_for_span(Span::new(0, document.text().len()))
        .unwrap();
    document.apply_edit(TextEdit::replace(range, text)).unwrap();
}

#[test]
fn modal_authorizes_unsaved_snapshots_renders_structured_results_and_reinvokes_when_stale() {
    let fixture = Fixture::new();
    let service = Inspector::new("fixture.one", "inspectone");
    let mut app = fixture.app(&[service]);
    app.open_path("first.inspectone").unwrap();
    replace(&mut app, "unsaved λ");
    let mut tui = Tui::default();
    let mut terminal = Terminal::new(TestBackend::new(150, 60)).unwrap();
    screen(&mut tui, &app, &mut terminal);

    open_inspection(&mut tui, &mut app);
    let listing = screen(&mut tui, &app, &mut terminal);
    assert!(listing.contains("fixture.one.alpha"), "{listing}");
    assert!(listing.contains("fixture.one.beta"), "{listing}");
    assert_eq!(service.count(), 0, "discovery/rendering must not execute");

    // Both navigation directions are dispatched through the real modal.
    press(&mut tui, &mut app, KeyCode::Down);
    press(&mut tui, &mut app, KeyCode::Up);
    press(&mut tui, &mut app, KeyCode::Down);
    press(&mut tui, &mut app, KeyCode::Enter);
    let confirmation = screen(&mut tui, &app, &mut terminal);
    assert!(confirmation.contains("fixture.one.beta"), "{confirmation}");
    assert!(
        confirmation.to_lowercase().contains("confirm"),
        "{confirmation}"
    );
    assert_eq!(
        service.count(),
        0,
        "first Enter captures but does not authorize execution"
    );

    let before = app.open_document().unwrap().clone();
    press(&mut tui, &mut app, KeyCode::Enter);
    let preview = screen(&mut tui, &app, &mut terminal);
    assert!(
        preview.contains("d toggles Details (full retained evidence)"),
        "{preview}"
    );
    press(&mut tui, &mut app, KeyCode::Char('e'));
    let result = screen(&mut tui, &app, &mut terminal);
    assert!(result.contains("CURRENT"), "{result}");
    assert!(result.contains("\"validation\": false"), "{result}");
    assert!(result.contains("snapshot-marker"), "{result}");
    assert!(result.contains("\"float\": 1.25"), "{result}");
    assert!(
        result.contains("\"unsigned\": 18446744073709551615"),
        "{result}"
    );
    // Pretty JSON must span lines rather than be a debug/compact object dump.
    let nested = result.find("\"nested\": {").expect(&result);
    let items = result.find("\"items\": [").expect(&result);
    assert!(result[nested..items].contains('\n'), "{result}");
    assert_eq!(app.open_document().unwrap(), &before);
    assert_eq!(
        service.calls.lock().unwrap().as_slice(),
        &[Invocation {
            operation: "fixture.one.beta".into(),
            text: "unsaved λ".into(),
            version: 1,
        }]
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("first.inspectone")).unwrap(),
        "saved first"
    );

    app.open_document_mut()
        .unwrap()
        .apply_edit(TextEdit::insert(Position::new(0, 0), "fresh "))
        .unwrap();
    let stale = screen(&mut tui, &app, &mut terminal);
    assert!(stale.contains("STALE"), "{stale}");
    assert!(stale.contains("\"validation\": false"), "{stale}");
    assert_eq!(service.count(), 1);
    press(&mut tui, &mut app, KeyCode::Enter);
    screen(&mut tui, &app, &mut terminal);
    assert_eq!(
        service.count(),
        1,
        "reinvocation requires a new confirmation"
    );
    let edited = app.open_document().unwrap().clone();
    press(&mut tui, &mut app, KeyCode::Enter);
    let fresh = screen(&mut tui, &app, &mut terminal);
    assert!(fresh.contains("CURRENT"), "{fresh}");
    assert!(!fresh.contains("STALE"), "{fresh}");
    assert_eq!(
        service.calls.lock().unwrap().as_slice(),
        &[
            Invocation {
                operation: "fixture.one.beta".into(),
                text: "unsaved λ".into(),
                version: 1
            },
            Invocation {
                operation: "fixture.one.beta".into(),
                text: "fresh unsaved λ".into(),
                version: 2
            },
        ]
    );
    assert_eq!(app.open_document().unwrap(), &edited);

    press(&mut tui, &mut app, KeyCode::Esc);
    let closed = screen(&mut tui, &app, &mut terminal);
    assert!(!closed.contains("snapshot-marker"), "{closed}");
    assert_eq!(service.count(), 2);
    assert_eq!(
        tui.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &mut app),
        InputOutcome::QuitRequested
    );
}

#[test]
fn editing_after_confirmation_rejects_execution_without_invoking_the_service() {
    let fixture = Fixture::new();
    let service = Inspector::new("fixture.one", "inspectone");
    let mut app = fixture.app(&[service]);
    app.open_path("first.inspectone").unwrap();
    let mut tui = Tui::default();
    let mut terminal = Terminal::new(TestBackend::new(150, 60)).unwrap();
    screen(&mut tui, &app, &mut terminal);
    open_inspection(&mut tui, &mut app);
    press(&mut tui, &mut app, KeyCode::Enter);
    let confirmation = screen(&mut tui, &app, &mut terminal);
    assert!(
        confirmation.contains("Inspect current document?"),
        "{confirmation}"
    );
    assert_eq!(service.count(), 0);

    replace(&mut app, "changed after confirmation");
    let before = app.open_document().unwrap().clone();
    let stale = screen(&mut tui, &app, &mut terminal);
    assert!(stale.contains("STALE confirmation"), "{stale}");
    press(&mut tui, &mut app, KeyCode::Enter);
    let rejected = screen(&mut tui, &app, &mut terminal);
    assert!(rejected.contains("Invocation error:"), "{rejected}");
    assert!(rejected.contains("SnapshotMismatch"), "{rejected}");
    assert_eq!(
        service.count(),
        0,
        "stale authorization must never dispatch"
    );
    assert_eq!(app.open_document().unwrap(), &before);
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("first.inspectone")).unwrap(),
        "saved first"
    );
    press(&mut tui, &mut app, KeyCode::Esc);
    let closed = screen(&mut tui, &app, &mut terminal);
    assert!(!closed.contains("Invocation error:"), "{closed}");
}

#[test]
fn blocked_and_error_feedback_remain_visible_in_the_result_modal() {
    let outcomes = [
        (
            Ok(SemanticOperationResult::Blocked {
                blocker: SemanticBlocker {
                    operation_id: "fixture.one.alpha".into(),
                    category: BlockerCategory::MissingDependency,
                    explanation: "fixture dependency unavailable".into(),
                    recoverable: false,
                    missing_dependencies: vec!["fixture-library".into()],
                    diagnostics: vec![],
                    suggested_next_action: None,
                },
            }),
            "\"status\": \"blocked\"",
            "fixture dependency unavailable",
        ),
        (
            Err(SemanticOperationError::InternalFailure(
                "fixture inspection failed".into(),
            )),
            "\"status\": \"internal_failure\"",
            "fixture inspection failed",
        ),
    ];
    for (outcome, status, message) in outcomes {
        let fixture = Fixture::new();
        let service = Inspector::with_outcome("fixture.one", "inspectone", Some(outcome));
        let mut app = fixture.app(&[service]);
        app.open_path("first.inspectone").unwrap();
        let before = app.open_document().unwrap().clone();
        let mut tui = Tui::default();
        let mut terminal = Terminal::new(TestBackend::new(150, 60)).unwrap();
        screen(&mut tui, &app, &mut terminal);
        open_inspection(&mut tui, &mut app);
        press(&mut tui, &mut app, KeyCode::Enter);
        screen(&mut tui, &app, &mut terminal);
        assert_eq!(service.count(), 0);
        press(&mut tui, &mut app, KeyCode::Enter);
        let result = screen(&mut tui, &app, &mut terminal);
        assert!(result.contains(status), "{result}");
        assert!(result.contains(message), "{result}");
        assert_eq!(service.count(), 1);
        assert_eq!(app.open_document().unwrap(), &before);
        press(&mut tui, &mut app, KeyCode::Esc);
        let closed = screen(&mut tui, &app, &mut terminal);
        assert!(!closed.contains(message), "{closed}");
        assert_eq!(service.count(), 1);
    }
}

#[test]
fn reopening_after_document_language_switch_discovers_and_invokes_the_new_service() {
    let fixture = Fixture::new();
    let first = Inspector::new("fixture.one", "inspectone");
    let second = Inspector::new("fixture.two", "inspecttwo");
    let mut app = fixture.app(&[first, second]);
    app.open_path("first.inspectone").unwrap();
    let mut tui = Tui::default();
    let mut terminal = Terminal::new(TestBackend::new(150, 60)).unwrap();
    screen(&mut tui, &app, &mut terminal);
    open_inspection(&mut tui, &mut app);
    press(&mut tui, &mut app, KeyCode::Enter);
    screen(&mut tui, &app, &mut terminal);
    press(&mut tui, &mut app, KeyCode::Esc);
    assert_eq!(first.count(), 0, "cancelling confirmation must not execute");

    app.open_path("second.inspecttwo").unwrap();
    replace(&mut app, "second unsaved");
    open_inspection_alias(&mut tui, &mut app);
    let listing = screen(&mut tui, &app, &mut terminal);
    assert!(listing.contains("fixture.two.alpha"), "{listing}");
    assert!(!listing.contains("fixture.one.alpha"), "{listing}");
    assert_eq!(second.count(), 0);
    press(&mut tui, &mut app, KeyCode::Enter);
    screen(&mut tui, &app, &mut terminal);
    assert_eq!(second.count(), 0);
    press(&mut tui, &mut app, KeyCode::Enter);
    let result = screen(&mut tui, &app, &mut terminal);
    assert!(result.contains("CURRENT"), "{result}");
    assert_eq!(first.count(), 0);
    assert_eq!(
        second.calls.lock().unwrap().as_slice(),
        &[Invocation {
            operation: "fixture.two.alpha".into(),
            text: "second unsaved".into(),
            version: 1,
        }]
    );
    press(&mut tui, &mut app, KeyCode::Esc);
}

fn workspace_screen(ui: &mut scope_ide::tui::WorkspaceTui, app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(150, 60)).unwrap();
    terminal
        .draw(|frame| ui.render(frame, frame.area(), app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn app_workspace(app: &App) -> scope_ide::tui::WorkspaceTui {
    use scope_ide::tui::{EditableScratchPane, View, WorkspaceTui};
    let mut ui = WorkspaceTui::new();
    ui.register_editable("editor", EditableScratchPane::new())
        .unwrap();
    ui.layout("main", View::leaf("editor")).unwrap();
    workspace_screen(&mut ui, app);
    ui
}

#[test]
fn workspace_app_editor_publishes_and_evaluates_same_unsaved_document_without_scratch_copy() {
    use crossterm::event::Event;
    use scope_ide::{scratch::ScratchBuffers, tui::WorkspaceAction};
    let fixture = Fixture::new();
    let service = Inspector::new("fixture.one", "inspectone");
    let mut app = fixture.app(&[service]);
    let mut ui = app_workspace(&app);
    let mut sources = ScratchBuffers::default();
    ui.open_app_document(&mut app, "first.inspectone").unwrap();
    let identity = app.open_document().unwrap().evaluation_source_id().clone();
    let key = |code, modifiers| Event::Key(KeyEvent::new(code, modifiers));
    ui.handle_event_with_sources(
        key(KeyCode::End, KeyModifiers::SHIFT),
        &mut app,
        &mut sources,
    );
    ui.handle_event_with_sources(Event::Paste("unsaved λ".into()), &mut app, &mut sources);
    assert_eq!(app.open_document().unwrap().text(), "saved first");
    assert!(
        ui.invoke_action(
            WorkspaceAction::InspectDocument,
            &mut app,
            &mut sources,
            None
        )
        .is_err()
    );
    ui.handle_event_with_sources(
        key(KeyCode::Char('s'), KeyModifiers::CONTROL),
        &mut app,
        &mut sources,
    );
    let document = app.open_document().unwrap();
    assert_eq!(document.evaluation_source_id(), &identity);
    assert_eq!(document.text(), "unsaved λ");
    assert_eq!(document.version(), 1);
    assert!(document.is_dirty());
    assert!(
        ui.invoke_action(
            WorkspaceAction::SaveAppDocument,
            &mut app,
            &mut sources,
            None
        )
        .is_err(),
        "read-only App authority must not be upgraded by the editor"
    );
    assert!(app.open_document().unwrap().is_dirty());
    assert!(
        sources.selected().is_err(),
        "App editing must not create a scratch copy"
    );
    let rendered = workspace_screen(&mut ui, &app);
    assert!(rendered.contains("unsaved λ"), "{rendered}");
    assert!(!rendered.contains("revision"), "{rendered}");
    assert!(!rendered.contains("Ctrl-S publish"), "{rendered}");
    ui.invoke_action(
        WorkspaceAction::InspectDocument,
        &mut app,
        &mut sources,
        None,
    )
    .unwrap();
    ui.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
    assert_eq!(service.count(), 0);
    let confirm = workspace_screen(&mut ui, &app);
    assert!(confirm.contains("unsaved λ"), "{confirm}");
    ui.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
    assert_eq!(
        service.calls.lock().unwrap().as_slice(),
        &[Invocation {
            operation: "fixture.one.alpha".into(),
            text: "unsaved λ".into(),
            version: 1,
        }]
    );
    assert!(ui.inspection_result().unwrap().is_fresh_app(&app));
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("first.inspectone")).unwrap(),
        "saved first"
    );
}

#[test]
fn workspace_app_editor_stale_publish_is_rejected_and_draft_survives() {
    use crossterm::event::Event;
    use scope_ide::{scratch::ScratchBuffers, tui::WorkspaceAction};
    let fixture = Fixture::new();
    let service = Inspector::new("fixture.one", "inspectone");
    let mut app = fixture.app(&[service]);
    let mut ui = app_workspace(&app);
    let mut sources = ScratchBuffers::default();
    ui.open_app_document(&mut app, "first.inspectone").unwrap();
    ui.handle_event_with_sources(Event::Paste("local draft ".into()), &mut app, &mut sources);
    replace(&mut app, "external revision");
    ui.handle_event_with_sources(
        Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        &mut app,
        &mut sources,
    );
    assert_eq!(app.open_document().unwrap().text(), "external revision");
    let screen = workspace_screen(&mut ui, &app);
    assert!(screen.contains("local draft"), "{screen}");
    assert!(screen.contains("CONFLICT"), "{screen}");
    assert!(ui.open_app_document(&mut app, "second.inspecttwo").is_err());
    assert!(
        ui.invoke_action(
            WorkspaceAction::InspectDocument,
            &mut app,
            &mut sources,
            None
        )
        .is_err()
    );
    assert_eq!(service.count(), 0);
    ui.handle_event_with_sources(
        Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        &mut app,
        &mut sources,
    );
    assert!(!workspace_screen(&mut ui, &app).contains("local draft"));
    assert!(
        ui.open_app_document(&mut app, "second.inspecttwo").is_err(),
        "unsaved App revision must not be discarded"
    );
}

#[cfg(unix)]
#[test]
#[ignore = "PTY child; invoked by app_document_editor_through_real_launcher"]
fn app_document_launcher_child() {
    assert_eq!(
        std::env::var("SCOPE_APP_EDITOR_PTY_CHILD").as_deref(),
        Ok("1")
    );
    let service = Inspector::new("fixture.one", "inspectone");
    scope_ide::cli::run_with_registry(
        ["workspace", "."].map(str::to_owned),
        scope_ide::language::LanguageRegistry::builtins().with_language(service),
    )
    .unwrap();
    assert_eq!(
        service.calls.lock().unwrap().as_slice(),
        &[Invocation {
            operation: "fixture.one.alpha".into(),
            text: "unsaved λ".into(),
            version: 1,
        }]
    );
}

#[cfg(unix)]
#[test]
fn app_document_editor_through_real_launcher() {
    let result = std::process::Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/pty_app_document_editor.py"
        ))
        .arg(std::env::current_exe().unwrap())
        .output()
        .expect("Python 3 and Unix PTYs are required");
    assert!(
        result.status.success(),
        "App editor PTY failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn missing_app_editor_target_never_falls_back_to_selected_scratch() {
    use crossterm::event::Event;
    use scope_ide::{scratch::ScratchBuffers, tui::WorkspaceAction};
    let fixture = Fixture::new();
    let service = Inspector::new("fixture.one", "inspectone");
    let mut app = fixture.app(&[service]);
    let mut ui = app_workspace(&app);
    let mut sources = ScratchBuffers::default();
    sources.create("scope-script", "unrelated scratch");
    ui.open_app_document(&mut app, "first.inspectone").unwrap();
    let run = ui
        .actions(&sources, true)
        .into_iter()
        .find(|a| a.action == WorkspaceAction::RunScript)
        .unwrap();
    assert!(run.disabled_reason.unwrap().contains("App editor"));
    app = fixture.app(&[service]);
    ui.handle_event_with_sources(
        Event::Paste("must not reach scratch".into()),
        &mut app,
        &mut sources,
    );
    ui.handle_event_with_sources(
        Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        &mut app,
        &mut sources,
    );
    assert_eq!(sources.selected().unwrap().text(), "unrelated scratch");
    assert!(app.open_document().is_none());
    assert!(
        ui.invoke_action(
            WorkspaceAction::InspectDocument,
            &mut app,
            &mut sources,
            None
        )
        .is_err()
    );
    assert_eq!(service.count(), 0);
}
