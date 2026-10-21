use scope_ide::{
    app::App,
    cli::{self, ReadOnlyCommand},
    document::Document,
    language::{
        EvaluationResult, EvaluationStatus, InspectionError, InteractiveLanguageSession,
        InteractiveSession, InteractiveSessionProvider, LanguageRegistry, SessionError,
        SymbolDescription,
    },
    scratch::ScratchBuffer,
    text::{Position, TextEdit, TextRange},
    workspace::Workspace,
};
use scope_script::workbench::WorkbenchRunner;
use std::{collections::BTreeMap, io::Write, sync::Mutex};

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Create,
    Evaluate(String),
    Reset,
    Close,
}

struct Provider(Mutex<Vec<Event>>);
struct Session {
    provider: &'static Provider,
    bindings: BTreeMap<String, String>,
}

// Each test owns a separate static registration, as required by LanguageRegistry.
impl InteractiveSessionProvider for &'static Provider {
    fn language_id(&self) -> &'static str {
        "downstream"
    }

    fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
        self.0.lock().unwrap().push(Event::Create);
        Ok(Box::new(Session {
            provider: self,
            bindings: BTreeMap::new(),
        }))
    }
}

impl InteractiveLanguageSession for Session {
    fn language_id(&self) -> &str {
        "downstream"
    }

    fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
        self.provider
            .0
            .lock()
            .unwrap()
            .push(Event::Evaluate(source.into()));
        let output = if let Some((name, value)) =
            source.strip_prefix("bind ").and_then(|s| s.split_once(' '))
        {
            self.bindings.insert(name.into(), value.into());
            format!("bound {name}={value}")
        } else {
            format!(
                "downstream {source}={}",
                self.bindings
                    .get(source)
                    .map(String::as_str)
                    .unwrap_or("missing")
            )
        };
        Ok(EvaluationResult {
            output: vec![output],
            diagnostics: vec![],
            status: EvaluationStatus::Success,
        })
    }

    fn inspect(&self, _: &str) -> Result<Option<SymbolDescription>, InspectionError> {
        Ok(None)
    }

    fn reset(&mut self) -> Result<(), SessionError> {
        self.provider.0.lock().unwrap().push(Event::Reset);
        self.bindings.clear();
        Ok(())
    }

    fn close(&mut self) -> Result<(), SessionError> {
        self.provider.0.lock().unwrap().push(Event::Close);
        Ok(())
    }
}

struct Host {
    runner: WorkbenchRunner,
    expected: Document,
    checks: usize,
}

impl ReadOnlyCommand for Host {
    fn invoke_script(
        &mut self,
        source: &ScratchBuffer,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.runner.invoke_script(source, output)
    }

    fn invoke(
        &mut self,
        document: &Document,
        _: TextRange,
        operation: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(operation, "check");
        assert_eq!(
            document, &self.expected,
            "script operations changed the live document"
        );
        self.checks += 1;
        Ok(())
    }
}

#[test]
fn scratch_edit_and_real_script_runs_preserve_active_bindings_and_unsaved_document() {
    let root = tempfile::tempdir().unwrap();
    let disk = "fn original() {}\n";
    std::fs::write(root.path().join("target.scope"), disk).unwrap();
    let provider: &'static Provider = Box::leak(Box::new(Provider(Mutex::new(vec![]))));
    let registry =
        LanguageRegistry::builtins().with_interactive_provider(Box::leak(Box::new(provider)));
    let mut app = App::builder(Workspace::read_only(root.path().to_str().unwrap()).unwrap())
        .language_registry(registry)
        .build()
        .unwrap();
    app.open_path("target.scope").unwrap();
    app.open_document_mut()
        .unwrap()
        .apply_edit(TextEdit::insert(Position::new(0, 0), "# unsaved 雪\n"))
        .unwrap();
    let mut host = Host {
        runner: WorkbenchRunner::default(),
        expected: app.open_document().unwrap().clone(),
        checks: 0,
    };
    let input = concat!(
        ":scope session downstream\nbind answer 91\n",
        ":scope host check --selection 0:0..0:0\n",
        ":scope scratch new script\n:scope edit\nlet answer = 7; print(answer); answer\n:done\n",
        "answer\n:scope script run\nanswer\n:scope script run\nanswer\n",
        ":scope edit\n:at 1 1\n:delete 38\nprint(3); missing\n:done\n",
        ":scope script run\nanswer\n",
        ":scope host check --selection 0:0..0:0\n:scope session\n:scope quit\n",
    );
    let mut output = Vec::new();
    cli::run_with_session_and_command(
        app,
        InteractiveSession::new(),
        &mut host,
        input.as_bytes(),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert_eq!(host.checks, 2, "{output}");
    assert_eq!(host.runner.records.len(), 3, "{output}");
    assert_eq!(
        output
            .matches("final: 7\nstatus: interpreter success")
            .count(),
        2,
        "{output}"
    );
    assert!(output.contains("status: interpreter failure"), "{output}");
    assert_eq!(
        output.matches("downstream answer=91").count(),
        4,
        "{output}"
    );
    assert!(output.contains("Active language: downstream"), "{output}");
    assert_eq!(
        *provider.0.lock().unwrap(),
        vec![
            Event::Create,
            Event::Evaluate("bind answer 91".into()),
            Event::Evaluate("answer".into()),
            Event::Evaluate("answer".into()),
            Event::Evaluate("answer".into()),
            Event::Evaluate("answer".into()),
            Event::Close
        ]
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("target.scope")).unwrap(),
        disk
    );
}

#[test]
fn file_extensions_select_analysis_but_never_execute_without_explicit_script_run() {
    for extension in ["scope", "script", "txt"] {
        let root = tempfile::tempdir().unwrap();
        let name = format!("source.{extension}");
        let source = "print(42); 7";
        std::fs::write(root.path().join(&name), source).unwrap();
        let mut app = App::builder(Workspace::read_only(root.path().to_str().unwrap()).unwrap())
            .build()
            .unwrap();
        app.open_path(&name).unwrap();
        if extension == "scope" {
            let language = app.open_language().unwrap();
            assert_eq!(language.language_id(), "scope.simple");
            assert_eq!(language.language_name(), "Scope Simple");
            let analysis = app.open_analysis().unwrap();
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("print")),
                "Scope Simple analysis must not be confused with Script execution"
            );
        } else {
            assert!(app.open_language().is_none());
        }
        let document = app.open_document().unwrap();
        let mut host = Host {
            runner: WorkbenchRunner::default(),
            expected: document.clone(),
            checks: 0,
        };
        let input = format!(
            ":scope scratch open {name}\n:scope show\n:scope host check --selection 0:0..0:0\n:scope quit\n"
        );
        let mut output = Vec::new();
        cli::run_with_session_and_command(
            app,
            InteractiveSession::new(),
            &mut host,
            input.as_bytes(),
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(
            host.runner.records.is_empty(),
            "extension auto-executed: {output}"
        );
        assert!(!output.contains("print: 42"), "{output}");
        assert_eq!(host.checks, 1, "{output}");

        let app = App::builder(Workspace::read_only(root.path().to_str().unwrap()).unwrap())
            .build()
            .unwrap();
        let input = format!(":scope scratch open {name}\n:scope script run\n:scope quit\n");
        let mut runner = WorkbenchRunner::default();
        let mut output = Vec::new();
        cli::run_with_session_and_command(
            app,
            InteractiveSession::new(),
            &mut runner,
            input.as_bytes(),
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(runner.records.len(), 1, "{output}");
        assert!(
            output.contains("print: 42\nfinal: 7\nstatus: interpreter success"),
            "{output}"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join(name)).unwrap(),
            source
        );
    }
}
