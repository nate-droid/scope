//! External-consumer verification of the ordinary workspace launcher.
#![cfg(all(feature = "tui", unix))]

use scope_ide::{
    cli::{self, DownstreamError, DownstreamOutcome, ReadOnlyCommand},
    document::Document,
    language::{
        LanguageAnalysis, LanguageDocument, LanguagePath, LanguageRegistry, LanguageService,
    },
    scratch::ScratchBuffer,
    text::TextRange,
    tui::DownstreamAction,
};
use std::{io::Write, process::Command};

struct ConsumerLanguage;

impl LanguageService for ConsumerLanguage {
    fn language_id(&self) -> &'static str {
        "consumer-language"
    }

    fn language_name(&self) -> &'static str {
        "Consumer language"
    }

    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("consumer")
    }

    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
}

static CONSUMER_LANGUAGE: ConsumerLanguage = ConsumerLanguage;

#[derive(Default)]
struct Consumer {
    calls: usize,
}

impl ReadOnlyCommand for Consumer {
    fn downstream_namespaces(&self) -> &[&str] {
        &["consumer"]
    }

    fn downstream_actions(&self) -> Vec<DownstreamAction> {
        vec![DownstreamAction {
            id: "inspect".into(),
            label: "Consumer inspect committed text".into(),
            language: "consumer-language".into(),
            namespace: "consumer".into(),
            command: "inspect".into(),
            args: vec!["literal-argument".into()],
        }]
    }

    fn invoke_downstream(
        &mut self,
        namespace: &str,
        command: &str,
        args: &[&str],
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<DownstreamOutcome, DownstreamError> {
        assert_eq!(
            (namespace, command, args),
            ("consumer", "inspect", &["literal-argument"][..])
        );
        let target = target.expect("action must bind the selected source");
        assert_eq!(target.language_id(), "consumer-language");
        assert_eq!(target.text(), "selected-live-text");
        self.calls += 1;
        writeln!(output, "CONSUMER_DISPATCH_OK_{}", self.calls).unwrap();
        Ok(DownstreamOutcome::Handled)
    }

    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("action must use downstream dispatch, not the document callback")
    }
}

// Spawned only by the PTY driver. The test crate is a separate consumer of the
// library: no private imports or direct WorkspaceTui registration are used.
#[test]
#[ignore = "PTY child; invoked by downstream_actions_through_real_launcher"]
fn launcher_child() {
    assert_eq!(
        std::env::var("SCOPE_CONSUMER_PTY_CHILD").as_deref(),
        Ok("1")
    );
    let mut consumer = Consumer::default();
    cli::run_with_registry_and_command(
        ["workspace", "."].map(str::to_owned),
        LanguageRegistry::builtins().with_language(&CONSUMER_LANGUAGE),
        &mut consumer,
    )
    .unwrap();
    assert_eq!(consumer.calls, 2, "only matching F1 actions may dispatch");
}

struct H4Language(std::sync::atomic::AtomicUsize);

impl LanguageService for H4Language {
    fn language_id(&self) -> &'static str {
        "h4-fixture"
    }
    fn language_name(&self) -> &'static str {
        "H4 fixture"
    }
    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("h4")
    }
    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
    fn semantic_operations(&self) -> Vec<scope_language::SemanticOperationDescription> {
        use scope_language::*;
        vec![
            SemanticOperationDescription {
                id: "h4.evaluate".into(),
                description: "Evaluate captured H4 source".into(),
                arguments: vec![],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            }
            .with_purpose(SemanticOperationPurpose::CandidateEvaluation)
            .unwrap(),
        ]
    }
    fn perform_semantic_operation(
        &self,
        request: scope_language::SemanticOperationRequest<'_>,
    ) -> Result<scope_language::SemanticOperationResult, scope_language::SemanticOperationError>
    {
        assert_eq!(request.operation_id, "h4.evaluate");
        assert_eq!(
            request.document.text(),
            "published-draft-run-draft-original"
        );
        assert_eq!(request.document.version(), 2);
        assert!(request.selection.is_none());
        assert_eq!(request.arguments, serde_json::json!({}));
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(scope_language::SemanticOperationResult::Evaluated {
            value: serde_json::json!({"value": 16, "evidence": "H4_EXACT_SOURCE_OK"}),
            rationale: "Captured source only; no disk write".into(),
            evaluation: scope_language::SemanticEvaluation {
                outcome: scope_language::SemanticOutcome::Satisfied,
                summary: "16".into(),
                diagnostics: None,
                failed_criteria: vec![],
                contract_id: None,
            },
        })
    }
}

#[test]
#[ignore = "PTY child; invoked by h4_shortcuts_through_real_launcher"]
fn h4_launcher_child() {
    assert_eq!(std::env::var("SCOPE_H4_PTY_CHILD").as_deref(), Ok("1"));
    let service = Box::leak(Box::new(H4Language(std::sync::atomic::AtomicUsize::new(0))));
    cli::run_with_registry(
        ["workspace", "."].map(str::to_owned),
        LanguageRegistry::builtins().with_language(service),
    )
    .unwrap();
    assert_eq!(service.0.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn h4_shortcuts_through_real_launcher() {
    let result = Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/pty_h4_launcher.py"
        ))
        .arg(std::env::current_exe().unwrap())
        .output()
        .expect("Python 3 is required for the H4 PTY test");
    assert!(
        result.status.success(),
        "H4 PTY failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn pty_command_synchronization() {
    let result = Command::new("python3")
        .args([
            "-B",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/test_pty_sync.py"),
        ])
        .output()
        .expect("Python 3 is required for PTY synchronization tests");
    assert!(
        result.status.success(),
        "PTY synchronization failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn downstream_actions_through_real_launcher() {
    let result = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/pty_downstream_launcher.py"
        ))
        .arg(std::env::current_exe().unwrap())
        .output()
        .expect("Python 3 is required for the Unix PTY launcher integration test");
    assert!(
        result.status.success(),
        "PTY integration failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
