use super::*;
use crate::{document::Document, text::TextRange};

#[test]
fn compact_refuses_pending_or_unsupported_without_mutation() {
    let mut context = ScriptContext::default();
    let mut buffers = ScratchBuffers::default();
    submit(&mut context, &mut buffers, ":scope script enter").unwrap();
    let mut proposals = super::super::source_proposals::Proposals::default();
    let baseline = buffers.selected().unwrap().version();
    let error = context
        .compact(&mut proposals, &mut buffers, Some(&Host), &mut Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("unsupported"));
    submit(&mut context, &mut buffers, "(").unwrap();
    let error = context
        .compact(&mut proposals, &mut buffers, None, &mut Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("pending"));
    assert_eq!(buffers.selected().unwrap().version(), baseline);
    assert_eq!(proposals.controller_mut().list().count(), 0);
}

struct Host;
impl ReadOnlyCommand for Host {
    fn supports_direct_script(&self) -> bool {
        true
    }
    fn admit_script_snapshot(&self, source: &str) -> ScriptSnapshotAdmission {
        if source.contains("invalid") {
            ScriptSnapshotAdmission::Invalid("invalid source".into())
        } else if source.ends_with('(') {
            ScriptSnapshotAdmission::Incomplete
        } else if source.contains("print") {
            ScriptSnapshotAdmission::ExplicitRunRequired
        } else {
            ScriptSnapshotAdmission::CompletePure
        }
    }
    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
fn submit(
    context: &mut ScriptContext,
    buffers: &mut ScratchBuffers,
    line: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    context.route(line, buffers, Some(&Host), &mut Vec::new())
}
#[test]
fn routing_purity_effect_replay_and_leave() {
    let mut context = ScriptContext::default();
    let mut buffers = ScratchBuffers::default();
    assert_eq!(
        submit(&mut context, &mut buffers, "help").unwrap(),
        Some("help".into())
    );
    submit(&mut context, &mut buffers, ":scope script enter").unwrap();
    assert_eq!(buffers.selected().unwrap().language_id(), "scope-script");
    assert_eq!(
        submit(&mut context, &mut buffers, "1").unwrap(),
        Some(":scope script run".into())
    );
    assert_eq!(
        submit(&mut context, &mut buffers, "2").unwrap(),
        Some(":scope script run".into())
    );
    assert_eq!(buffers.selected().unwrap().text(), "1;\n2");
    assert_eq!(
        submit(&mut context, &mut buffers, "print(3)").unwrap(),
        None
    );
    assert_eq!(submit(&mut context, &mut buffers, "4").unwrap(), None);
    assert_eq!(
        submit(&mut context, &mut buffers, ":run").unwrap(),
        Some(":scope script run".into())
    );
    assert_eq!(
        submit(&mut context, &mut buffers, ":edit").unwrap(),
        Some(":scope edit".into())
    );
    submit(&mut context, &mut buffers, ":scope script leave").unwrap();
    assert_eq!(
        submit(&mut context, &mut buffers, "quit").unwrap(),
        Some("quit".into())
    );
}
#[test]
fn pending_is_transactional_pinned_and_cancelable() {
    let mut context = ScriptContext::default();
    let mut buffers = ScratchBuffers::default();
    submit(&mut context, &mut buffers, ":scope script enter").unwrap();
    submit(&mut context, &mut buffers, "(").unwrap();
    assert_eq!(buffers.selected().unwrap().version(), 0);
    assert!(submit(&mut context, &mut buffers, "invalid").is_err());
    assert_eq!(context.prompt(), "script> ");
    assert_eq!(buffers.selected().unwrap().version(), 0);
    submit(&mut context, &mut buffers, "(").unwrap();
    assert!(submit(&mut context, &mut buffers, ":run").is_err());
    assert_eq!(
        submit(&mut context, &mut buffers, ")").unwrap(),
        Some(":scope script run".into())
    );
    assert_eq!(buffers.selected().unwrap().text(), "(\n)");
    submit(&mut context, &mut buffers, "(").unwrap();
    let id = buffers.selected().unwrap().id();
    buffers.create("scope-script", "other");
    assert!(submit(&mut context, &mut buffers, ")").is_err());
    buffers.select(id).unwrap();
    let source = buffers.selected().unwrap();
    let range = TextBuffer::new(source.text())
        .range_for_span(Span::new(0, source.text().len()))
        .unwrap();
    buffers
        .selected_mut()
        .unwrap()
        .apply_edit(TextEdit::replace(range, "changed"))
        .unwrap();
    assert!(submit(&mut context, &mut buffers, ")").is_err());
    submit(&mut context, &mut buffers, ":cancel").unwrap();
    submit(&mut context, &mut buffers, "5").unwrap();
    assert_eq!(buffers.selected().unwrap().text(), "changed;\n5");
}
#[test]
fn prompts_commands_and_completion_preserve_pending_revision() {
    let mut context = ScriptContext::default();
    let mut buffers = ScratchBuffers::default();
    assert!(!context.is_active());
    submit(&mut context, &mut buffers, ":scope script enter").unwrap();
    assert!(context.is_active());
    assert_eq!(context.prompt(), "script> ");
    let completions = context.complete(":sho", 4, &buffers, Some(&Host));
    assert!(
        completions
            .iter()
            .any(|item| item.insert_text == "show" && item.replacement == (1..4))
    );
    assert!(context.complete("x", 2, &buffers, Some(&Host)).is_empty());
    submit(&mut context, &mut buffers, "(").unwrap();
    assert_eq!(context.prompt(), "...> ");
    assert_eq!(submit(&mut context, &mut buffers, ":show").unwrap(), None);
    assert_eq!(context.prompt(), "...> ");
    assert_eq!(buffers.selected().unwrap().version(), 0);
    submit(&mut context, &mut buffers, ":cancel").unwrap();
    assert_eq!(context.prompt(), "script> ");
    assert_eq!(buffers.selected().unwrap().text(), "");
    submit(&mut context, &mut buffers, ":script leave").unwrap();
    assert!(!context.is_active());
}

#[test]
fn pending_show_is_separate_read_only_and_revision_guarded() {
    let mut context = ScriptContext::default();
    let mut buffers = ScratchBuffers::default();
    submit(&mut context, &mut buffers, ":scope script enter").unwrap();
    submit(&mut context, &mut buffers, "1 # keep").unwrap();
    let version = buffers.selected().unwrap().version();
    submit(&mut context, &mut buffers, "(").unwrap();
    for command in [":show", ":scope show"] {
        let mut output = Vec::new();
        assert_eq!(
            context
                .route(command, &mut buffers, Some(&Host), &mut output)
                .unwrap(),
            None
        );
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "Committed Script source:\n1 # keep\nPending Script snapshot (not committed; not evaluated):\n1 # keep\n;\n(\n"
        );
        assert_eq!(buffers.selected().unwrap().version(), version);
        assert_eq!(buffers.selected().unwrap().text(), "1 # keep");
        assert_eq!(context.prompt(), "...> ");
    }
    assert!(submit(&mut context, &mut buffers, ":run").is_err());
    let id = buffers.selected().unwrap().id();
    buffers.create("scope-script", "other");
    assert!(submit(&mut context, &mut buffers, ":show").is_err());
    buffers.select(id).unwrap();
    submit(&mut context, &mut buffers, ")").unwrap();
    assert_eq!(buffers.selected().unwrap().text(), "1 # keep\n;\n(\n)");
    assert_eq!(buffers.selected().unwrap().version(), version + 1);
    assert_eq!(
        submit(&mut context, &mut buffers, ":show").unwrap(),
        Some(":scope show".into())
    );
}

#[test]
fn entry_denies_missing_host_and_preserves_other_language() {
    let mut context = ScriptContext::default();
    let mut buffers = ScratchBuffers::default();
    assert!(
        context
            .route(":scope script enter", &mut buffers, None, &mut Vec::new())
            .is_err()
    );
    let id = buffers.create("text", "retain me");
    assert!(submit(&mut context, &mut buffers, ":scope script enter").is_err());
    assert_eq!(buffers.selected().unwrap().id(), id);
    assert_eq!(buffers.selected().unwrap().text(), "retain me");
}
