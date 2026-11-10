use super::*;
use crate::{test_support::TempWorkspace, tui::EditableScratchPane};
use scope_language::*;
use std::sync::Mutex;

static INPUTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
struct Formatter;
static FORMATTER: Formatter = Formatter;
impl LanguageService for Formatter {
    fn language_id(&self) -> &'static str {
        "format-fixture"
    }
    fn language_name(&self) -> &'static str {
        "Format fixture"
    }
    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("fmt")
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        vec![SemanticOperationDescription {
            id: "format".into(),
            description: "Canonical fixture".into(),
            arguments: vec![],
            effect: SemanticOperationEffect::ProposeChanges,
            target: SemanticTargetRequirement::Document,
            constraints: None,
        }]
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        assert_eq!(request.operation_id, "format");
        assert!(request.selection.is_none());
        assert_eq!(request.arguments, serde_json::json!({}));
        let text = request.document.text();
        INPUTS.lock().unwrap().push(text.to_string());
        if text.contains("wrong-result") {
            return Ok(SemanticOperationResult::ReadOnly {
                value: serde_json::json!("not edits"),
                rationale: "wrong effect".into(),
            });
        }
        if text.contains("broken") {
            return Err(SemanticOperationError::InvalidArguments(
                "fixture diagnostic".into(),
            ));
        }
        let range = if text.contains("partial") {
            TextRange::new(Position::new(0, 0), Position::new(0, 1))
        } else {
            LineIndex::new(text).range_for_span(0..text.len()).unwrap()
        };
        Ok(SemanticOperationResult::Changes {
            edits: if text == "CANONICAL" {
                vec![]
            } else {
                vec![SemanticEdit {
                    range,
                    replacement: "CANONICAL".into(),
                }]
            },
            rationale: "fixture canonical replacement".into(),
        })
    }
}

fn fixture() -> (
    TempWorkspace,
    App,
    WorkspaceTui,
    crate::scratch::ScratchBuffers,
) {
    let root = TempWorkspace::new("format-ui");
    root.write_file("draft.fmt", "original");
    let mut app = App::builder(root.workspace())
        .language(&FORMATTER)
        .build()
        .unwrap();
    app.open_path("draft.fmt").unwrap();
    let mut ui = WorkspaceTui::new();
    ui.register_editable("editor", EditableScratchPane::new())
        .unwrap();
    ui.layout("main", View::leaf("editor")).unwrap();
    ui.geometry(Rect::new(0, 0, 100, 25));
    ui.set_app_editor(true);
    (root, app, ui, crate::scratch::ScratchBuffers::default())
}
fn key(
    ui: &mut WorkspaceTui,
    app: &mut App,
    sources: &mut crate::scratch::ScratchBuffers,
    code: KeyCode,
) {
    ui.handle_event_with_sources(
        Event::Key(KeyEvent::new(code, event::KeyModifiers::NONE)),
        app,
        sources,
    );
}
fn screen(ui: &mut WorkspaceTui, app: &App, sources: &crate::scratch::ScratchBuffers) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 25)).unwrap();
    terminal
        .draw(|frame| ui.render_with_sources(frame, frame.area(), app, sources))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}
fn format(ui: &mut WorkspaceTui, app: &mut App, sources: &mut crate::scratch::ScratchBuffers) {
    ui.invoke_action(WorkspaceAction::FormatCurrentDocument, app, sources, None)
        .unwrap();
}

#[test]
fn format_previews_unpublished_draft_and_cancel_is_inert() {
    let (root, mut app, mut ui, mut sources) = fixture();
    let committed = app.open_document().unwrap().clone();
    ui.handle_event_with_sources(Event::Paste("unsaved ".into()), &mut app, &mut sources);
    let pinned = ui.current_editor_draft(&app, Some(&sources)).unwrap();
    format(&mut ui, &mut app, &mut sources);
    assert_eq!(ui.draft_review.as_ref().unwrap().pinned, pinned);
    assert!(INPUTS.lock().unwrap().contains(&pinned.text));
    assert_eq!(
        ui.draft_review.as_ref().unwrap().replacement.as_deref(),
        Some("CANONICAL")
    );
    let rendered = screen(&mut ui, &app, &sources);
    assert!(rendered.contains("Format preview"));
    assert!(rendered.contains("unsaved"));
    key(&mut ui, &mut app, &mut sources, KeyCode::Esc);
    assert!(ui.draft_review.is_none());
    assert_eq!(
        ui.current_editor_draft(&app, Some(&sources)).unwrap(),
        pinned
    );
    assert_eq!(app.open_document().unwrap(), &committed);
    assert_eq!(
        std::fs::read_to_string(root.root().join("draft.fmt")).unwrap(),
        "original"
    );
}

#[test]
fn format_apply_changes_only_local_editor_and_is_undoable() {
    let (_root, mut app, mut ui, mut sources) = fixture();
    let committed = app.open_document().unwrap().clone();
    ui.retained_inspection = Some(super::super::semantic::SemanticInspection::default());
    let before = ui.current_editor_draft(&app, Some(&sources)).unwrap().text;
    format(&mut ui, &mut app, &mut sources);
    key(&mut ui, &mut app, &mut sources, KeyCode::Enter);
    assert!(ui.draft_review.is_none());
    assert_eq!(
        ui.current_editor_draft(&app, Some(&sources)).unwrap().text,
        "CANONICAL"
    );
    assert_eq!(app.open_document().unwrap(), &committed);
    assert!(ui.retained_inspection.is_some());
    ui.handle_event_with_sources(
        Event::Key(KeyEvent::new(
            KeyCode::Char('z'),
            event::KeyModifiers::CONTROL,
        )),
        &mut app,
        &mut sources,
    );
    assert_eq!(
        ui.current_editor_draft(&app, Some(&sources)).unwrap().text,
        before
    );
}

#[test]
fn format_scratch_uses_registered_path_and_never_publishes() {
    let (root, mut app, mut ui, mut sources) = fixture();
    ui.set_app_editor(false);
    let mut session = InteractiveSession::new();
    ui.submit_command(
        ":scope open draft.fmt",
        &mut app,
        &mut session,
        None,
        &mut sources,
    );
    let version = sources.selected().unwrap().version();
    ui.handle_event_with_sources(
        Event::Paste("scratch draft ".into()),
        &mut app,
        &mut sources,
    );
    let before = ui.current_editor_draft(&app, Some(&sources)).unwrap();
    format(&mut ui, &mut app, &mut sources);
    assert!(INPUTS.lock().unwrap().contains(&before.text));
    key(&mut ui, &mut app, &mut sources, KeyCode::Enter);
    assert!(ui.draft_review.is_none());
    assert_eq!(
        ui.current_editor_draft(&app, Some(&sources)).unwrap().text,
        "CANONICAL"
    );
    assert_eq!(sources.selected().unwrap().version(), version);
    assert_eq!(sources.selected().unwrap().text(), "original");
    assert_eq!(root.read_file("draft.fmt"), "original");
}

#[test]
fn format_action_is_searchable_and_unsupported_path_cannot_dispatch() {
    let (root, mut app, mut ui, mut sources) = fixture();
    ui.refresh_action_context(&app);
    assert!(
        ui.actions(&sources, false)
            .iter()
            .any(|item| item.id == "document.format" && item.disabled_reason.is_none())
    );
    root.write_file("unknown.txt", "original");
    app.open_path("unknown.txt").unwrap();
    assert!(
        ui.invoke_action(
            WorkspaceAction::FormatCurrentDocument,
            &mut app,
            &mut sources,
            None
        )
        .is_err()
    );
    assert!(ui.draft_review.is_none());
}

#[test]
fn format_stale_preview_cannot_apply_to_changed_baseline() {
    let (_root, mut app, mut ui, mut sources) = fixture();
    format(&mut ui, &mut app, &mut sources);
    app.open_document_mut()
        .unwrap()
        .apply_edit(crate::text::TextEdit::insert(
            Position::new(0, 0),
            "changed ",
        ))
        .unwrap();
    key(&mut ui, &mut app, &mut sources, KeyCode::Enter);
    assert!(ui.draft_review.as_ref().unwrap().replacement.is_none());
    assert!(app.open_document().unwrap().text().contains("changed"));
    key(&mut ui, &mut app, &mut sources, KeyCode::Esc);
    assert!(ui.draft_review.is_none());
}

#[test]
fn format_noop_bad_result_and_provider_error_are_non_actionable() {
    for (text, expected) in [
        ("CANONICAL", "Already canonically"),
        ("partial", "whole-document"),
        ("broken", "fixture diagnostic"),
        ("wrong-result", "unexpected result"),
    ] {
        let (_root, mut app, mut ui, mut sources) = fixture();
        let source = app.open_document().unwrap().text();
        let range = LineIndex::new(source)
            .range_for_span(0..source.len())
            .unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(crate::text::TextEdit::replace(range, text))
            .unwrap();
        format(&mut ui, &mut app, &mut sources);
        assert!(ui.draft_review.as_ref().unwrap().replacement.is_none());
        assert!(screen(&mut ui, &app, &sources).contains(expected));
        key(&mut ui, &mut app, &mut sources, KeyCode::Enter);
        assert!(ui.draft_review.is_some());
        assert_eq!(app.open_document().unwrap().text(), text);
        key(&mut ui, &mut app, &mut sources, KeyCode::Char('c'));
        assert!(ui.draft_review.is_none());
    }
}

#[test]
fn format_preview_end_reaches_proposal_after_more_than_u16_rows() {
    let (_root, mut app, mut ui, mut sources) = fixture();
    let text = "\n".repeat(70_000);
    let range = LineIndex::new(app.open_document().unwrap().text())
        .range_for_span(0.."original".len())
        .unwrap();
    app.open_document_mut()
        .unwrap()
        .apply_edit(crate::text::TextEdit::replace(range, &text))
        .unwrap();
    format(&mut ui, &mut app, &mut sources);
    assert!(!screen(&mut ui, &app, &sources).contains("CANONICAL"));
    key(&mut ui, &mut app, &mut sources, KeyCode::End);
    assert!(screen(&mut ui, &app, &sources).contains("CANONICAL"));
    key(&mut ui, &mut app, &mut sources, KeyCode::Enter);
    assert_eq!(
        ui.current_editor_draft(&app, Some(&sources)).unwrap().text,
        "CANONICAL"
    );
    assert_eq!(app.open_document().unwrap().text(), text);
}

fn check_app(ui: &mut WorkspaceTui, app: &mut App, sources: &mut crate::scratch::ScratchBuffers) {
    ui.invoke_action(WorkspaceAction::CheckCurrentDocument, app, sources, None)
        .unwrap();
    assert_eq!(ui.rendered_output(app, Some(sources)), "No errors\n");
}

fn assert_historical_app_check(
    ui: &WorkspaceTui,
    app: &App,
    sources: &crate::scratch::ScratchBuffers,
) {
    let output = ui.rendered_output(app, Some(sources));
    assert!(
        output.starts_with("STALE (historical App Check;"),
        "{output}"
    );
    assert!(output.ends_with("No errors\n"), "{output}");
    assert_eq!(ui.output_errors, "No errors\n");
}

#[test]
fn app_check_format_apply_and_byte_identical_undo_remain_historical() {
    let (_root, mut app, mut ui, mut sources) = fixture();
    // Ordinary Check publishes first; evidence must pin that resulting baseline.
    ui.handle_event_with_sources(Event::Paste("local ".into()), &mut app, &mut sources);
    check_app(&mut ui, &mut app, &mut sources);
    let checked = ui.current_editor_draft(&app, Some(&sources)).unwrap();
    assert_eq!(checked.text, app.open_document().unwrap().text());
    format(&mut ui, &mut app, &mut sources);
    key(&mut ui, &mut app, &mut sources, KeyCode::Enter);
    assert_historical_app_check(&ui, &app, &sources);
    ui.handle_event_with_sources(
        Event::Key(KeyEvent::new(
            KeyCode::Char('z'),
            event::KeyModifiers::CONTROL,
        )),
        &mut app,
        &mut sources,
    );
    let undone = ui.current_editor_draft(&app, Some(&sources)).unwrap();
    assert_eq!(undone.text, checked.text);
    assert_eq!(undone.baseline, checked.baseline);
    assert_ne!(undone.edit_identity, checked.edit_identity);
    assert_historical_app_check(&ui, &app, &sources);
    check_app(&mut ui, &mut app, &mut sources);
}

#[test]
fn app_check_format_cancel_and_exact_noop_preserve_current_evidence() {
    let (_root, mut app, mut ui, mut sources) = fixture();
    check_app(&mut ui, &mut app, &mut sources);
    let checked = ui.current_editor_draft(&app, Some(&sources)).unwrap();
    format(&mut ui, &mut app, &mut sources);
    key(&mut ui, &mut app, &mut sources, KeyCode::Esc);
    assert_eq!(ui.rendered_output(&app, Some(&sources)), "No errors\n");
    assert_eq!(
        ui.apply_reviewed_editor_draft(
            &app,
            Some(&sources),
            &checked,
            &checked.text,
            super::super::DraftApplyDecision::Apply,
        )
        .unwrap(),
        super::super::DraftApplyOutcome::Unchanged
    );
    assert_eq!(ui.rendered_output(&app, Some(&sources)), "No errors\n");
}

#[test]
fn app_check_uses_full_draft_identity_for_cursor_baseline_and_source_changes() {
    for change in ["cursor", "baseline", "source"] {
        let (root, mut app, mut ui, mut sources) = fixture();
        check_app(&mut ui, &mut app, &mut sources);
        let checked = ui.current_editor_draft(&app, Some(&sources)).unwrap();
        match change {
            "cursor" => key(&mut ui, &mut app, &mut sources, KeyCode::Right),
            "baseline" => {
                // Even an identical replacement advances the authoritative revision.
                let range = LineIndex::new(&checked.text)
                    .range_for_span(0..checked.text.len())
                    .unwrap();
                app.open_document_mut()
                    .unwrap()
                    .apply_edit(crate::text::TextEdit::replace(range, &checked.text))
                    .unwrap();
            }
            "source" => {
                root.write_file("other.fmt", &checked.text);
                app.open_path("other.fmt").unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            ui.current_editor_draft(&app, Some(&sources)).unwrap().text,
            checked.text
        );
        assert_historical_app_check(&ui, &app, &sources);
    }
}

#[test]
fn direct_app_diagnostics_cannot_claim_an_unpublished_draft_is_current() {
    let (_root, mut app, mut ui, mut sources) = fixture();
    ui.handle_event_with_sources(Event::Paste("local ".into()), &mut app, &mut sources);
    let pinned = ui.current_editor_draft(&app, Some(&sources)).unwrap();
    assert!(
        ui.invoke_action(
            WorkspaceAction::CheckDocumentDiagnostics,
            &mut app,
            &mut sources,
            None,
        )
        .is_err()
    );
    assert_eq!(
        ui.current_editor_draft(&app, Some(&sources)).unwrap(),
        pinned
    );
    assert!(ui.app_diagnostics.is_none());
}

#[test]
fn app_check_does_not_mislabel_generic_errors_or_unrelated_source_changes() {
    let (_root, mut app, mut ui, mut sources) = fixture();
    check_app(&mut ui, &mut app, &mut sources);
    sources.create("text", "unrelated scratch source");
    let checked = ui.current_editor_draft(&app, Some(&sources)).unwrap();
    ui.register_editable("other-editor", EditableScratchPane::new())
        .unwrap();
    let other = &mut ui.instances.get_mut("other-editor").unwrap().pane;
    other
        .apply_editor_draft(&checked, "unrelated editor draft", true)
        .unwrap();
    assert!(other.has_dirty_draft());
    assert_eq!(ui.rendered_output(&app, Some(&sources)), "No errors\n");
    format(&mut ui, &mut app, &mut sources);
    key(&mut ui, &mut app, &mut sources, KeyCode::Enter);
    assert_historical_app_check(&ui, &app, &sources);
    ui.output_errors.push_str("Action failed\n");
    assert_eq!(ui.rendered_output(&app, Some(&sources)), ui.output_errors);
    ui.output_errors = "Another action failed\n".into();
    assert_eq!(
        ui.rendered_output(&app, Some(&sources)),
        "Another action failed\n"
    );
}
