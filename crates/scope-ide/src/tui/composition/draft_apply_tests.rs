use super::*;
use crate::{
    scratch::ScratchBuffers,
    test_support::TempWorkspace,
    tui::{CheckCurrentness, EditableScratchPane},
};

struct Fixture {
    app: App,
    ui: WorkspaceTui,
    sources: ScratchBuffers,
}

impl Fixture {
    fn new(root: &TempWorkspace) -> Self {
        root.write_file("draft.txt", "original");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("draft.txt").unwrap();
        let mut sources = ScratchBuffers::default();
        sources.create("text", "original");
        let mut ui = WorkspaceTui::new();
        ui.register_editable("editor", EditableScratchPane::new())
            .unwrap();
        ui.layout("main", View::leaf("editor")).unwrap();
        ui.geometry(Rect::new(0, 0, 100, 25));
        ui.select_editor("editor", &app, Some(&sources)).unwrap();
        Self { app, ui, sources }
    }

    fn draft(&self) -> EditorDraftSnapshot {
        self.ui
            .current_editor_draft(&self.app, Some(&self.sources))
            .unwrap()
    }

    fn apply(
        &mut self,
        pinned: &EditorDraftSnapshot,
        text: &str,
    ) -> Result<DraftApplyOutcome, String> {
        self.ui.apply_reviewed_editor_draft(
            &self.app,
            Some(&self.sources),
            pinned,
            text,
            DraftApplyDecision::Apply,
        )
    }

    fn key(&mut self, code: KeyCode, modifiers: event::KeyModifiers) {
        self.ui.handle_event_with_sources(
            Event::Key(KeyEvent::new(code, modifiers)),
            &mut self.app,
            &mut self.sources,
        );
    }

    fn control(&mut self, ch: char) {
        self.key(KeyCode::Char(ch), event::KeyModifiers::CONTROL);
    }

    fn paste(&mut self, text: &str) {
        self.ui.handle_event_with_sources(
            Event::Paste(text.into()),
            &mut self.app,
            &mut self.sources,
        );
    }

    fn reject(&mut self, pinned: &EditorDraftSnapshot) {
        let before = self.draft();
        assert!(self.apply(pinned, "must not appear").is_err());
        assert_eq!(self.draft(), before);
    }
}

fn assert_edit_state(actual: &EditorDraftSnapshot, expected: &EditorDraftSnapshot) {
    assert_eq!(actual.text, expected.text);
    assert_eq!(actual.cursor, expected.cursor);
    assert_eq!(actual.anchor, expected.anchor);
    assert_eq!(actual.baseline, expected.baseline);
}

#[test]
fn apply_is_one_exact_undo_redo_transaction_without_publication() {
    let root = TempWorkspace::new("draft-apply-transaction");
    let mut h = Fixture::new(&root);
    let committed = h.app.open_document().unwrap().clone();
    let source = h.sources.selected().unwrap().version();
    h.paste("local é\n");
    h.key(KeyCode::Left, event::KeyModifiers::SHIFT);
    let pinned = h.draft();
    assert!(pinned.anchor.is_some());
    assert_eq!(
        h.apply(&pinned, "新しい\n🙂\n").unwrap(),
        DraftApplyOutcome::Applied
    );
    let applied = h.draft();
    assert_eq!(applied.text, "新しい\n🙂\n");
    assert_eq!(applied.anchor, None);
    assert_ne!(applied.edit_identity, pinned.edit_identity);
    h.control('z');
    assert_edit_state(&h.draft(), &pinned);
    h.control('y');
    assert_edit_state(&h.draft(), &applied);
    h.control('z');
    h.control('z');
    assert_eq!(h.draft().text, "original");
    assert_eq!(h.sources.selected().unwrap().version(), source);
    assert_eq!(h.sources.selected().unwrap().text(), "original");
    assert_eq!(h.app.open_document().unwrap(), &committed);
    assert_eq!(root.read_file("draft.txt"), "original");
}

#[test]
fn app_owned_apply_is_local_and_undoable_without_touching_either_source() {
    let root = TempWorkspace::new("draft-apply-app-owner");
    let mut h = Fixture::new(&root);
    h.ui.set_app_editor(true);
    h.ui.select_editor("editor", &h.app, Some(&h.sources))
        .unwrap();
    let committed = h.app.open_document().unwrap().clone();
    let pinned = h.draft();
    assert_eq!(
        h.apply(&pinned, "app local\né").unwrap(),
        DraftApplyOutcome::Applied
    );
    let applied = h.draft();
    assert_eq!(applied.text, "app local\né");
    h.control('z');
    assert_edit_state(&h.draft(), &pinned);
    h.control('y');
    assert_edit_state(&h.draft(), &applied);
    assert_eq!(h.app.open_document().unwrap(), &committed);
    assert_eq!(h.sources.selected().unwrap().text(), "original");
    assert_eq!(root.read_file("draft.txt"), "original");
}

#[test]
fn apply_clears_redo_and_separates_subsequent_typing() {
    let root = TempWorkspace::new("draft-apply-redo");
    let mut h = Fixture::new(&root);
    h.paste("discarded ");
    h.control('z');
    let pinned = h.draft();
    h.apply(&pinned, "replacement").unwrap();
    let applied = h.draft();
    h.control('y');
    assert_eq!(h.draft(), applied);
    h.key(KeyCode::Char('x'), event::KeyModifiers::NONE);
    h.control('z');
    assert_edit_state(&h.draft(), &applied);
    h.control('z');
    assert_edit_state(&h.draft(), &pinned);
}

#[test]
fn noop_and_cancel_preserve_identity_and_both_history_stacks() {
    let root = TempWorkspace::new("draft-apply-inert");
    let mut h = Fixture::new(&root);
    h.paste("first ");
    let first = h.draft();
    h.paste("second ");
    let second = h.draft();
    h.control('z');
    let pinned = h.draft();
    assert_eq!(
        h.apply(&pinned, &pinned.text).unwrap(),
        DraftApplyOutcome::Unchanged
    );
    assert_eq!(h.draft(), pinned);
    assert_eq!(
        h.ui.apply_reviewed_editor_draft(
            &h.app,
            Some(&h.sources),
            &second,
            "ignored",
            DraftApplyDecision::Cancel,
        )
        .unwrap(),
        DraftApplyOutcome::Cancelled
    );
    assert_eq!(h.draft(), pinned);
    h.control('y');
    assert_edit_state(&h.draft(), &second);
    h.control('z');
    assert_edit_state(&h.draft(), &first);
    h.control('z');
    assert_eq!(h.draft().text, "original");
}

#[test]
fn typing_undo_textual_restore_and_redo_all_reject_old_pin_without_history_changes() {
    let root = TempWorkspace::new("draft-apply-stale-edit");
    let mut h = Fixture::new(&root);
    let pinned = h.draft();
    h.paste("typed ");
    let typed = h.draft();
    h.reject(&pinned);
    h.control('z');
    assert_eq!(h.draft().text, pinned.text);
    assert_ne!(h.draft().edit_identity, pinned.edit_identity);
    h.reject(&pinned);
    h.control('y');
    assert_edit_state(&h.draft(), &typed);
    h.reject(&pinned);
    h.control('z');
    assert_edit_state(&h.draft(), &pinned);
}

#[test]
fn cursor_and_selection_only_changes_reject_without_adding_history() {
    for modifiers in [event::KeyModifiers::NONE, event::KeyModifiers::SHIFT] {
        let root = TempWorkspace::new("draft-apply-stale-selection");
        let mut h = Fixture::new(&root);
        h.paste("prefix ");
        let pinned = h.draft();
        h.key(KeyCode::Left, modifiers);
        let moved = h.draft();
        assert_eq!(moved.text, pinned.text);
        assert_eq!(moved.edit_identity, pinned.edit_identity);
        assert_ne!(moved.cursor, pinned.cursor);
        if modifiers == event::KeyModifiers::SHIFT {
            assert_ne!(moved.anchor, pinned.anchor);
        }
        h.reject(&pinned);
        h.control('z');
        assert_eq!(h.draft().text, "original");
    }
}

#[test]
fn source_switch_and_unobserved_round_trip_reject_old_pin() {
    let root = TempWorkspace::new("draft-apply-source");
    let mut h = Fixture::new(&root);
    let first = h.sources.selected().unwrap().id();
    let pinned = h.draft();
    h.sources.create("text", "original");
    assert!(h.apply(&pinned, "wrong source").is_err());
    assert_eq!(h.sources.selected().unwrap().text(), "original");
    h.sources.select(first).unwrap();
    h.reject(&pinned);
    assert_eq!(h.draft().text, pinned.text);
    assert_ne!(
        h.draft().source_selection_identity,
        pinned.source_selection_identity
    );
}

#[test]
fn app_document_switch_and_reopen_reject_byte_identical_pin() {
    let root = TempWorkspace::new("draft-apply-app-switch");
    let mut h = Fixture::new(&root);
    h.ui.set_app_editor(true);
    let pinned = h.draft();
    root.write_file("other.txt", &pinned.text);
    h.app.open_path("other.txt").unwrap();
    h.reject(&pinned);
    h.app.open_path("draft.txt").unwrap();
    assert_eq!(h.draft().text, pinned.text);
    h.reject(&pinned);
}

#[test]
fn editor_replacement_and_rebinding_reject_equal_text_pins() {
    let root = TempWorkspace::new("draft-apply-generation");
    let mut h = Fixture::new(&root);
    let pinned = h.draft();
    h.ui.instances.remove("editor");
    h.ui.register_editable("editor", EditableScratchPane::new())
        .unwrap();
    assert!(h.apply(&pinned, "wrong generation").is_err());
    h.ui.select_editor("editor", &h.app, Some(&h.sources))
        .unwrap();
    assert_ne!(h.draft().editor_generation, pinned.editor_generation);
    h.reject(&pinned);
    let rebound = h.draft();
    h.ui.set_app_editor(true);
    h.ui.select_editor("editor", &h.app, Some(&h.sources))
        .unwrap();
    assert!(h.apply(&rebound, "wrong owner").is_err());
    h.ui.set_app_editor(false);
    h.ui.select_editor("editor", &h.app, Some(&h.sources))
        .unwrap();
    h.reject(&rebound);
}

#[test]
fn utf8_multiline_and_empty_replacements_round_trip_exactly() {
    let root = TempWorkspace::new("draft-apply-unicode");
    let mut h = Fixture::new(&root);
    for replacement in ["é中🙂\nsecond\n", "", "\n", "last"] {
        let pinned = h.draft();
        assert_eq!(
            h.apply(&pinned, replacement).unwrap(),
            DraftApplyOutcome::Applied
        );
        let applied = h.draft();
        assert_eq!(applied.text, replacement);
        assert!(replacement.is_char_boundary(applied.cursor));
        h.control('z');
        assert_edit_state(&h.draft(), &pinned);
        h.control('y');
        assert_edit_state(&h.draft(), &applied);
    }
}

#[test]
fn empty_document_and_cursor_inside_new_grapheme_have_exact_history() {
    let root = TempWorkspace::new("draft-apply-empty-cursor");
    let mut h = Fixture::new(&root);
    h.sources.create("text", "");
    let empty = h.draft();
    h.apply(&empty, "é\n猫").unwrap();
    assert_eq!(h.draft().cursor, 0);
    h.control('z');
    assert_edit_state(&h.draft(), &empty);
    h.control('y');
    assert_eq!(h.draft().text, "é\n猫");

    h.sources.create("text", "\u{301}");
    h.paste("e");
    let combined = h.draft();
    // Inserting before a combining mark leaves an exact UTF-8 byte cursor
    // inside the newly formed grapheme. Capture must not silently remap it.
    assert_eq!(combined.text, "e\u{301}");
    assert_eq!(combined.cursor, 1);
    h.apply(&combined, "猫\n").unwrap();
    assert_eq!(h.draft().cursor, 0);
    h.control('z');
    assert_edit_state(&h.draft(), &combined);
}

struct AnalysisHost;
impl crate::cli::ReadOnlyCommand for AnalysisHost {
    fn invoke(
        &mut self,
        _: &crate::document::Document,
        _: crate::text::TextRange,
        _: &str,
        _: &mut dyn std::io::Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("draft analysis must not execute");
    }

    fn check_analysis(
        &mut self,
        _: &DownstreamAction,
        _: &mut crate::analysis::AnalysisContext<'_>,
        _: &mut dyn std::io::Write,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn apply_retains_check_evidence_but_undo_does_not_restore_freshness() {
    let root = TempWorkspace::new("draft-apply-evidence");
    let mut h = Fixture::new(&root);
    h.ui.register_downstream_action(DownstreamAction {
        id: "inspect".into(),
        label: "Inspect".into(),
        language: "text".into(),
        namespace: "native".into(),
        command: "inspect".into(),
        args: vec![],
    })
    .unwrap();
    h.ui.set_downstream_check_action(Some("inspect".into()));
    let pinned = h.draft();
    h.ui.check_pinned_editor_draft(&h.app, &h.sources, &pinned, &mut AnalysisHost)
        .unwrap();
    let invocation = h.ui.check_evidence().unwrap().invocation_id();
    assert_eq!(
        h.ui.check_presentation(&h.app, Some(&h.sources)).status(),
        CheckCurrentness::Current
    );
    h.apply(&pinned, &pinned.text).unwrap();
    assert_eq!(
        h.ui.check_presentation(&h.app, Some(&h.sources)).status(),
        CheckCurrentness::Current
    );
    h.apply(&pinned, "reviewed").unwrap();
    for undo in [false, true] {
        if undo {
            h.control('z');
            assert_eq!(h.draft().text, pinned.text);
        }
        assert_eq!(h.ui.check_evidence().unwrap().invocation_id(), invocation);
        assert_eq!(
            h.ui.check_presentation(&h.app, Some(&h.sources)).status(),
            CheckCurrentness::ValidationUnavailable
        );
    }
}
