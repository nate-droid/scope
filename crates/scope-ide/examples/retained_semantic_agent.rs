//! Run with: cargo run -p scope-ide --example retained_semantic_agent --features agent
//! No endpoint, credentials, Hilb runtime, or filesystem writes are needed.
#[cfg(not(feature = "agent"))]
fn main() {
    eprintln!("Enable this example with --features agent");
}

#[cfg(feature = "agent")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    demo::run()
}

#[cfg(feature = "agent")]
mod demo {
    use scope_ide::{
        agent::{AgentLimits, DecisionMessage, DecisionModel},
        app::App,
        cli::{
            ReadOnlyCommand, RetainedScratchAgentRun, SemanticAgentBinding,
            run_borrowed_with_sources,
        },
        document::Document,
        language::{InteractiveSession, LanguageRegistry},
        scratch::ScratchBuffers,
        text::TextRange,
        workspace::Workspace,
    };
    use scope_language::*;
    use serde_json::json;
    use std::io::{Cursor, Write};

    struct Notes;
    impl LanguageService for Notes {
        fn language_id(&self) -> &'static str {
            "retained-notes"
        }
        fn language_name(&self) -> &'static str {
            "Retained notes"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            false
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            Default::default()
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            vec![SemanticOperationDescription {
                id: "inspect".into(),
                description: "Read the live, unsaved notes without changing them".into(),
                arguments: vec![],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            }]
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            Ok(SemanticOperationResult::ReadOnly {
                value: json!({"text": request.document.text(), "version": request.document.version()}),
                rationale: "Read only; no candidate installation or save".into(),
            })
        }
    }

    struct ScriptedModel(bool);
    impl DecisionModel for ScriptedModel {
        fn decide(&mut self, messages: &[DecisionMessage], _: u32) -> Result<String, String> {
            if !self.0 {
                self.0 = true;
                Ok(json!({"type":"tool", "name":"language.retained-notes.inspect", "arguments":{}}).to_string())
            } else {
                let feedback = messages
                    .last()
                    .map(|message| message.content.as_str())
                    .unwrap_or("");
                Ok(json!({"type":"complete", "content":format!("Observed retained notes: {feedback}")}).to_string())
            }
        }
    }

    struct Host<'a> {
        model: ScriptedModel,
        allowed: Vec<String>,
        authorize:
            &'a mut dyn for<'b> FnMut(scope_ide::tool::semantic::SemanticAuthorization<'b>) -> bool,
        observe: &'a mut dyn FnMut(&RetainedScratchAgentRun<'_>),
    }
    impl ReadOnlyCommand for Host<'_> {
        fn invoke(
            &mut self,
            _: &Document,
            _: TextRange,
            _: &str,
            _: &mut dyn Write,
        ) -> Result<(), Box<dyn std::error::Error>> {
            Err("This example exposes only the retained semantic agent".into())
        }
        fn semantic_agent(&mut self) -> Option<SemanticAgentBinding<'_>> {
            Some(SemanticAgentBinding {
                service: &Notes,
                trace: None,
                allowed_operation_ids: &self.allowed,
                authorize: self.authorize,
                limits: AgentLimits::default(),
                model: Some(&mut self.model),
                observe: self.observe,
            })
        }
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = App::builder(Workspace::read_only(env!("CARGO_MANIFEST_DIR"))?)
            .language_registry(LanguageRegistry::empty().with_language(&Notes))
            .build()?;
        let mut session = InteractiveSession::new();
        let mut sources = ScratchBuffers::default();
        let mut authorize =
            |authorization: scope_ide::tool::semantic::SemanticAuthorization<'_>| {
                authorization.operation.id == "inspect"
                    && authorization.source.language == "retained-notes"
            };
        let mut observe = |result: &RetainedScratchAgentRun<'_>| {
            eprintln!(
                "Host attribution: {:?}, revision {:?}, stop {:?}",
                result.source.id, result.source.version, result.run.stop
            );
        };
        let mut host = Host {
            model: ScriptedModel(false),
            allowed: vec!["inspect".into()],
            authorize: &mut authorize,
            observe: &mut observe,
        };
        // This is the ordinary stream shell, not a direct runner invocation.
        run_borrowed_with_sources(
            &mut app,
            &mut session,
            Some(&mut host),
            &mut sources,
            Cursor::new(concat!(
                ":scope scratch new retained-notes\n",
                ":scope line 1 \"Unsaved notes: café 🦀\"\n",
                ":scope semantic-target 0\n",
                ":scope agent inspect my retained notes\n",
                ":scope quit\n",
            )),
            std::io::stdout(),
        )?;
        println!(
            "After shell exit (still caller-owned): {:?}",
            sources.bound_source()
        );
        Ok(())
    }
}
