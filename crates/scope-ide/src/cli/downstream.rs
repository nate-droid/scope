use super::ReadOnlyCommand;
use crate::scratch::ScratchBuffers;
use std::io::Write;

/// Whether a registered host recognized a downstream command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownstreamOutcome {
    Handled,
    Unhandled,
}

/// Stable machine-readable code and human-readable detail for downstream failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownstreamError {
    pub code: String,
    pub message: String,
}

impl DownstreamError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for DownstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for DownstreamError {}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

pub(super) fn registrations(
    host: Option<&dyn ReadOnlyCommand>,
) -> Result<Vec<String>, DownstreamError> {
    let names = host.map_or(&[][..], ReadOnlyCommand::downstream_namespaces);
    if names.len() > 64 {
        return Err(DownstreamError::new(
            "registration_limit",
            "at most 64 downstream namespaces are allowed",
        ));
    }
    let mut result = Vec::new();
    for name in names {
        if !valid_name(name) {
            return Err(DownstreamError::new(
                "invalid_namespace",
                format!(
                    "invalid downstream namespace: {name:?}; expected [a-z][a-z0-9_-]*, at most 64 bytes"
                ),
            ));
        }
        if result.iter().any(|existing| existing == name) {
            return Err(DownstreamError::new(
                "duplicate_namespace",
                format!("duplicate downstream namespace: {name}"),
            ));
        }
        result.push((*name).to_owned());
    }
    Ok(result)
}

pub(super) fn dispatch(
    input: &str,
    namespaces: &[String],
    host: &mut Option<&mut dyn ReadOnlyCommand>,
    scratches: &ScratchBuffers,
    output: &mut dyn Write,
) -> Result<(), DownstreamError> {
    if input.contains(['\'', '"', '\\']) {
        return Err(DownstreamError::new(
            "invalid_arguments",
            "downstream arguments are whitespace-delimited only; quotes and backslashes are not supported",
        ));
    }
    let mut parts = input.split_whitespace();
    let Some(namespace) = parts.next() else {
        writeln!(
            output,
            "Downstream namespaces: {}",
            if namespaces.is_empty() {
                "(none)".to_owned()
            } else {
                namespaces.join(", ")
            }
        )
        .map_err(io_error)?;
        return Ok(());
    };
    if !valid_name(namespace) {
        return Err(DownstreamError::new(
            "invalid_namespace",
            "expected namespace matching [a-z][a-z0-9_-]*, at most 64 bytes",
        ));
    }
    if !namespaces.iter().any(|name| name == namespace) {
        return Err(DownstreamError::new(
            "unknown_namespace",
            format!("unregistered downstream namespace: {namespace}"),
        ));
    }
    let command = parts.next().ok_or_else(|| {
        DownstreamError::new(
            "missing_command",
            "usage: :scope downstream NAME COMMAND ARGS",
        )
    })?;
    let args: Vec<_> = parts.collect();
    let target = scratches
        .semantic_target()
        .map_err(|error| DownstreamError::new("target_unavailable", error.to_string()))?;
    let Some(host) = host.as_deref_mut() else {
        return Err(DownstreamError::new(
            "unknown_namespace",
            "no downstream host installed",
        ));
    };
    match host.invoke_downstream(namespace, command, &args, target, output)? {
        DownstreamOutcome::Handled => Ok(()),
        DownstreamOutcome::Unhandled => Err(DownstreamError::new(
            "unknown_command",
            format!("unhandled downstream command: {namespace} {command}"),
        )),
    }
}

fn io_error(error: std::io::Error) -> DownstreamError {
    DownstreamError::new("output_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::App, cli::run_with_session_and_command, document::Document,
        language::InteractiveSession, scratch::ScratchBuffer, test_support::TempWorkspace,
        text::TextRange,
    };

    #[derive(Default)]
    struct Host {
        names: Vec<&'static str>,
        calls: Vec<Option<(u64, String, u64)>>,
    }
    impl ReadOnlyCommand for Host {
        fn downstream_namespaces(&self) -> &[&str] {
            &self.names
        }
        fn invoke(
            &mut self,
            _: &Document,
            _: TextRange,
            _: &str,
            _: &mut dyn Write,
        ) -> Result<(), Box<dyn std::error::Error>> {
            panic!("legacy callback")
        }
        fn invoke_downstream(
            &mut self,
            namespace: &str,
            command: &str,
            args: &[&str],
            target: Option<&ScratchBuffer>,
            output: &mut dyn Write,
        ) -> Result<DownstreamOutcome, DownstreamError> {
            assert!(self.names.contains(&namespace));
            let snapshot = |s: &ScratchBuffer| (s.id(), s.text().to_owned(), s.version());
            self.calls.push(target.map(snapshot));
            match command {
                "read" => {
                    assert_eq!(args, ["α", "$literal"]);
                    writeln!(output, "host call {}", self.calls.len()).map_err(io_error)?;
                    Ok(DownstreamOutcome::Handled)
                }
                "fail" => Err(DownstreamError::new("host_denied", "explicit failure")),
                _ => Ok(DownstreamOutcome::Unhandled),
            }
        }
    }
    fn run(host: &mut Host, input: &str) -> Result<String, Box<dyn std::error::Error>> {
        let root = TempWorkspace::new("downstream");
        let mut output = Vec::new();
        run_with_session_and_command(
            App::new(root.workspace())?,
            InteractiveSession::new(),
            host,
            std::io::Cursor::new(input),
            &mut output,
        )?;
        Ok(String::from_utf8(output)?)
    }
    #[test]
    fn lifecycle_live_revisions_independent_binding_closed_target_and_recovery() {
        let mut host = Host {
            names: vec!["demo"],
            ..Host::default()
        };
        let output = run(&mut host, concat!(
            ":scope downstream\n:scope downstream demo read α $literal\n",
            ":scope scratch new text\n:scope line 1 \"target\"\n:scope semantic-target 0\n",
            ":scope scratch new text\n:scope line 1 \"source\"\n:scope downstream demo read α $literal\n",
            ":scope line 1 \"edited\"\n:scope downstream demo read α $literal\n",
            ":scope scratch select 0\n:scope line 1 \"new target\"\n:scope scratch select 1\n:scope downstream demo read α $literal\n",
            ":scope downstream demo fail\n:scope downstream demo unknown\n",
            ":scope scratch select 0\n:scope discard\n:scope downstream demo read α $literal\n",
            ":scope semantic-target none\n:scope downstream demo read α $literal\n:scope session\n:scope quit\n"
        )).unwrap();
        assert_eq!(host.calls.len(), 7, "{output}");
        assert_eq!(host.calls[0], None);
        let initial_target = host.calls[1].as_ref().unwrap();
        assert_eq!(initial_target.0, 0);
        assert_eq!(initial_target.1, "target");
        // Editing the independently selected scratch leaves the target unchanged.
        assert_eq!(host.calls[2], host.calls[1]);
        let edited_target = host.calls[3].as_ref().unwrap();
        assert_eq!(edited_target.0, initial_target.0);
        assert_eq!(edited_target.1, "new target");
        assert!(edited_target.2 > initial_target.2);
        assert_eq!(host.calls[4], host.calls[3]);
        assert_eq!(host.calls[5], host.calls[3]);
        assert!(host.calls[6].is_none());
        for text in [
            "Downstream namespaces: demo",
            "host_denied",
            "unknown_command",
            "target_unavailable",
            "Active language: none",
        ] {
            assert!(output.contains(text), "{output}");
        }
    }
    #[test]
    fn explicit_rebind_changes_target_independently_of_selection() {
        let mut host = Host {
            names: vec!["demo"],
            ..Host::default()
        };
        let output = run(
            &mut host,
            concat!(
                ":scope scratch new text\n:scope line 1 \"target0\"\n",
                ":scope semantic-target 0\n:scope scratch new text\n:scope line 1 \"target1\"\n",
                ":scope downstream demo read α $literal\n",
                ":scope semantic-target 1\n:scope scratch select 0\n",
                ":scope downstream demo read α $literal\n",
                ":scope line 1 \"edited target0\"\n:scope downstream demo read α $literal\n",
                ":scope scratch select 1\n:scope line 1 \"edited target1\"\n",
                ":scope scratch select 0\n:scope downstream demo read α $literal\n:scope quit\n"
            ),
        )
        .unwrap();
        assert_eq!(host.calls.len(), 4, "{output}");
        let target0 = host.calls[0].as_ref().unwrap();
        assert_eq!((target0.0, target0.1.as_str()), (0, "target0"));
        let target1 = host.calls[1].as_ref().unwrap();
        assert_eq!((target1.0, target1.1.as_str()), (1, "target1"));
        assert_eq!(host.calls[2], host.calls[1]);
        let edited_target1 = host.calls[3].as_ref().unwrap();
        assert_eq!(
            (edited_target1.0, edited_target1.1.as_str()),
            (1, "edited target1")
        );
        assert!(edited_target1.2 > target1.2);
        assert!(!output.contains("error:"), "{output}");
    }

    #[test]
    fn builtin_named_namespace_does_not_shadow_session() {
        let mut host = Host {
            names: vec!["session"],
            ..Host::default()
        };
        let output = run(
            &mut host,
            concat!(
                ":scope session\n:scope downstream\n",
                ":scope downstream session read α $literal\n",
                ":scope session none\n:scope session\n:scope quit\n"
            ),
        )
        .unwrap();
        assert_eq!(host.calls, [None]);
        assert_eq!(
            output.matches("Active language: none").count(),
            3,
            "{output}"
        );
        assert!(
            output.contains("Downstream namespaces: session"),
            "{output}"
        );
        assert!(output.contains("host call 1"), "{output}");
        assert!(!output.contains("error:"), "{output}");
    }

    #[test]
    fn shell_without_host_lists_none_and_preserves_builtins() {
        let root = TempWorkspace::new("downstream-no-host");
        let mut output = Vec::new();
        crate::cli::run_with_session(
            App::new(root.workspace()).unwrap(),
            InteractiveSession::new(),
            std::io::Cursor::new(concat!(
                ":scope downstream\n:scope downstream demo read\n",
                ":scope session\n:scope quit\n"
            )),
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Downstream namespaces: (none)"), "{output}");
        assert!(output.contains("unknown_namespace"), "{output}");
        assert!(output.contains("Active language: none"), "{output}");
    }

    #[test]
    fn malformed_unknown_and_builtin_commands_never_dispatch() {
        let mut host = Host {
            names: vec!["demo"],
            ..Host::default()
        };
        let output = run(&mut host, ":scope downstream other read\n:scope downstream BAD read\n:scope downstream demo\n:scope downstream demo read 'x'\n:scope downstream demo read \"x\"\n:scope downstream demo read x\\y\n:scope session\n:scope help\n:scope quit\n").unwrap();
        assert!(host.calls.is_empty());
        for code in [
            "unknown_namespace",
            "invalid_namespace",
            "missing_command",
            "invalid_arguments",
        ] {
            assert!(output.contains(code), "{output}");
        }
    }
    #[test]
    fn startup_registration_is_bounded_and_deterministic() {
        for (names, code) in [
            (vec!["demo", "demo"], "duplicate_namespace"),
            (vec!["Bad", "Bad"], "invalid_namespace"),
            (vec!["demo"; 65], "registration_limit"),
            (vec![""], "invalid_namespace"),
        ] {
            let mut host = Host {
                names,
                ..Host::default()
            };
            let error = run(&mut host, ":scope downstream demo read α $literal\n").unwrap_err();
            assert_eq!(error.downcast_ref::<DownstreamError>().unwrap().code, code);
            assert!(host.calls.is_empty());
        }
    }
    #[test]
    fn no_extension_and_default_callback_are_safe() {
        let output = run(
            &mut Host::default(),
            ":scope downstream\n:scope downstream demo read\n:scope quit\n",
        )
        .unwrap();
        assert!(output.contains("Downstream namespaces: (none)"));
        assert!(output.contains("unknown_namespace"));
        struct Legacy;
        impl ReadOnlyCommand for Legacy {
            fn invoke(
                &mut self,
                _: &Document,
                _: TextRange,
                _: &str,
                _: &mut dyn Write,
            ) -> Result<(), Box<dyn std::error::Error>> {
                unreachable!()
            }
        }
        let mut legacy = Legacy;
        assert!(registrations(Some(&legacy)).unwrap().is_empty());
        assert_eq!(
            legacy
                .invoke_downstream("demo", "read", &[], None, &mut Vec::new())
                .unwrap(),
            DownstreamOutcome::Unhandled
        );
    }
}
