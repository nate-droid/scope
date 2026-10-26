//! Public-consumer coverage of retained source proposals through the real launcher.
#![cfg(all(feature = "tui", unix))]

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
        let expected = match self.calls {
            0 => ("original 猫\nsecond\n", 0, "pending"),
            1 => ("committed é\nsecond\n", 1, "accepted"),
            2 => ("committed é\nsecond\n", 1, "duplicate"),
            3 => ("committed é\nsecond\n", 1, "rejected"),
            _ => panic!("unexpected callback"),
        };
        assert_eq!(args, &[expected.2]);
        let target = target.expect("downstream inspection must bind the selected source");
        assert_eq!(target.text(), expected.0);
        assert_eq!(target.version(), expected.1);
        self.calls += 1;
        writeln!(output, "SOURCE_PROPOSAL_CALLBACK_OK_{}", self.calls).unwrap();
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
#[ignore = "PTY child; invoked by source_proposals_through_real_launcher"]
fn launcher_child() {
    assert_eq!(
        std::env::var("SCOPE_SOURCE_PROPOSAL_PTY_CHILD").as_deref(),
        Ok("1")
    );
    let mut consumer = Consumer::default();
    cli::run_with_registry_and_command(
        ["workspace", "."].map(str::to_owned),
        LanguageRegistry::builtins(),
        &mut consumer,
    )
    .unwrap();
    assert_eq!(consumer.calls, 4, "all committed-source checks must run");
}

#[test]
fn source_proposals_through_real_launcher() {
    let result = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/pty_source_proposal_launcher.py"
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
