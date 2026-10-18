use scope_ide::{
    document::{Document, DocumentId},
    proposal::{ProposalError, ProposalId, ProposalStatus, SourceEditProposal},
    scratch::ScratchBuffers,
    source_edit::{ControllerError, DocumentSource, SourceEditController, SourceId, SourceTarget},
    text::{Position, TextEdit, TextRange},
};

fn edit(start: usize, end: usize, replacement: &str) -> TextEdit {
    TextEdit::replace(
        TextRange::new(Position::new(0, start), Position::new(0, end)),
        replacement,
    )
}

fn validation(error: ControllerError, predicate: impl Fn(&ProposalError) -> bool) {
    match error {
        ControllerError::Validation(errors) => assert!(errors.iter().any(predicate), "{errors:?}"),
        other => panic!("expected validation error, got {other:?}"),
    }
}

#[test]
fn scratch_review_is_pure_and_acceptance_is_one_atomic_revision() {
    let mut buffers = ScratchBuffers::default();
    let target = buffers.create("rust", "one two\n");
    let mut controller = SourceEditController::default();
    let id = controller
        .propose(
            buffers.get(target).unwrap(),
            vec![edit(4, 7, "β"), edit(0, 3, "α")],
            Some("rename both".into()),
        )
        .unwrap();
    let proposal = controller.get(id).unwrap();
    assert_eq!(proposal.target(), SourceId::Scratch(target));
    assert_eq!(proposal.expected_version(), 0);
    assert_eq!(proposal.expected_source(), "one two\n");
    assert_eq!(proposal.rationale(), Some("rename both"));
    assert_eq!(proposal.status(), ProposalStatus::Pending);
    assert_eq!(
        controller.list().map(|p| p.id()).collect::<Vec<_>>(),
        vec![id]
    );
    let buffer = buffers.get(target).unwrap();
    assert_eq!(controller.preview(id, buffer).unwrap(), "α β\n");
    let diff = controller.diff(id, buffer).unwrap();
    assert!(diff.starts_with("--- before\n+++ after\n"));
    assert!(diff.contains("-one two\n+α β\n"));
    for budget in [0, 1, diff.find('α').unwrap() + 1, diff.len()] {
        let bounded = controller.diff_bounded(id, buffer, budget).unwrap();
        assert!(bounded.text.len() <= budget);
        assert!(diff.starts_with(&bounded.text));
        assert_eq!(bounded.truncated, budget < diff.len());
    }
    assert_eq!(buffer.text(), "one two\n");
    assert_eq!(buffer.version(), 0);
    assert!(buffer.is_dirty());
    controller
        .accept(id, buffers.get_mut(target).unwrap())
        .unwrap();
    let buffer = buffers.get(target).unwrap();
    assert_eq!(buffer.text(), "α β\n");
    assert_eq!(buffer.version(), 1);
    assert!(buffer.is_dirty());
    assert_eq!(
        controller.get(id).unwrap().status(),
        ProposalStatus::Applied
    );
    validation(
        controller
            .accept(id, buffers.get_mut(target).unwrap())
            .unwrap_err(),
        |e| matches!(e, ProposalError::Applied),
    );
    controller.reject(id).unwrap();
    assert_eq!(
        controller.get(id).unwrap().status(),
        ProposalStatus::Applied
    );
    assert_eq!(buffers.get(target).unwrap().version(), 1);
}

#[test]
fn rejection_is_terminal_and_unknown_ids_are_reported() {
    let mut buffers = ScratchBuffers::default();
    let target = buffers.create("text", "abc");
    let mut controller = SourceEditController::default();
    let id = controller
        .propose(buffers.get(target).unwrap(), vec![edit(0, 1, "x")], None)
        .unwrap();
    controller.reject(id).unwrap();
    controller.reject(id).unwrap();
    assert_eq!(
        controller.get(id).unwrap().status(),
        ProposalStatus::Rejected
    );
    validation(
        controller
            .preview(id, buffers.get(target).unwrap())
            .unwrap_err(),
        |e| matches!(e, ProposalError::Rejected),
    );
    validation(
        controller
            .accept(id, buffers.get_mut(target).unwrap())
            .unwrap_err(),
        |e| matches!(e, ProposalError::Rejected),
    );
    assert_eq!(buffers.get(target).unwrap().text(), "abc");
    assert_eq!(buffers.get(target).unwrap().version(), 0);
    assert!(buffers.get(target).unwrap().is_dirty());
    let unknown = ProposalId(999);
    assert!(
        matches!(controller.get(unknown), Err(ControllerError::UnknownProposal(id)) if id == unknown)
    );
    assert!(
        matches!(controller.reject(unknown), Err(ControllerError::UnknownProposal(id)) if id == unknown)
    );
    assert!(
        matches!(controller.accept(unknown, buffers.get_mut(target).unwrap()), Err(ControllerError::UnknownProposal(id)) if id == unknown)
    );
}

#[test]
fn source_snapshot_and_content_conflict_preserve_original_state() {
    let id = DocumentId::new(12);
    let mut original = Document::new(id, "file.txt", "abc");
    let target = DocumentSource::new(&mut original, "text");
    let snapshot = target.snapshot();
    assert_eq!(snapshot.id, SourceId::Document(id));
    assert_eq!(snapshot.version, 0);
    assert_eq!(snapshot.language, "text");
    assert_eq!(snapshot.text, "abc");
    let mut controller = SourceEditController::default();
    let proposal = controller
        .propose(&target, vec![edit(0, 1, "x")], None)
        .unwrap();
    assert_eq!(
        controller.get(proposal).unwrap().to_json()["target"]["kind"],
        "document"
    );
    let mut different = Document::new(id, "file.txt", "def");
    let mut different_target = DocumentSource::new(&mut different, "text");
    validation(
        controller
            .accept(proposal, &mut different_target)
            .unwrap_err(),
        |e| matches!(e, ProposalError::SourceConflict),
    );
    assert_eq!(different_target.text(), "def");
    assert_eq!(different_target.version(), 0);
    assert_eq!(
        controller.get(proposal).unwrap().status(),
        ProposalStatus::Pending
    );
    assert_eq!(original.text(), "abc");
    assert!(!original.is_dirty());
    assert!(!different.is_dirty());
}

#[test]
fn registration_rejects_duplicates_without_replacing_original() {
    let mut document = Document::new(DocumentId::new(7), "file.rs", "abc");
    let target = DocumentSource::new(&mut document, "rust");
    let mut controller = SourceEditController::default();
    let id = ProposalId(40);
    let original =
        SourceEditProposal::new(id, &target, vec![edit(0, 1, "x")], Some("original".into()))
            .unwrap();
    assert_eq!(controller.register(original, &target).unwrap(), id);
    let duplicate = SourceEditProposal::new(id, &target, vec![edit(0, 1, "y")], None).unwrap();
    assert!(
        matches!(controller.register(duplicate, &target), Err(ControllerError::DuplicateProposal(found)) if found == id)
    );
    assert_eq!(controller.preview(id, &target).unwrap(), "xbc");
    assert_eq!(controller.get(id).unwrap().rationale(), Some("original"));
    assert_eq!(
        controller
            .propose(&target, vec![edit(1, 2, "z")], None)
            .unwrap(),
        ProposalId(41)
    );
    assert_eq!(controller.list().count(), 2);
    assert_eq!(target.text(), "abc");
    assert_eq!(target.version(), 0);
}

#[test]
fn scratch_and_document_with_same_raw_id_are_independent_targets() {
    let mut buffers = ScratchBuffers::default();
    let raw = buffers.create("text", "abc");
    let mut document = Document::new(DocumentId::new(raw), "file.txt", "abc");
    let mut target = DocumentSource::new(&mut document, "text");
    let scratch = buffers.get(raw).unwrap();
    assert_eq!(scratch.source_id().raw(), target.source_id().raw());
    assert_ne!(scratch.source_id(), target.source_id());
    let mut controller = SourceEditController::default();
    let a = controller
        .propose(scratch, vec![edit(0, 1, "s")], None)
        .unwrap();
    let b = controller
        .propose(&target, vec![edit(0, 1, "d")], None)
        .unwrap();
    validation(controller.preview(a, &target).unwrap_err(), |e| {
        matches!(e, ProposalError::WrongTarget { .. })
    });
    validation(controller.diff(b, scratch).unwrap_err(), |e| {
        matches!(e, ProposalError::WrongTarget { .. })
    });
    validation(controller.accept(a, &mut target).unwrap_err(), |e| {
        matches!(e, ProposalError::WrongTarget { .. })
    });
    validation(
        controller
            .accept(b, buffers.get_mut(raw).unwrap())
            .unwrap_err(),
        |e| matches!(e, ProposalError::WrongTarget { .. }),
    );
    assert_eq!(controller.get(a).unwrap().status(), ProposalStatus::Pending);
    assert_eq!(controller.get(b).unwrap().status(), ProposalStatus::Pending);
    assert_eq!(target.version(), 0);
    assert_eq!(buffers.get(raw).unwrap().version(), 0);
    controller.accept(a, buffers.get_mut(raw).unwrap()).unwrap();
    assert_eq!(controller.preview(b, &target).unwrap(), "dbc");
    controller.accept(b, &mut target).unwrap();
    assert_eq!(target.text(), "dbc");
    assert_eq!(buffers.get(raw).unwrap().text(), "sbc");
}

#[test]
fn stale_revisions_fail_even_when_source_text_is_unchanged() {
    let mut buffers = ScratchBuffers::default();
    let raw = buffers.create("text", "abc");
    let mut document = Document::new(DocumentId::new(9), "file.txt", "abc");
    let mut target = DocumentSource::new(&mut document, "text");
    let mut controller = SourceEditController::default();
    let a = controller
        .propose(buffers.get(raw).unwrap(), vec![edit(0, 1, "x")], None)
        .unwrap();
    let b = controller
        .propose(&target, vec![edit(0, 1, "y")], None)
        .unwrap();
    buffers
        .get_mut(raw)
        .unwrap()
        .apply_edit(edit(0, 1, "a"))
        .unwrap();
    target.apply_source_edit(edit(0, 1, "a")).unwrap();
    validation(
        controller
            .preview(a, buffers.get(raw).unwrap())
            .unwrap_err(),
        |e| {
            matches!(
                e,
                ProposalError::RevisionConflict {
                    expected: 0,
                    actual: 1
                }
            )
        },
    );
    validation(controller.diff_bounded(b, &target, 100).unwrap_err(), |e| {
        matches!(e, ProposalError::RevisionConflict { .. })
    });
    validation(
        controller
            .accept(a, buffers.get_mut(raw).unwrap())
            .unwrap_err(),
        |e| matches!(e, ProposalError::RevisionConflict { .. }),
    );
    validation(controller.accept(b, &mut target).unwrap_err(), |e| {
        matches!(e, ProposalError::RevisionConflict { .. })
    });
    assert_eq!(target.text(), "abc");
    assert_eq!(target.version(), 1);
    assert_eq!(buffers.get(raw).unwrap().text(), "abc");
    assert_eq!(buffers.get(raw).unwrap().version(), 1);
    assert!(
        controller
            .list()
            .all(|p| p.status() == ProposalStatus::Pending)
    );
}

#[test]
fn invalid_and_overlapping_batches_never_register_or_partially_edit() {
    let mut document = Document::new(DocumentId::new(1), "file.txt", "aβcd");
    let before = document.clone();
    let target = DocumentSource::new(&mut document, "text");
    let mut controller = SourceEditController::default();
    for edits in [
        vec![],
        vec![edit(0, 1, "valid"), edit(2, 3, "inside utf8")],
        vec![edit(0, 1, "valid"), edit(4, 99, "outside")],
        vec![edit(0, 3, "first"), edit(1, 4, "overlap")],
    ] {
        let error = controller.propose(&target, edits, None).unwrap_err();
        validation(error, |e| {
            matches!(
                e,
                ProposalError::EmptyEdits
                    | ProposalError::InvalidEdit { .. }
                    | ProposalError::OverlappingEdits { .. }
            )
        });
        assert_eq!(controller.list().count(), 0);
        assert_eq!(target.text(), "aβcd");
        assert_eq!(target.version(), 0);
    }
    let invalid = SourceEditProposal::new(
        ProposalId(50),
        &target,
        vec![edit(0, 4, "x"), edit(3, 5, "y")],
        None,
    )
    .unwrap();
    validation(controller.register(invalid, &target).unwrap_err(), |e| {
        matches!(e, ProposalError::OverlappingEdits { .. })
    });
    assert_eq!(
        controller
            .propose(&target, vec![edit(0, 1, "x")], None)
            .unwrap(),
        ProposalId(1)
    );
    assert_eq!(document, before);
}

#[test]
fn dirty_document_acceptance_preserves_saved_baseline_and_does_not_save() {
    // A real backing file is deliberately not exposed as a save capability.
    let path = std::env::temp_dir().join(format!(
        "scope-source-edit-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let fixture = Fixture(path);
    std::fs::write(&fixture.0, "saved").unwrap();
    let mut document = Document::new(
        DocumentId::new(3),
        fixture.0.file_name().unwrap().to_str().unwrap(),
        std::fs::read_to_string(&fixture.0).unwrap(),
    );
    document.apply_edit(edit(0, 5, "baseline")).unwrap();
    document.mark_saved_revision("disk-baseline");
    std::fs::write(&fixture.0, "baseline").unwrap();
    document.apply_edit(edit(0, 8, "dirty")).unwrap();
    let saved_version = document.saved_version();
    let version = document.version();
    let before = document.clone();
    let mut controller = SourceEditController::default();
    {
        let mut target = DocumentSource::new(&mut document, "text");
        let id = controller
            .propose(&target, vec![edit(0, 1, "D"), edit(1, 5, "IRTY")], None)
            .unwrap();
        assert_eq!(controller.preview(id, &target).unwrap(), "DIRTY");
        controller.diff(id, &target).unwrap();
        assert_eq!(std::fs::read_to_string(&fixture.0).unwrap(), "baseline");
        controller.accept(id, &mut target).unwrap();
    }
    assert_eq!(before.text(), "dirty");
    assert_eq!(document.text(), "DIRTY");
    assert_eq!(document.version(), version + 1);
    assert_eq!(document.saved_version(), saved_version);
    assert_eq!(document.disk_revision(), Some("disk-baseline"));
    assert!(document.is_dirty());
    assert_eq!(std::fs::read_to_string(&fixture.0).unwrap(), "baseline");
    document.revert();
    assert_eq!(document.text(), "baseline");
    assert!(!document.is_dirty());
}

#[cfg(feature = "agent")]
mod generation {
    use super::*;
    use scope_ide::{
        agent::{DecisionMessage, DecisionModel},
        agent_edit::{SourceEditError, SourceEditLimits},
    };
    use serde_json::{Value, json};

    #[derive(Default)]
    struct MockModel {
        envelopes: Vec<Value>,
        invalid: bool,
    }

    impl DecisionModel for MockModel {
        fn decide(
            &mut self,
            messages: &[DecisionMessage],
            max_tokens: u32,
        ) -> Result<String, String> {
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0].role, "system");
            assert_eq!(messages[1].role, "user");
            assert_eq!(max_tokens, 123);
            let envelope: Value = serde_json::from_str(&messages[1].content).unwrap();
            assert_eq!(envelope["instruction"], "replace first character");
            assert_eq!(envelope["language"], "rust");
            let response = json!({
                "type": "source_edit",
                "target_buffer": envelope["target_buffer"],
                "expected_version": envelope["expected_version"],
                "expected_source": envelope["source"],
                "edits": [{"range": {"start": {"line": 0, "column": 0}, "end": {"line": 0, "column": 1}}, "replacement": "X"}],
                "rationale": "deterministic mock"
            });
            self.envelopes.push(envelope);
            Ok(if self.invalid {
                "not JSON".into()
            } else {
                response.to_string()
            })
        }
    }

    fn limits() -> SourceEditLimits {
        SourceEditLimits {
            max_tokens: 123,
            ..SourceEditLimits::default()
        }
    }

    #[test]
    fn generation_reads_live_scratch_and_document_envelopes_without_applying() {
        let mut buffers = ScratchBuffers::default();
        let raw = buffers.create("rust", "abc");
        buffers
            .get_mut(raw)
            .unwrap()
            .apply_edit(edit(0, 1, "d"))
            .unwrap();
        let mut document = Document::new(DocumentId::new(raw), "file.rs", "abc");
        document.apply_edit(edit(0, 1, "e")).unwrap();
        let before = document.clone();
        let mut controller = SourceEditController::default();
        let mut model = MockModel::default();
        let a = controller
            .generate(
                &mut model,
                buffers.get(raw).unwrap(),
                "replace first character",
                limits(),
            )
            .unwrap();
        let mut target = DocumentSource::new(&mut document, "rust");
        let b = controller
            .generate(&mut model, &target, "replace first character", limits())
            .unwrap();
        assert_ne!(a, b);
        assert_eq!(model.envelopes.len(), 2);
        assert_eq!(model.envelopes[0]["source"], "dbc");
        assert_eq!(model.envelopes[1]["source"], "ebc");
        for envelope in &model.envelopes {
            assert_eq!(envelope["target_buffer"], raw);
            assert_eq!(envelope["expected_version"], 1);
        }
        assert_eq!(controller.get(a).unwrap().target(), SourceId::Scratch(raw));
        assert_eq!(
            controller.get(b).unwrap().target(),
            SourceId::Document(DocumentId::new(raw))
        );
        assert!(
            controller
                .list()
                .all(|p| p.status() == ProposalStatus::Pending)
        );
        assert_eq!(
            controller.preview(a, buffers.get(raw).unwrap()).unwrap(),
            "Xbc"
        );
        assert_eq!(controller.preview(b, &target).unwrap(), "Xbc");
        assert_eq!(target.text(), "ebc");
        assert_eq!(target.version(), 1);
        assert_eq!(buffers.get(raw).unwrap().text(), "dbc");
        assert_eq!(buffers.get(raw).unwrap().version(), 1);
        controller.accept(b, &mut target).unwrap();
        assert_eq!(document.saved_version(), before.saved_version());
        assert_eq!(document.version(), 2);
        assert_eq!(document.text(), "Xbc");
        assert!(document.is_dirty());
        controller.accept(a, buffers.get_mut(raw).unwrap()).unwrap();
        assert_eq!(buffers.get(raw).unwrap().version(), 2);
    }

    #[test]
    fn invalid_model_output_leaves_registry_and_document_unchanged() {
        let mut document = Document::new(DocumentId::new(12), "file.rs", "abc");
        let before = document.clone();
        let target = DocumentSource::new(&mut document, "rust");
        let mut controller = SourceEditController::default();
        let mut model = MockModel {
            invalid: true,
            ..MockModel::default()
        };
        assert!(matches!(
            controller.generate(&mut model, &target, "replace first character", limits()),
            Err(ControllerError::Generation(SourceEditError::InvalidJson(_)))
        ));
        assert_eq!(model.envelopes.len(), 1);
        assert_eq!(controller.list().count(), 0);
        model.invalid = false;
        assert_eq!(
            controller
                .generate(&mut model, &target, "replace first character", limits())
                .unwrap(),
            ProposalId(1)
        );
        assert_eq!(document, before);
    }
}
