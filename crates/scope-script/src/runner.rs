//! Invocation-local scratch scripts over a separately bound, read-only document.
use crate::{Error, Host, Interpreter, Value, semantic::SemanticHost};
use scope_ide::{
    cli::ReadOnlyCommand,
    document::Document,
    scratch::ScratchBuffer,
    source_edit::{DocumentSource, PermittedEditScope, SourceEditController, SourceTarget},
    text::{Span, TextBuffer, TextRange},
    tool::{AgentSession, semantic::SemanticToolProvider},
};
use scope_language::{LanguageService, RevisionId, SourceId};
use std::io::Write;

/// Owned provenance: records never borrow a live shell buffer.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub identity: SourceId,
    pub revision: RevisionId,
    pub text: String,
}

#[derive(Debug)]
pub struct RunRecord {
    pub operation: String,
    pub source: Snapshot,
    pub target: Snapshot,
    pub selection: TextRange,
    /// Full discovery projection, including opaque language metadata.
    pub catalog: Value,
    /// Full semantic envelopes, independent of script projections or final value.
    pub results: Vec<Value>,
    pub printed: Vec<Value>,
    /// Original interpreter error, including its UTF-8 byte offset and error chain.
    pub outcome: Result<Value, Error>,
}

/// One explicitly installed semantic read. `None` and `Some(false)` deny execution.
/// Discovery and pure scripts require no authorization. This grants no workspace,
/// runtime, file persistence, proposal, or source-edit access to scripts.
pub struct ScriptRunner<'a> {
    service: &'a dyn LanguageService,
    action: String,
    authorization: Option<bool>,
    pub records: Vec<RunRecord>,
}
impl<'a> ScriptRunner<'a> {
    pub fn new(
        service: &'a dyn LanguageService,
        operation_id: &str,
        authorization: Option<bool>,
    ) -> Self {
        Self {
            service,
            action: format!("language.{}.{}", service.language_id(), operation_id),
            authorization,
            records: Vec::new(),
        }
    }

    /// Selection is in the target, never in the script. Setup errors precede
    /// evaluation and are returned unchanged; evaluation failures live in the record.
    pub fn run(
        &mut self,
        source: &ScratchBuffer,
        document: &Document,
        selection: TextRange,
        operation: &str,
    ) -> Result<&RunRecord, Box<dyn std::error::Error>> {
        if operation != self.action {
            return Err(format!("unavailable script operation: {operation}").into());
        }
        TextBuffer::new(document.text()).span_for_range(selection)?;
        let mut cloned = document.clone();
        let target = DocumentSource::new(&mut cloned, self.service.language_id());
        let target_snapshot = Snapshot {
            identity: document.evaluation_source_id().clone(),
            revision: RevisionId::from_version(target.version()),
            text: target.text().to_owned(),
        };
        self.evaluate(source, &target, target_snapshot, selection, operation)
    }

    /// Invocation-local entry point shared with the ReadOnlyCommand callback.
    pub fn invoke_script_with_target(
        &mut self,
        source: &ScratchBuffer,
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(target) = target else {
            return <Self as ReadOnlyCommand>::invoke_script(self, source, output);
        };
        let selection = TextBuffer::new(target.text()).range_for_span(Span {
            start: 0,
            end: target.text().len(),
        })?;
        let snapshot = Snapshot {
            identity: target.evaluation_source_id().clone(),
            revision: RevisionId::from_version(target.version()),
            text: target.text().to_owned(),
        };
        let operation = self.action.clone();
        let record = self.evaluate(source, target, snapshot, selection, &operation)?;
        write_record(record, output)
    }

    fn evaluate(
        &mut self,
        source: &ScratchBuffer,
        target: &impl SourceTarget,
        target_snapshot: Snapshot,
        selection: TextRange,
        operation: &str,
    ) -> Result<&RunRecord, Box<dyn std::error::Error>> {
        let source_snapshot = Snapshot {
            identity: source.evaluation_source_id().clone(),
            revision: RevisionId::from_version(source.version()),
            text: source.text().to_owned(),
        };
        let mut controller = SourceEditController::default();
        let allowed = self.authorization == Some(true);
        let provider = SemanticToolProvider::new(
            self.service,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(selection),
            move |request| {
                allowed && request.operation.effect == scope_language::SemanticOperationEffect::Read
            },
        )?;
        let session = AgentSession::builder().provider(provider).build()?;
        let mut host = SemanticHost::new(
            session,
            &self.action,
            target.text(),
            target.version(),
            self.authorization,
        );
        let catalog = host
            .operations()
            .map_err(|kind| Error { offset: 0, kind })?;
        let mut captured = CapturedHost {
            host: &mut host,
            results: Vec::new(),
        };
        let mut interpreter = Interpreter::new();
        let outcome = interpreter.eval(&source_snapshot.text, &mut captured);
        let printed = interpreter.take_output();
        self.records.push(RunRecord {
            operation: operation.to_owned(),
            source: source_snapshot,
            target: target_snapshot,
            selection,
            catalog,
            results: captured.results,
            printed,
            outcome,
        });
        Ok(self.records.last().unwrap())
    }
}
// Capture before script projection; this also works with hosts predating results.
struct CapturedHost<'a, H> {
    host: &'a mut H,
    results: Vec<Value>,
}
impl<H: Host> Host for CapturedHost<'_, H> {
    fn operations(&mut self) -> Result<Value, crate::ErrorKind> {
        self.host.operations()
    }
    fn invoke(
        &mut self,
        provider: &str,
        action: &str,
        arguments: Value,
    ) -> Result<Value, crate::ErrorKind> {
        let value = self.host.invoke(provider, action, arguments)?;
        self.results.push(value.clone());
        Ok(value)
    }
}
// Completion advertises only the explicitly installed action; it carries no authority.
struct AuthoringHost<'a>(&'a str);
impl Host for AuthoringHost<'_> {
    fn operation_names(&self) -> Vec<(String, String)> {
        vec![("semantic".into(), self.0.into())]
    }
    fn operations(&mut self) -> Result<Value, crate::ErrorKind> {
        Err(crate::ErrorKind::Runtime(
            "completion is metadata-only".into(),
        ))
    }
    fn invoke(&mut self, _: &str, _: &str, _: Value) -> Result<Value, crate::ErrorKind> {
        Err(crate::ErrorKind::Runtime(
            "completion cannot invoke operations".into(),
        ))
    }
}
impl ReadOnlyCommand for ScriptRunner<'_> {
    fn invoke_script_with_target(
        &mut self,
        source: &ScratchBuffer,
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        ScriptRunner::invoke_script_with_target(self, source, target, output)
    }

    fn complete_source(
        &self,
        _source: &ScratchBuffer,
        draft: &str,
        cursor: usize,
    ) -> Vec<scope_ide::language::CompletionItem> {
        use scope_terminal::CompletionProvider;
        let interpreter = Interpreter::new();
        let host = AuthoringHost(&self.action);
        crate::completion::ScriptCompletionProvider::new(&interpreter, &host)
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
        Err("script runner requires :scope run and selected scratch source".into())
    }
    fn invoke_source(
        &mut self,
        source: &ScratchBuffer,
        document: &Document,
        selection: TextRange,
        operation: &str,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let record = self.run(source, document, selection, operation)?;
        write_record(record, output)
    }
}

fn write_record(
    record: &RunRecord,
    output: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(
        output,
        "script: {:?} revision {:?}",
        record.source.identity, record.source.revision
    )?;
    writeln!(
        output,
        "target: {:?} revision {:?}",
        record.target.identity, record.target.revision
    )?;
    writeln!(output, "operation: {}", record.operation)?;
    writeln!(
        output,
        "host: {} returned envelopes retained (not semantic success; failures remain in interpreter error)",
        record.results.len()
    )?;
    writeln!(
        output,
        "semantic status: integration-defined; inspect retained results"
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
            writeln!(
                output,
                "status: interpreter success (see semantic envelopes for semantic status)"
            )?;
        }
        Err(error) => {
            let mut offset = error.offset.min(record.source.text.len());
            while !record.source.text.is_char_boundary(offset) {
                offset -= 1;
            }
            let prefix = &record.source.text[..offset];
            let line = 1 + prefix.bytes().filter(|byte| *byte == b'\n').count();
            let column = 1 + prefix.rsplit('\n').next().unwrap_or("").chars().count();
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
