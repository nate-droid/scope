//! Contracted agent editing through the public launcher, real PTY and local model HTTP fixture.
#![cfg(all(feature = "tui", feature = "agent", unix))]

#[path = "support/configured_editing.rs"]
mod configured_editing;

use scope_ide::{
    cli::{self, DownstreamError, DownstreamOutcome, ReadOnlyCommand},
    document::Document,
    language::LanguageRegistry,
    scratch::ScratchBuffer,
    text::TextRange,
};
use std::{io::Write, process::Command};

#[derive(Default)]
struct Consumer {
    calls: usize,
    denied: bool,
    rejected: bool,
    unavailable: bool,
}

impl ReadOnlyCommand for Consumer {
    fn downstream_namespaces(&self) -> &[&str] {
        &["consumer"]
    }

    fn invoke_downstream(
        &mut self,
        namespace: &str,
        command: &str,
        args: &[&str],
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<DownstreamOutcome, DownstreamError> {
        assert_eq!((namespace, command), ("consumer", "inspect"));
        let expected = match (self.denied, self.rejected, self.unavailable, self.calls) {
            (_, _, true, 0) => ("old", 0, "unavailable"),
            (true, _, false, 0) => ("old", 0, "denied"),
            (false, true, false, 0) => ("old", 0, "pending"),
            (false, true, false, 1) => ("old", 0, "rejected"),
            (false, false, false, 0) => ("old", 0, "draft-dispatch-cancelled"),
            (false, false, false, 1) => ("old", 0, "pending"),
            (false, false, false, 2) => ("old", 0, "draft-accept-cancelled"),
            (false, false, false, 3) => ("new", 1, "accepted"),
            (false, false, false, 4) => ("new", 1, "duplicate"),
            _ => panic!("unexpected callback"),
        };
        assert_eq!(args, &[expected.2]);
        let target = target.expect("downstream inspection must bind the selected source");
        assert_eq!(target.text(), expected.0);
        assert_eq!(target.version(), expected.1);
        self.calls += 1;
        writeln!(output, "CONFIGURED_AGENT_CALLBACK_OK_{}", self.calls).unwrap();
        Ok(DownstreamOutcome::Handled)
    }

    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("must use public downstream dispatch")
    }
}

#[test]
#[ignore = "PTY child; invoked by contracted_agent_edit_through_real_launcher"]
fn launcher_child() {
    assert_eq!(
        std::env::var("SCOPE_CONFIGURED_AGENT_PTY_CHILD").as_deref(),
        Ok("1")
    );
    let denied = std::env::var("SCOPE_CONFIGURED_AGENT_DENY").as_deref() == Ok("1");
    let rejected = std::env::var("SCOPE_CONFIGURED_AGENT_REJECT").as_deref() == Ok("1");
    let unavailable = std::env::var("SCOPE_CONFIGURED_AGENT_UNAVAILABLE").as_deref() == Ok("1");
    let mut consumer = Consumer {
        calls: 0,
        denied,
        rejected,
        unavailable,
    };
    let mut editing = configured_editing::config();
    let authorizations = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = authorizations.clone();
    editing.authorize = Box::new(move |_| {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        !denied
    });
    let registry = LanguageRegistry::default().with_language(editing.service);
    let base_url = std::env::var("SCOPE_CONFIGURED_AGENT_BASE_URL").unwrap();
    cli::run_with_registry_and_command_and_editing(
        ["--agent-base-url", &base_url, "workspace", "."].map(str::to_owned),
        registry,
        &mut consumer,
        editing,
    )
    .unwrap();
    assert_eq!(
        consumer.calls,
        if denied || unavailable {
            1
        } else if rejected {
            2
        } else {
            5
        }
    );
    let authorized_calls = authorizations.load(std::sync::atomic::Ordering::SeqCst);
    if unavailable {
        assert_eq!(
            authorized_calls, 0,
            "incompatible language fails before authorization"
        );
    } else {
        assert!(
            authorized_calls > 0,
            "the configured authorization policy must be consulted"
        );
    }
}

#[test]
fn contracted_agent_edit_through_real_launcher() {
    let result = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/pty_configured_agent_launcher.py"
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
