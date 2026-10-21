#![cfg(feature = "tools")]

//! End-to-end inspection through the shipped shell parser with default features.
use scope_ide::{
    app::App,
    cli,
    document::Document,
    language::InteractiveSession,
    semantic_inspection::{self, InspectionBinding, InspectionError},
    text::{Span, TextEdit, TextRange},
    workspace::Workspace,
};
use scope_language::{
    LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, SemanticArgument,
    SemanticArgumentKind, SemanticOperationDescription, SemanticOperationEffect,
    SemanticOperationError, SemanticOperationRequest, SemanticOperationResult,
    SemanticTargetRequirement,
};
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug)]
struct Invocation {
    text: String,
    version: u64,
    path: String,
}

struct Inspector {
    operations: Vec<SemanticOperationDescription>,
    calls: Mutex<Vec<Invocation>>,
    value: Value,
    outcome: Mutex<Option<Result<SemanticOperationResult, SemanticOperationError>>>,
    catalog_revision: AtomicUsize,
}

fn operation(id: &str) -> SemanticOperationDescription {
    SemanticOperationDescription {
        id: id.into(),
        description: format!("Read {id} from the App snapshot"),
        arguments: vec![],
        effect: SemanticOperationEffect::Read,
        target: SemanticTargetRequirement::Document,
        constraints: None,
    }
}

impl Inspector {
    fn new(operations: Vec<SemanticOperationDescription>) -> &'static Self {
        // The public registry requires static services; each test has its own state.
        Box::leak(Box::new(Self {
            operations,
            outcome: Mutex::new(None),
            catalog_revision: AtomicUsize::new(0),
            calls: Mutex::new(vec![]),
            value: json!({
                "float": 1.25,
                "unsigned": u64::MAX,
                "nested": {"items": [true, 0.125, {"max": u64::MAX}]},
                "control": "line\nnext\t\u{1b}[31m\r\u{009b}\u{007f}\u{202e}",
                "long": "native-payload-marker".repeat(40),
            }),
        }))
    }

    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl LanguageService for Inspector {
    fn language_id(&self) -> &'static str {
        "shell-inspection-fixture"
    }
    fn language_name(&self) -> &'static str {
        "Shell inspection fixture"
    }
    fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
        true
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        let mut operations = self.operations.clone();
        if self.catalog_revision.load(Ordering::SeqCst) != 0 {
            for operation in &mut operations {
                operation.description.push_str(" (revised contract)");
            }
        }
        operations
    }
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        assert_eq!(request.operation_id, "fixture.read");
        assert_eq!(request.arguments, json!({}));
        assert!(request.selection.is_none());
        self.calls.lock().unwrap().push(Invocation {
            text: request.document.text().into(),
            version: request.document.version(),
            path: request.document.path().as_str().into(),
        });
        if let Some(outcome) = self.outcome.lock().unwrap().as_ref() {
            return outcome.clone();
        }
        Ok(SemanticOperationResult::ReadOnly {
            value: self.value.clone(),
            rationale: "Read only; never evaluate or save".into(),
        })
    }
}

fn app(service: &'static Inspector, source: Option<&str>) -> App {
    let mut app = App::builder(Workspace::read_only(env!("CARGO_MANIFEST_DIR")).unwrap())
        .languages([service as &'static dyn LanguageService])
        .build()
        .unwrap();
    if let Some(source) = source {
        app.open_path("Cargo.toml").unwrap();
        let document = app.open_document_mut().unwrap();
        let range = document
            .buffer()
            .range_for_span(Span::new(0, document.text().len()))
            .unwrap();
        document
            .apply_edit(TextEdit::replace(range, source))
            .unwrap();
    }
    app
}

fn shell(app: App, input: &str) -> String {
    let mut output = vec![];
    cli::run_with_session(
        app,
        InteractiveSession::new(),
        input.as_bytes(),
        &mut output,
    )
    .unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn direct_binding_denial_checks_exact_source_without_dispatch_or_mutation() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let mut app = app(service, Some("unsaved denied λ"));
    let before = app.open_document().unwrap().clone();
    let binding = InspectionBinding::capture_app(&app, "fixture.read").unwrap();
    let mut authorizations = 0;
    let result = binding
        .invoke_app(&mut app, |request| {
            authorizations += 1;
            assert_eq!(request.operation.id, "fixture.read");
            assert_eq!(request.source.text, before.text());
            assert_eq!(request.source.version, before.version());
            assert_eq!(request.source.id, binding.source_id());
            assert_eq!(request.arguments, &json!({}));
            false
        })
        .unwrap();
    assert_eq!(authorizations, 1);
    assert_eq!(service.count(), 0);
    assert_eq!(app.open_document().unwrap(), &before);
    assert!(result.native_result.is_none());
    let scope_ide::tool::ToolValue::Object(fields) = &result.result.value else {
        panic!("Expected structured denial: {:?}", result.result);
    };
    assert_eq!(fields["status"].as_str(), Some("unauthorized"));
}

#[test]
fn duplicate_and_empty_catalog_ids_fail_discovery_and_capture_closed() {
    for invalid_id in ["fixture.read", ""] {
        // Invalid catalog entries must invalidate even otherwise eligible reads.
        let mut invalid = operation(invalid_id);
        invalid.effect = SemanticOperationEffect::ProposeChanges;
        let service = Inspector::new(vec![operation("fixture.read"), invalid]);
        let app = app(service, Some("unchanged"));
        let before = app.open_document().unwrap().clone();
        assert!(matches!(semantic_inspection::discover_app(&app),
            Err(InspectionError::InvalidOperationId(id)) if id == invalid_id));
        assert!(
            matches!(InspectionBinding::capture_app(&app, "fixture.read"),
            Err(InspectionError::InvalidOperationId(id)) if id == invalid_id)
        );
        let output = shell(
            app,
            "inspect\ninspect operation fixture.read\ninspect confirm 1\nquit\n",
        );
        assert!(output.contains("InvalidOperationId"), "{output}");
        assert_eq!(service.count(), 0);
        assert_eq!(before.text(), "unchanged");
    }
}

#[test]
fn selection_target_is_excluded_from_discovery_and_cannot_be_captured() {
    let mut selection = operation("fixture.selection");
    selection.target = SemanticTargetRequirement::Selection;
    let service = Inspector::new(vec![operation("fixture.read"), selection]);
    let app = app(service, Some("source"));
    let discovered = semantic_inspection::discover_app(&app).unwrap();
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].id, "fixture.read");
    assert!(
        matches!(InspectionBinding::capture_app(&app, "fixture.selection"),
        Err(InspectionError::NotInspectable(id)) if id == "fixture.selection")
    );
    let output = shell(
        app,
        "inspect\ninspect operation fixture.selection\ninspect confirm 1\nquit\n",
    );
    assert!(output.contains("NotInspectable"), "{output}");
    assert_eq!(service.count(), 0);
}

#[test]
fn reserved_control_ids_are_discovered_and_captured_only_as_operations() {
    let ids = ["result", "confirm", "cancel", "operation", "control"];
    let service = Inspector::new(ids.iter().map(|id| operation(id)).collect());
    let app = app(service, Some("source"));
    let discovered = semantic_inspection::discover_app(&app).unwrap();
    assert_eq!(
        discovered
            .iter()
            .map(|op| op.id.as_str())
            .collect::<Vec<_>>(),
        ids
    );
    let mut input = String::from("inspect\n");
    for id in ids {
        input.push_str(&format!("inspect operation {id}\ninspect cancel\n"));
    }
    input.push_str("quit\n");
    let output = shell(app, &input);
    assert!(!output.contains("error:"), "{output}");
    for token in 1..=ids.len() {
        assert!(
            output.contains(&format!("inspect confirm {token}")),
            "{output}"
        );
    }
    assert_eq!(
        output.to_lowercase().matches("cancelled").count(),
        ids.len(),
        "{output}"
    );
    assert_eq!(service.count(), 0);
}

struct NonmatchingService;
impl LanguageService for NonmatchingService {
    fn language_id(&self) -> &'static str {
        "nonmatching-inspector"
    }
    fn language_name(&self) -> &'static str {
        "Nonmatching inspector"
    }
    fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
        false
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        panic!("Nonmatching service must not analyze this document")
    }
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        panic!("Nonmatching service must not supply the inspection catalog")
    }
}

#[test]
fn inspection_resolves_matching_service_not_first_registered_service() {
    static NONMATCHING: NonmatchingService = NonmatchingService;
    let matching = Inspector::new(vec![operation("fixture.read")]);
    let mut app = App::builder(Workspace::read_only(env!("CARGO_MANIFEST_DIR")).unwrap())
        .languages([&NONMATCHING as &'static dyn LanguageService, matching])
        .build()
        .unwrap();
    app.open_path("Cargo.toml").unwrap();
    assert_eq!(
        app.semantic_service_for_document(app.open_document().unwrap())
            .language_id(),
        matching.language_id()
    );
    let output = shell(
        app,
        "inspect\ninspect operation fixture.read\ninspect confirm 1\nquit\n",
    );
    assert!(output.contains("fixture.read"), "{output}");
    assert_eq!(matching.count(), 1, "{output}");
}

fn assert_terminal_safe(output: &str) {
    for character in output.chars() {
        assert!(
            (!character.is_control() || matches!(character, '\n' | '\t'))
                && !matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'),
            "Unsafe raw character {character:?} in output: {output:?}",
        );
    }
}

#[test]
fn shell_inspection_walkthrough() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, None),
        concat!(
            "open Cargo.toml\n",
            "inspect\n",
            "inspect operation fixture.read\n",
            "inspect confirm 1\n",
            "inspect result\n",
            "inspect result --full\n",
            "replace 1 1 1 1 walkthrough-unsaved-\n",
            "inspect result\n",
            "inspect operation fixture.read\n",
            "inspect confirm 2\n",
            "inspect result\n",
            "quit\n",
        ),
    );
    println!("{output}");
    assert!(!output.contains("error:"), "{output}");
    assert!(output.contains("opened Cargo.toml"), "{output}");
    assert!(output.contains("inspect confirm 1"), "{output}");
    assert!(output.contains("inspect confirm 2"), "{output}");
    assert!(
        output.contains(&"native-payload-marker".repeat(40)),
        "{output}"
    );
    let mut states = output
        .lines()
        .filter(|line| line.contains("Last inspection result"))
        .map(|line| {
            if line.contains("STALE") {
                "STALE"
            } else if line.contains("CURRENT") {
                "CURRENT"
            } else {
                panic!("Missing freshness in result header: {line}")
            }
        })
        .collect::<Vec<_>>();
    assert!(
        states.len() >= 5,
        "Expected confirmed, summary, full, stale, and refreshed result headers: {output}"
    );
    states.dedup();
    assert_eq!(states, ["CURRENT", "STALE", "CURRENT"], "{output}");
    let calls = service.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].path, "Cargo.toml");
    assert_eq!(calls[1].path, calls[0].path);
    assert_eq!(
        calls[1].text,
        format!("walkthrough-unsaved-{}", calls[0].text)
    );
    assert!(calls[1].version > calls[0].version);
    assert_terminal_safe(&output);
}

#[test]
fn operation_ids_and_descriptions_escape_terminal_controls() {
    let id = "fixture.unsafe\u{009b}\u{007f}\u{202e}";
    let mut unsafe_operation = operation(id);
    unsafe_operation.description = "description\u{009b}\u{007f}\u{202e}".into();
    let service = Inspector::new(vec![unsafe_operation]);
    let input = format!("inspect\ninspect operation {id}\ninspect cancel\nquit\n");
    let output = shell(app(service, Some("source")), &input);
    assert!(!output.contains("error:"), "{output}");
    assert!(output.contains("fixture.unsafe"), "{output}");
    assert!(output.contains("description"), "{output}");
    assert!(output.contains("inspect confirm 1"), "{output}");
    assert_terminal_safe(&output);
    assert_eq!(service.count(), 0);
}

#[test]
fn discovery_and_capture_never_dispatch() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, Some("unsaved λ")),
        "inspect\ninspect operation fixture.read\nquit\n",
    );
    assert!(output.contains("fixture.read"), "{output}");
    assert!(output.contains("App document"), "{output}");
    assert!(output.contains("confirm 1"), "{output}");
    assert_eq!(service.count(), 0);
}

#[test]
fn confirmation_is_explicit_single_use_and_tokens_increment_per_capture() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, Some("unsaved λ")),
        concat!(
            "inspect operation fixture.read\n",
            "inspect confirm 999\n",
            "inspect confirm 1\n",
            "inspect confirm 1\n",
            "replace 1 1 1 11 second source\n",
            "inspect operation fixture.read\n",
            "inspect confirm 1\n",
            "inspect confirm 2\n",
            "inspect confirm 2\n",
            "quit\n",
        ),
    );
    assert!(output.contains("confirm 1"), "{output}");
    assert!(output.contains("confirm 2"), "{output}");
    assert!(
        output.to_lowercase().contains("no pending inspection"),
        "{output}"
    );
    assert_eq!(service.count(), 2, "{output}");
    let calls = service.calls.lock().unwrap();
    assert_eq!(calls[0].text, "unsaved λ");
    assert_eq!(calls[1].text, "second source");
    for call in calls.iter() {
        assert_eq!(call.path, "Cargo.toml");
        assert!(call.version > 0);
    }
}

#[test]
fn cancel_discards_pending_without_dispatch_and_does_not_reuse_token() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, Some("source")),
        concat!(
            "inspect operation fixture.read\ninspect cancel\ninspect confirm 1\n",
            "inspect operation fixture.read\ninspect confirm 1\ninspect confirm 2\nquit\n",
        ),
    );
    assert!(output.contains("confirm 2"), "{output}");
    assert!(output.to_lowercase().contains("cancelled"), "{output}");
    assert!(
        output.to_lowercase().contains("no pending inspection"),
        "{output}"
    );
    assert_eq!(service.count(), 1, "{output}");
}

#[test]
fn missing_document_and_empty_catalog_fail_without_dispatch() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, None),
        "inspect\ninspect operation fixture.read\ninspect confirm 1\ninspect result\nquit\n",
    );
    assert!(
        output.to_lowercase().contains("no app document"),
        "{output}"
    );
    assert_eq!(service.count(), 0);

    let empty = Inspector::new(vec![]);
    let output = shell(
        app(empty, Some("source")),
        "inspect\ninspect operation fixture.read\ninspect confirm 1\nquit\n",
    );
    assert!(output.to_lowercase().contains("no eligible"), "{output}");
    assert_eq!(empty.count(), 0);
}

#[test]
fn unsupported_effect_arguments_and_invalid_shell_syntax_never_dispatch() {
    let mut write = operation("fixture.write");
    write.effect = SemanticOperationEffect::ProposeChanges;
    let mut arguments = operation("fixture.arguments");
    arguments.arguments.push(SemanticArgument {
        name: "optional".into(),
        kind: SemanticArgumentKind::String,
        required: false,
        description: "Even optional arguments are outside the inspection contract".into(),
    });
    let service = Inspector::new(vec![operation("fixture.read"), write, arguments]);
    let output = shell(
        app(service, Some("source")),
        concat!(
            "inspect\n",
            "inspect operation fixture.write\ninspect confirm 1\n",
            "inspect operation fixture.arguments\ninspect confirm 1\n",
            "inspect operation unknown\ninspect confirm\ninspect confirm nope\n",
            "inspect operation fixture.read extra\ninspect result --unknown\n",
            "inspect cancel extra\nquit\n",
        ),
    );
    assert!(output.contains("fixture.read"), "{output}");
    assert!(output.to_lowercase().contains("usage"), "{output}");
    assert_eq!(service.count(), 0, "{output}");
}

#[test]
fn selected_scratch_is_not_the_app_inspection_target() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, Some("App unsaved source")),
        concat!(
            ":scope scratch new shell-inspection-fixture\n",
            ":scope scratch replace 1 1 1 1 scratch-only-source\n",
            ":scope inspect\n:scope inspect operation fixture.read\n",
            ":scope inspect confirm 1\n:scope quit\n",
        ),
    );
    assert!(output.contains("App document"), "{output}");
    assert!(output.contains("not the inspection target"), "{output}");
    assert_eq!(service.count(), 1, "{output}");
    assert_eq!(service.calls.lock().unwrap()[0].text, "App unsaved source");
}

#[derive(Default)]
struct Observe(Vec<Document>);
impl cli::ReadOnlyCommand for Observe {
    fn invoke(
        &mut self,
        document: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.0.push(document.clone());
        Ok(())
    }
}

#[test]
fn unsaved_source_and_shell_edits_are_seen_by_inspection_and_host_without_mutation() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let mut host = Observe::default();
    let mut output = vec![];
    cli::run_with_session_and_command(
        app(service, Some("before")),
        InteractiveSession::new(),
        &mut host,
        concat!(
            ":scope host observe --selection 0:0..0:0\n",
            "inspect operation fixture.read\ninspect confirm 1\n",
            ":scope host observe --selection 0:0..0:0\n",
            "replace 1 1 1 7 after\n",
            ":scope host observe --selection 0:0..0:0\n",
            "inspect operation fixture.read\ninspect confirm 2\n",
            ":scope host observe --selection 0:0..0:0\nquit\n",
        )
        .as_bytes(),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(!output.contains("error:"), "{output}");
    assert_eq!(host.0.len(), 4);
    assert_eq!(host.0[0], host.0[1]);
    assert_eq!(host.0[2], host.0[3]);
    assert_eq!(host.0[0].text(), "before");
    assert_eq!(host.0[2].text(), "after");
    assert_eq!(host.0[0].id(), host.0[2].id());
    assert!(host.0[2].version() > host.0[0].version());
    assert!(host.0.iter().all(Document::is_dirty));
    let calls = service.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].text, "before");
    assert_eq!(calls[1].text, "after");
    assert_eq!(calls[0].version, host.0[0].version());
    assert_eq!(calls[1].version, host.0[2].version());
}

#[test]
fn edits_invalidate_pending_confirmation_even_if_text_is_restored() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, Some("before")),
        concat!(
            "inspect operation fixture.read\nreplace 1 1 1 7 after\n",
            "replace 1 1 1 6 before\ninspect confirm 1\n",
            "inspect confirm 1\nquit\n",
        ),
    );
    assert!(output.contains("SnapshotMismatch"), "{output}");
    assert_eq!(service.count(), 0, "{output}");
}

#[test]
fn actual_open_replace_and_same_path_reopen_track_result_freshness_and_identity() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, None),
        concat!(
            "open Cargo.toml\ninspect operation fixture.read\ninspect confirm 1\n",
            "inspect result\nreplace 1 1 1 1 unsaved-\ninspect result\n",
            "inspect operation fixture.read\nopen Cargo.toml\ninspect confirm 2\n",
            "inspect result\nquit\n",
        ),
    );
    let current = output.find("CURRENT").unwrap_or_else(|| panic!("{output}"));
    let stale = output.find("STALE").unwrap_or_else(|| panic!("{output}"));
    assert!(current < stale, "{output}");
    assert!(output.contains("SnapshotMismatch"), "{output}");
    assert_eq!(service.count(), 1, "{output}");
}

#[test]
fn context_does_not_renew_stale_inspection_or_change_its_owner() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, None),
        concat!(
            "open Cargo.toml\n:scope open Cargo.toml\n",
            "inspect operation fixture.read\ninspect confirm 1\n",
            "replace 1 1 1 1 unsaved-\ninspect result\n",
            ":scope context\ncontext\ninspect result\nquit\n",
        ),
    );
    let headers: Vec<_> = output
        .lines()
        .filter(|line| line.contains("Last inspection result:"))
        .collect();
    assert_eq!(headers.len(), 3, "{output}");
    assert!(headers[0].contains("CURRENT"), "{output}");
    assert!(headers[1].contains("STALE"), "{output}");
    assert!(headers[2].contains("STALE"), "{output}");
    assert!(
        output.contains("App and buffer targets differ: true"),
        "{output}"
    );
    assert!(
        output.contains("Inspection target (eligible operations/confirmation required): Document("),
        "{output}"
    );
    assert_eq!(service.count(), 1, "{output}");
}

#[test]
fn reopening_same_path_without_edits_invalidates_capture_and_completed_result() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let output = shell(
        app(service, None),
        concat!(
            "open Cargo.toml\ninspect operation fixture.read\ninspect confirm 1\n",
            "inspect result\ninspect operation fixture.read\nopen Cargo.toml\n",
            "inspect result\ninspect confirm 2\nquit\n",
        ),
    );
    assert!(output.contains("CURRENT"), "{output}");
    assert!(output.contains("STALE"), "{output}");
    assert!(output.contains("SnapshotMismatch"), "{output}");
    assert_eq!(service.count(), 1, "{output}");
}

#[test]
fn concise_result_is_bounded_and_full_result_preserves_native_json_and_escapes_controls() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let concise = shell(
        app(service, Some("source")),
        "inspect operation fixture.read\ninspect confirm 1\ninspect result\nquit\n",
    );
    assert!(concise.contains("CURRENT"), "{concise}");
    assert!(
        !concise.contains(&"native-payload-marker".repeat(40)),
        "{concise}"
    );
    for line in concise
        .lines()
        .filter(|line| line.contains("native-payload-marker"))
    {
        assert!(
            line.chars().count() <= 300,
            "Unbounded concise result: {line}"
        );
    }

    let full = shell(
        app(service, Some("source")),
        "inspect operation fixture.read\ninspect confirm 1\ninspect result --full\nquit\n",
    );
    assert!(full.contains("1.25"), "{full}");
    assert!(full.contains("18446744073709551615"), "{full}");
    assert!(full.contains("0.125"), "{full}");
    assert!(full.contains(&"native-payload-marker".repeat(40)), "{full}");
    assert_terminal_safe(&full);
    assert!(full.contains("\\n") && full.contains("\\t"), "{full}");
    // Accept compact or pretty native JSON, and an outer structured result object.
    let mut preserved = false;
    for (offset, _) in full.match_indices('{') {
        if let Some(Ok(value)) = serde_json::Deserializer::from_str(&full[offset..])
            .into_iter::<Value>()
            .next()
            && (value == service.value || value.get("value") == Some(&service.value))
        {
            preserved = true;
            break;
        }
    }
    assert!(
        preserved,
        "Full output did not preserve the native JSON: {full}"
    );
}

#[test]
fn blocked_result_preserves_actionable_feedback_and_confirmation_is_consumed() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    *service.outcome.lock().unwrap() = Some(Ok(SemanticOperationResult::Blocked {
        blocker: scope_language::SemanticBlocker {
            operation_id: "fixture.read".into(),
            category: scope_language::BlockerCategory::MissingDependency,
            explanation: "fixture dependency unavailable".into(),
            recoverable: true,
            missing_dependencies: vec!["fixture-library".into()],
            diagnostics: vec![],
            suggested_next_action: Some("provide fixture-library".into()),
        },
    }));
    let output = shell(
        app(service, Some("unsaved source")),
        concat!(
            "inspect operation fixture.read\ninspect confirm 1\ninspect result --full\n",
            "inspect confirm 1\nquit\n",
        ),
    );
    assert!(output.to_lowercase().contains("blocked"), "{output}");
    assert!(
        output.contains("fixture dependency unavailable"),
        "{output}"
    );
    assert!(output.contains("provide fixture-library"), "{output}");
    assert!(
        output.to_lowercase().contains("no pending inspection"),
        "{output}"
    );
    assert_eq!(service.count(), 1, "{output}");
}

#[test]
fn native_domain_rejection_is_preserved_not_mistaken_for_dispatch_failure() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let value = json!({
        "accepted": false,
        "status": "domain-rejected",
        "reason": "fixture invariant is unsatisfied",
        "evidence": {"ratio": 0.125, "unsigned": u64::MAX},
    });
    *service.outcome.lock().unwrap() = Some(Ok(SemanticOperationResult::ReadOnly {
        value: value.clone(),
        rationale: "Native domain rejection is still a successful read".into(),
    }));
    let output = shell(
        app(service, Some("source")),
        "inspect operation fixture.read\ninspect confirm 1\ninspect result --full\nquit\n",
    );
    assert!(output.contains("CURRENT"), "{output}");
    assert!(output.contains("domain-rejected"), "{output}");
    let preserved = output.match_indices('{').any(|(offset, _)| {
        serde_json::Deserializer::from_str(&output[offset..])
            .into_iter::<Value>()
            .next()
            .is_some_and(|parsed| parsed.is_ok_and(|parsed| parsed == value))
    });
    assert!(preserved, "Native rejection JSON was lost: {output}");
    // ToolValue cannot represent all native numbers; its projection failure must
    // not erase the provider's domain observation or become a transport error.
    assert!(output.contains("Transport: ok"), "{output}");
    assert_eq!(service.count(), 1);
}

#[test]
fn operation_errors_are_reported_and_do_not_allow_confirmation_replay() {
    for (error, status, message) in [
        (
            SemanticOperationError::InvalidArguments("fixture invalid arguments".into()),
            "invalid_arguments",
            "fixture invalid arguments",
        ),
        (
            SemanticOperationError::InternalFailure("fixture operation failed".into()),
            "internal_failure",
            "fixture operation failed",
        ),
    ] {
        let service = Inspector::new(vec![operation("fixture.read")]);
        *service.outcome.lock().unwrap() = Some(Err(error));
        let output = shell(
            app(service, Some("source")),
            concat!(
                "inspect operation fixture.read\ninspect confirm 1\ninspect result --full\n",
                "inspect confirm 1\nquit\n",
            ),
        );
        assert!(output.contains(status), "{output}");
        assert!(output.contains(message), "{output}");
        assert!(
            output.to_lowercase().contains("no pending inspection"),
            "{output}"
        );
        assert_eq!(service.count(), 1, "{output}");
    }
}

struct RecordingSession(std::sync::Arc<Mutex<Vec<String>>>);
impl scope_language::InteractiveLanguageSession for RecordingSession {
    fn language_id(&self) -> &str {
        "shell-inspection-fixture"
    }
    fn evaluate(
        &mut self,
        source: &str,
    ) -> Result<scope_language::EvaluationResult, scope_language::SessionError> {
        self.0.lock().unwrap().push(source.into());
        Ok(scope_language::EvaluationResult {
            output: vec![],
            diagnostics: vec![],
            status: scope_language::EvaluationStatus::Success,
        })
    }
    fn reset(&mut self) -> Result<(), scope_language::SessionError> {
        Ok(())
    }
    fn close(&mut self) -> Result<(), scope_language::SessionError> {
        Ok(())
    }
}

#[test]
fn active_language_session_requires_namespaced_inspection_and_confirmation() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let evaluated = std::sync::Arc::new(Mutex::new(Vec::new()));
    let mut session = InteractiveSession::new();
    session
        .install(
            "shell-inspection-fixture",
            Box::new(RecordingSession(evaluated.clone())),
        )
        .unwrap();
    let mut output = vec![];
    cli::run_with_session(
        app(service, Some("App snapshot, not REPL source")),
        session,
        concat!(
            ":scope inspect\n:scope inspect operation fixture.read\n",
            "inspect confirm 1\n",
            ":scope inspect confirm 1\n:scope inspect confirm 1\n",
            ":scope inspect result --full\n:scope quit\n",
        )
        .as_bytes(),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert_eq!(*evaluated.lock().unwrap(), ["inspect confirm 1"]);
    assert_eq!(service.count(), 1, "{output}");
    assert_eq!(
        service.calls.lock().unwrap()[0].text,
        "App snapshot, not REPL source"
    );
    assert!(
        output.to_lowercase().contains("no pending inspection"),
        "{output}"
    );
    assert!(output.contains("Last inspection result"), "{output}");
    assert!(output.contains("CURRENT"), "{output}");
}

#[test]
fn json_quoted_operation_ids_are_discovered_losslessly_and_captured_without_dispatch() {
    let ids = vec![
        "fixture.with whitespace".to_owned(),
        "fixture.\"quoted\"\\backslash".to_owned(),
        "fixture.line\nnext\tcolumn".to_owned(),
        format!("fixture.{}", "very-long-id-".repeat(100)),
    ];
    let service = Inspector::new(ids.iter().map(|id| operation(id)).collect());
    let discovery = shell(app(service, Some("source")), "inspect\nquit\n");
    assert_terminal_safe(&discovery);
    let mut input = String::new();
    for id in &ids {
        let encoded = serde_json::to_string(id).unwrap();
        // Discovery must expose the complete copyable JSON string, not a clipped ID.
        assert!(
            discovery.contains(&encoded),
            "Missing {encoded} in {discovery}"
        );
        input.push_str(&format!("inspect operation {encoded}\ninspect cancel\n"));
    }
    input.push_str("quit\n");
    let output = shell(app(service, Some("source")), &input);
    assert_terminal_safe(&output);
    assert!(!output.contains("error:"), "{output}");
    assert_eq!(
        output.to_lowercase().matches("cancelled").count(),
        ids.len(),
        "{output}"
    );
    for token in 1..=ids.len() {
        assert!(
            output.contains(&format!("inspect confirm {token}")),
            "{output}"
        );
    }
    assert_eq!(service.count(), 0);
}

#[test]
fn completed_result_becomes_stale_when_catalog_contract_changes() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let mut callback = ReviseCatalog(service);
    let mut output = vec![];
    cli::run_with_session_and_command(
        app(service, Some("unchanged source")),
        InteractiveSession::new(),
        &mut callback,
        concat!(
            "inspect operation fixture.read\ninspect confirm 1\ninspect result\n",
            ":scope host revise --selection 0:0..0:0\n",
            "inspect result\ninspect result --full\nquit\n",
        )
        .as_bytes(),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    let headers = output
        .lines()
        .filter(|line| line.contains("Last inspection result"))
        .collect::<Vec<_>>();
    assert_eq!(headers.len(), 4, "{output}");
    assert!(headers[0].contains("CURRENT"), "{output}");
    assert!(headers[1].contains("CURRENT"), "{output}");
    assert!(headers[2].contains("STALE"), "{output}");
    assert!(headers[3].contains("STALE"), "{output}");
    assert_eq!(service.catalog_revision.load(Ordering::SeqCst), 1);
    assert_eq!(service.count(), 1);
    assert_eq!(service.calls.lock().unwrap()[0].text, "unchanged source");
}

struct ReviseCatalog(&'static Inspector);
impl cli::ReadOnlyCommand for ReviseCatalog {
    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.0.catalog_revision.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn changed_catalog_contract_invalidates_pending_confirmation_without_dispatch() {
    let service = Inspector::new(vec![operation("fixture.read")]);
    let mut callback = ReviseCatalog(service);
    let mut output = vec![];
    cli::run_with_session_and_command(
        app(service, Some("unchanged source")),
        InteractiveSession::new(),
        &mut callback,
        concat!(
            "inspect operation fixture.read\n",
            ":scope host revise --selection 0:0..0:0\n",
            "inspect confirm 1\ninspect confirm 1\nquit\n",
        )
        .as_bytes(),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert_eq!(service.catalog_revision.load(Ordering::SeqCst), 1);
    assert!(output.contains("SnapshotMismatch"), "{output}");
    assert!(output.contains("STALE"), "{output}");
    assert!(
        output.to_lowercase().contains("no pending inspection"),
        "{output}"
    );
    assert_eq!(service.count(), 0, "{output}");
}
