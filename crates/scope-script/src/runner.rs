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
use std::{collections::BTreeMap, io::Write};

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

/// Explicitly installed semantic reads. `None` and `Some(false)` deny execution.
/// Discovery and pure scripts require no authorization. This grants no workspace,
/// runtime, file persistence, proposal, or source-edit access to scripts.
pub struct ScriptRunner<'a> {
    service: &'a dyn LanguageService,
    bindings: BTreeMap<String, Option<bool>>,
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
            bindings: BTreeMap::from([(
                format!("language.{}.{}", service.language_id(), operation_id),
                authorization,
            )]),
            records: Vec::new(),
        }
    }

    /// Expose existing language-local read operation IDs against one bound target.
    /// Authorization is independent for each entry; missing permission denies calls.
    /// Empty, duplicate, unknown, and non-read selections are rejected.
    pub fn with_operations(
        service: &'a dyn LanguageService,
        operations: impl IntoIterator<Item = (String, Option<bool>)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let descriptions = service.semantic_operations();
        let mut bindings = BTreeMap::new();
        for (id, authorization) in operations {
            let action = format!("language.{}.{id}", service.language_id());
            if bindings.contains_key(&action) {
                return Err(format!("duplicate script operation: {id}").into());
            }
            let description = descriptions
                .iter()
                .find(|description| description.id == id)
                .ok_or_else(|| format!("unknown script operation: {id}"))?;
            if description.effect != scope_language::SemanticOperationEffect::Read {
                return Err(
                    format!("unsupported script operation effect: {id} is not Read").into(),
                );
            }
            bindings.insert(action, authorization);
        }
        if bindings.is_empty() {
            return Err("script operations must not be empty".into());
        }
        Ok(Self {
            service,
            bindings,
            records: Vec::new(),
        })
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
        if !self.bindings.contains_key(operation) {
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
        let operation = self
            .bindings
            .keys()
            .next()
            .expect("runner has a binding")
            .clone();
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
        let bindings = &self.bindings;
        let language = self.service.language_id();
        let provider = SemanticToolProvider::new(
            self.service,
            target,
            &mut controller,
            PermittedEditScope::Document(target.source_id()),
            Some(selection),
            move |request| {
                let action = format!("language.{language}.{}", request.operation.id);
                bindings.get(&action) == Some(&Some(true))
                    && request.operation.effect == scope_language::SemanticOperationEffect::Read
            },
        )?;
        let session = AgentSession::builder().provider(provider).build()?;
        let mut host = SemanticHost::with_bindings(
            session,
            self.bindings
                .iter()
                .map(|(action, authorization)| (action.clone(), *authorization)),
            target.text(),
            target.version(),
        )?;
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
// Completion advertises all explicitly installed actions; it carries no authority.
struct AuthoringHost<'a>(&'a BTreeMap<String, Option<bool>>);
impl Host for AuthoringHost<'_> {
    fn operation_names(&self) -> Vec<(String, String)> {
        self.0
            .keys()
            .map(|action| ("semantic".into(), action.clone()))
            .collect()
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
    fn reduce_script_snapshot(
        &self,
        snapshot: scope_language::SourceSnapshot,
    ) -> Result<scope_ide::cli::ScriptReduction, String> {
        reduce_snapshot(snapshot)
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

    fn invoke_script_with_target(
        &mut self,
        source: &ScratchBuffer,
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        ScriptRunner::invoke_script_with_target(self, source, target, output)
    }

    fn invoke_script_context(
        &mut self,
        source: &ScratchBuffer,
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Evaluate through the same binding/authorization path exactly once.
        let mut detailed = Vec::new();
        self.invoke_script_with_target(source, target, &mut detailed)?;
        let record = self.records.last().ok_or("no retained Script evaluation")?;
        if let Ok(value) = &record.outcome {
            for printed in &record.printed {
                writeln!(output, "{}", crate::presentation::format_value(printed))?;
            }
            writeln!(output, "{}", crate::presentation::format_value(value))?;
            Ok(())
        } else {
            output.write_all(&detailed)?;
            Ok(())
        }
    }

    fn inspect_script_evidence(
        &self,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let record = self.records.last().ok_or("no retained Script evaluation")?;
        write_record(record, output)?;
        writeln!(output, "source snapshot:\n{}", record.source.text)?;
        writeln!(output, "target snapshot:\n{}", record.target.text)?;
        writeln!(
            output,
            "catalog: {}",
            crate::presentation::format_value(&record.catalog)
        )?;
        for result in &record.results {
            writeln!(
                output,
                "envelope: {}",
                crate::presentation::format_value(result)
            )?;
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
        let host = AuthoringHost(&self.bindings);
        crate::completion::ScriptCompletionProvider::new(&interpreter, &host)
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

/// Bridge reducer byte offsets to IDE positions against the exact captured text.
pub(crate) fn reduce_snapshot(
    snapshot: scope_language::SourceSnapshot,
) -> Result<scope_ide::cli::ScriptReduction, String> {
    let candidate = crate::reduction::reduce(&Snapshot {
        identity: snapshot.source_id.clone(),
        revision: snapshot.revision_id,
        text: snapshot.text.to_string(),
    })
    .map_err(|refusal| {
        match refusal {
            crate::reduction::Refusal::InvalidSource => "Script reduction refused: invalid source",
            crate::reduction::Refusal::Unsupported => {
                "Script reduction refused: unsupported source"
            }
        }
        .to_owned()
    })?;
    let buffer = TextBuffer::new(snapshot.text.as_ref());
    let edits = candidate
        .edits
        .into_iter()
        .map(|edit| {
            let range = buffer
                .range_for_span(Span {
                    start: edit.range.start,
                    end: edit.range.end,
                })
                .map_err(|error| error.to_string())?;
            Ok(scope_ide::text::TextEdit::replace(range, edit.text))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(scope_ide::cli::ScriptReduction {
        original: snapshot,
        edits,
        rule: candidate.rule.into(),
        checked: candidate
            .checked
            .iter()
            .map(|check| (*check).to_owned())
            .collect(),
    })
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
