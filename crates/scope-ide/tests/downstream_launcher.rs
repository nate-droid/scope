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
