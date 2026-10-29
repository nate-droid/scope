//! Pure, invocation-local scratch execution without a document or native authority.
use crate::{Error, Interpreter, Value, host::NoHost, runner::Snapshot};
use scope_ide::{
    cli::{ReadOnlyCommand, ScriptPresentation, ScriptPresentationError},
    document::Document,
    scratch::ScratchBuffer,
    text::TextRange,
};
use scope_language::RevisionId;
use std::io::Write;

/// Owned source provenance and interpreter results, independent of live buffers.
#[derive(Debug)]
pub struct PureRunRecord {
    pub source: Snapshot,
    pub printed: Vec<Value>,
    /// Evaluation failures retain their original byte offset and error chain.
    pub outcome: Result<Value, Error>,
}

/// Pure Script execution with no native authority. Agent inspection/proposals
/// are separately opt-in and never install interpreter host capabilities.
#[derive(Default)]
pub struct WorkbenchRunner {
    pub records: Vec<PureRunRecord>,
    #[cfg(feature = "agent")]
    agent: Option<crate::workbench_agent::AgentBinding>,
}

impl WorkbenchRunner {
    /// Install the Script read binding only with explicit host authorization.
    /// Endpoint opt-in and independent edit authorization remain launcher-owned.
    #[cfg(feature = "agent")]
    pub fn with_agent_reads(mut self) -> Self {
        self.agent = Some(crate::workbench_agent::AgentBinding::new());
        self
    }

    pub fn run(&mut self, source: &ScratchBuffer) -> &PureRunRecord {
        let source = Snapshot {
            identity: source.evaluation_source_id().clone(),
            revision: RevisionId::from_version(source.version()),
            text: source.text().to_owned(),
        };
        let mut interpreter = Interpreter::new();
        let outcome = interpreter.eval(&source.text, &mut NoHost);
        self.records.push(PureRunRecord {
            source,
            printed: interpreter.take_output(),
            outcome,
        });
        self.records.last().unwrap()
    }
}

impl ReadOnlyCommand for WorkbenchRunner {
    #[cfg(feature = "agent")]
    fn allow_app_agent_fallback(&self) -> bool {
        false
    }

    #[cfg(feature = "agent")]
    fn semantic_agent(&mut self) -> Option<scope_ide::cli::SemanticAgentBinding<'_>> {
        self.agent.as_mut().map(|agent| agent.binding())
    }

    fn reduce_script_snapshot(
        &self,
        snapshot: scope_language::SourceSnapshot,
    ) -> Result<scope_ide::cli::ScriptReduction, String> {
        crate::runner::reduce_snapshot(snapshot)
    }

    fn admit_script_snapshot(&self, source: &str) -> scope_ide::cli::ScriptSnapshotAdmission {
        use scope_ide::cli::ScriptSnapshotAdmission as Admission;
        match crate::admit_snapshot(source) {
            crate::SnapshotAdmission::CompletePure => Admission::CompletePure,
            crate::SnapshotAdmission::CompleteExplicitRunRequired => Admission::ExplicitRunRequired,
            crate::SnapshotAdmission::Incomplete(_) => Admission::Incomplete,
            crate::SnapshotAdmission::Invalid(error) => {
                let (line, column) = crate::presentation::source_location(source, error.offset);
                Admission::Invalid(format!("{line}:{column}: {error}"))
            }
        }
    }

    fn supports_direct_script(&self) -> bool {
        true
    }

    fn invoke_script_direct(
        &mut self,
        source: &ScratchBuffer,
    ) -> Result<ScriptPresentation, Box<dyn std::error::Error>> {
        if source.language_id() != "scope-script" {
            return Err("direct script execution requires scope-script source".into());
        }
        let record = self.run(source);
        let (final_value, error) = match &record.outcome {
            Ok(value) => (Some(crate::presentation::format_value(value)), None),
            Err(error) => {
                let (line, column) =
                    crate::presentation::source_location(&record.source.text, error.offset);
                (
                    None,
                    Some(ScriptPresentationError {
                        message: error.to_string(),
                        byte_offset: error.offset,
                        line,
                        column,
                    }),
                )
            }
        };
        Ok(ScriptPresentation {
            source_identity: record.source.identity.clone(),
            source_revision: record.source.revision,
            source_text: record.source.text.clone(),
            prints: record
                .printed
                .iter()
                .map(crate::presentation::format_value)
                .collect(),
            final_value,
            error,
        })
    }

    fn invoke_script(
        &mut self,
        source: &ScratchBuffer,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let record = self.run(source);
        write_record(record, output)
    }

    fn invoke_script_context(
        &mut self,
        source: &ScratchBuffer,
        _target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let record = self.run(source);
        if let Ok(value) = &record.outcome {
            for printed in &record.printed {
                writeln!(output, "{}", crate::presentation::format_value(printed))?;
            }
            writeln!(output, "{}", crate::presentation::format_value(value))?;
            Ok(())
        } else {
            write_record(record, output)
        }
    }

    fn inspect_script_evidence(
        &self,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let record = self.records.last().ok_or("no retained Script evaluation")?;
        write_record(record, output)?;
        writeln!(output, "source snapshot:\n{}", record.source.text)?;
        Ok(())
    }

    fn complete_source(
        &self,
        _source: &ScratchBuffer,
        draft: &str,
        cursor: usize,
    ) -> Vec<scope_ide::language::CompletionItem> {
        use scope_terminal::CompletionProvider;
        let interpreter = Interpreter::new();
        crate::completion::ScriptCompletionProvider::new(&interpreter, &NoHost)
            .complete(draft, cursor)
            .into_iter()
            .map(|item| scope_ide::language::CompletionItem {
                label: item.label.unwrap_or_else(|| item.insertion.clone()),
                replacement: item.replacement,
                insert_text: item.insertion,
                detail: Some("Script authoring (syntax-only bindings)".into()),
            })
            .collect()
    }

    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Err("pure workbench requires selected scratch source via :scope script".into())
    }
}

fn write_record(
    record: &PureRunRecord,
    output: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(
        output,
        "script: {:?} revision {:?}",
        record.source.identity, record.source.revision
    )?;
    for value in &record.printed {
        writeln!(
            output,
            "print: {}",
            crate::presentation::format_value(value)
        )?;
    }
    match &record.outcome {
        Ok(value) => {
            writeln!(
                output,
                "final: {}",
                crate::presentation::format_value(value)
            )?;
            writeln!(output, "status: interpreter success")?;
        }
        Err(error) => {
            let (line, column) =
                crate::presentation::source_location(&record.source.text, error.offset);
            writeln!(
                output,
                "diagnostic: {:?} revision {:?} {line}:{column} (Unicode scalars; byte offset {})",
                record.source.identity, record.source.revision, error.offset
            )?;
            writeln!(output, "status: interpreter failure")?;
            writeln!(output, "error: {error}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use scope_ide::scratch::ScratchBuffers;

    struct TestSource(ScratchBuffers);

    impl std::ops::Deref for TestSource {
        type Target = ScratchBuffer;
        fn deref(&self) -> &ScratchBuffer {
            self.0.selected().unwrap()
        }
    }

    fn source(text: &str) -> TestSource {
        let mut buffers = ScratchBuffers::default();
        buffers.create("script", text);
        TestSource(buffers)
    }

    fn invoke(runner: &mut WorkbenchRunner, source: &ScratchBuffer) -> String {
        let mut output = Vec::new();
        runner.invoke_script(source, &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    fn direct(runner: &mut WorkbenchRunner, text: &str) -> ScriptPresentation {
        let mut buffers = ScratchBuffers::default();
        buffers.create("scope-script", text);
        runner
            .invoke_script_direct(buffers.selected().unwrap())
            .unwrap()
    }

    #[test]
    fn context_success_is_concise_but_evidence_and_failures_are_detailed() {
        let mut runner = WorkbenchRunner::default();
        let mut output = Vec::new();
        assert!(runner.inspect_script_evidence(&mut output).is_err());
        runner
            .invoke_script_context(&source("print(7); 42"), None, &mut output)
            .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "7\n42\n");
        let mut evidence = Vec::new();
        runner.inspect_script_evidence(&mut evidence).unwrap();
        let evidence = String::from_utf8(evidence).unwrap();
        assert!(evidence.contains("final: 42\nstatus: interpreter success"));
        assert!(evidence.contains("source snapshot:\nprint(7); 42"));
        assert_eq!(runner.records.len(), 1);
        let mut failure = Vec::new();
        runner
            .invoke_script_context(&source("print(7); missing"), None, &mut failure)
            .unwrap();
        let failure = String::from_utf8(failure).unwrap();
        assert!(failure.contains("print: 7"));
        assert!(failure.contains("diagnostic:"));
        assert!(failure.contains("status: interpreter failure"));
        assert_eq!(runner.records.len(), 2);
    }

    #[test]
    fn shell_limits_concise_presentation_to_active_context() {
        use scope_ide::{app::App, cli, language::InteractiveSession, workspace::Workspace};
        let root = tempfile::tempdir().unwrap();
        let app = App::builder(Workspace::read_only(root.path().to_str().unwrap()).unwrap())
            .build()
            .unwrap();
        let mut runner = WorkbenchRunner::default();
        let mut output = Vec::new();
        cli::run_with_session_and_command(
            app,
            InteractiveSession::new(),
            &mut runner,
            concat!(
                ":scope script enter\n42\n:run\n:scope script evidence\n",
                ":scope script leave\n:scope script run\n:scope quit\n"
            )
            .as_bytes(),
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(runner.records.len(), 3, "inspection must not execute");
        let before_evidence = output.split("script:").next().unwrap();
        assert_eq!(before_evidence.matches("42\n").count(), 2);
        assert!(!before_evidence.contains("status: interpreter success"));
        assert_eq!(output.matches("status: interpreter success").count(), 2);
        assert_eq!(output.matches("source snapshot:").count(), 1);
    }

    #[test]
    fn direct_success_and_owned_provenance() {
        let mut runner = WorkbenchRunner::default();
        assert!(runner.supports_direct_script());
        let result = direct(&mut runner, "print(1); print([2, 3]); 42");
        let record = &runner.records[0];
        assert_eq!(result.source_identity, record.source.identity);
        assert_eq!(result.source_revision, record.source.revision);
        assert_eq!(result.source_text, record.source.text);
        assert_eq!(result.prints, ["1", "[2, 3]"]);
        assert_eq!(result.final_value.as_deref(), Some("42"));
        assert!(result.error.is_none());
        direct(&mut runner, "99");
        drop(runner);
        assert_eq!(result.source_text, "print(1); print([2, 3]); 42");
    }

    #[test]
    fn direct_failure_retains_prints_and_location() {
        let mut runner = WorkbenchRunner::default();
        let text = "print(\"雪😀\"); print(2);\nmissing";
        let result = direct(&mut runner, text);
        assert_eq!(result.prints, ["\"雪😀\"", "2"]);
        assert!(result.final_value.is_none());
        let error = result.error.unwrap();
        assert_eq!(error.byte_offset, text.find("missing").unwrap());
        assert_eq!((error.line, error.column), (2, 1));
        assert_eq!(
            error.message,
            runner.records[0].outcome.as_ref().unwrap_err().to_string()
        );
        let parse = direct(&mut runner, "print(1); if true { print([) }");
        assert!(parse.error.is_some());
        assert!(parse.prints.is_empty());
    }

    #[test]
    fn direct_has_no_host_and_rejects_other_languages() {
        let mut runner = WorkbenchRunner::default();
        assert_eq!(
            direct(&mut runner, "scope.operations()")
                .final_value
                .as_deref(),
            Some("[]")
        );
        for provider in ["semantic", "workspace", "runtime", "filesystem"] {
            assert!(
                direct(
                    &mut runner,
                    &format!("scope.invoke(\"{provider}\", \"write\", {{}})")
                )
                .error
                .is_some()
            );
        }
        let count = runner.records.len();
        assert!(runner.invoke_script_direct(&source("42")).is_err());
        assert_eq!(runner.records.len(), count);
    }

    #[test]
    fn invocations_and_completion_are_fresh() {
        let mut runner = WorkbenchRunner::default();
        let first = source("let private_binding = 42; print(private_binding)");
        invoke(&mut runner, &first);
        let second = source("private_binding");
        invoke(&mut runner, &second);
        assert!(runner.records[1].outcome.is_err());
        assert!(runner.records[1].printed.is_empty());
        assert!(runner.complete_source(&first, "private_", 8).is_empty());
        let items = runner.complete_source(&first, "scope.i", 7);
        assert_eq!(items[0].insert_text, "scope.invoke");
        assert_eq!(items[0].replacement, 0..7);
        assert_eq!(runner.records.len(), 2);
    }

    #[test]
    fn conditionals_short_circuit_and_format_values() {
        let mut runner = WorkbenchRunner::default();
        let output = invoke(
            &mut runner,
            &source(
                "if false { scope.invoke(\"native\", \"write\", {}) } else { print({items: [1, 2]}) }; 42",
            ),
        );
        assert_eq!(runner.records[0].printed.len(), 1);
        assert!(output.contains("print: {\n  \"items\": [1, 2]\n}"));
        assert!(output.contains("final: 42\nstatus: interpreter success"));
        assert!(!output.contains("semantic status"));
        assert!(!output.contains("target:"));
    }

    #[test]
    fn errors_retain_ordered_prints_and_unicode_locations() {
        let mut runner = WorkbenchRunner::default();
        let text = "print(\"雪😀\"); print(2); missing";
        let output = invoke(&mut runner, &source(text));
        let record = &runner.records[0];
        assert_eq!(
            record.printed,
            [Value::String("雪😀".into()), Value::Integer(2)]
        );
        let error = record.outcome.as_ref().unwrap_err();
        assert_eq!(error.offset, text.find("missing").unwrap());
        let (line, column) = crate::presentation::source_location(text, error.offset);
        assert!(output.contains(&format!(
            "{line}:{column} (Unicode scalars; byte offset {}",
            error.offset
        )));
        assert!(output.find("print: \"雪😀\"").unwrap() < output.find("print: 2").unwrap());
        assert!(output.find("print: 2").unwrap() < output.find("diagnostic:").unwrap());
        assert!(output.contains("status: interpreter failure"));
        invoke(&mut runner, &source("print(1); if true { print([) }"));
        assert!(runner.records[1].outcome.is_err());
        assert!(runner.records[1].printed.is_empty());
    }

    #[test]
    fn no_native_authority_or_provider_completions() {
        let mut runner = WorkbenchRunner::default();
        assert_eq!(
            runner
                .run(&source("scope.operations()"))
                .outcome
                .as_ref()
                .unwrap(),
            &Value::Array(vec![])
        );
        for provider in ["semantic", "workspace", "runtime", "filesystem"] {
            let text = format!("scope.invoke(\"{provider}\", \"write\", {{}})");
            assert!(runner.run(&source(&text)).outcome.is_err());
        }
        let draft = "scope.invoke(\"";
        assert!(
            runner
                .complete_source(&source(""), draft, draft.len())
                .is_empty()
        );
    }

    #[test]
    fn provenance_is_owned_and_writer_errors_are_returned() {
        let mut runner = WorkbenchRunner::default();
        let mut buffers = ScratchBuffers::default();
        buffers.create("script", "print(7); 9");
        let original = buffers.selected().unwrap();
        let identity = original.evaluation_source_id().clone();
        let revision = RevisionId::from_version(original.version());
        runner.run(original);
        buffers.create("script", "different");
        drop(buffers);
        let record = &runner.records[0];
        assert_eq!(record.source.identity, identity);
        assert_eq!(record.source.revision, revision);
        assert_eq!(record.source.text.as_bytes(), b"print(7); 9");

        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "closed",
                ))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let error = runner
            .invoke_script(&source("print(1); missing"), &mut BrokenWriter)
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::BrokenPipe
        );
        assert_eq!(runner.records.len(), 2);
        assert_eq!(runner.records[1].printed, [Value::Integer(1)]);
        assert!(runner.records[1].outcome.is_err());
    }
}
