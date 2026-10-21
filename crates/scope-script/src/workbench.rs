//! Pure, invocation-local scratch execution without a document or native authority.
use crate::{Error, Interpreter, Value, host::NoHost, runner::Snapshot};
use scope_ide::{
    cli::ReadOnlyCommand, document::Document, scratch::ScratchBuffer, text::TextRange,
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

/// A pure workbench: no target, session, installed providers, or persistent bindings.
#[derive(Default)]
pub struct WorkbenchRunner {
    pub records: Vec<PureRunRecord>,
}

impl WorkbenchRunner {
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
    fn invoke_script(
        &mut self,
        source: &ScratchBuffer,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let record = self.run(source);
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
                detail: Some("Script authoring (no live bindings)".into()),
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
