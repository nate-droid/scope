use crate::app::{App, AppError};
use crate::path::ScopePath;
use crate::workspace::Workspace;

pub use crate::tool::{
    AgentSession, AgentSessionBuilder, ScopeToolProvider, SessionCatalog, ToolArguments, ToolCall,
    ToolCatalogEntry, ToolDefinition, ToolError, ToolInputDefinition, ToolInputKind, ToolProvider,
    ToolResult, ToolValue,
};

pub use cell_capability_openai::Message as DecisionMessage;
use cell_capability_openai::{ChatRequest, Message, OpenAiChat};

#[path = "agent_loop.rs"]
mod decision;
pub use decision::edit::{
    EditBudgets, EditCapabilities, EditDispatch, EditEvent, EditOutcome, EditPhase, EditRun,
    EditState, EditSummary, InvalidTransition, ModelCompletion, SemanticDiscoveryOptions,
    SemanticEditOptions, ToolResultClassification, run_edit_session, run_edit_session_with_state,
    run_semantic_edit, run_semantic_edit_with_discovery,
};
pub use decision::{
    AgentLimits, AgentRun, AgentStop, DecisionModel, read_only_policy, run_session,
};

const DEFAULT_TEMPERATURE: f32 = 0.2;
const DEFAULT_MAX_TOKENS: u32 = 700;
const SYSTEM_PROMPT: &str = "You are a small Scope coding assistant. You do not have shell access, process execution, ambient filesystem access, network access, or hidden tools. You may reason only from the context provided by Scope commands. If the context is insufficient, say which Scope command or file would help next. Prefer concise, actionable answers.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
}

impl AgentConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            base_url: None,
            api_key: None,
        }
    }

    pub fn local(model: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            base_url: Some(base_url.into()),
            api_key: None,
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }
}

#[derive(Clone)]
pub struct CellAgent {
    chat: OpenAiChat,
    cell: std::sync::Arc<cell::Cell>,
}

impl std::fmt::Debug for CellAgent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CellAgent")
            .field("capabilities", &self.capabilities())
            .finish_non_exhaustive()
    }
}

impl CellAgent {
    pub fn new(config: AgentConfig) -> Result<Self, AgentError> {
        let chat = match (config.base_url, config.api_key) {
            (None, Some(api_key)) => OpenAiChat::new(config.model, api_key),
            (None, None) => Err(anyhow::anyhow!(
                "OpenAI public endpoint requires an API key; use AgentConfig::local for a host-approved local endpoint"
            )),
            (Some(base_url), None) => OpenAiChat::local(config.model, base_url),
            (Some(base_url), Some(api_key)) => OpenAiChat::builder(config.model)
                .base_url(base_url)
                .api_key(api_key)
                .build(),
        }
        .map_err(AgentError::ConfigureChat)?;

        let cell = cell::Cell::new()
            .and_then(|cell| cell.with(chat.clone()))
            .map_err(AgentError::ConfigureChat)?;
        Ok(Self {
            chat,
            cell: std::sync::Arc::new(cell),
        })
    }

    /// Installed chat interfaces; listing does not make a network request.
    pub fn capabilities(&self) -> &[&'static str] {
        self.cell.capabilities()
    }

    /// Advisory installation snapshots, not authorization checks. No network request is made.
    pub fn capability_descriptors(&self) -> &[cell::CapabilityDescriptor] {
        self.cell.capability_descriptors()
    }

    /// Run installed session tools with explicit limits and host authorization.
    pub fn run_session(
        &self,
        session: &mut AgentSession<'_>,
        request: &str,
        limits: AgentLimits,
        policy: impl FnMut(&ToolDefinition, &ToolCall) -> bool,
    ) -> Result<AgentRun, AgentError> {
        run_session(&mut self.clone(), session, request, limits, policy)
    }

    /// Built-in CLI/shell path: bounded, read-only policy, active buffer context.
    pub fn run_app(
        &self,
        app: &mut App,
        request: &str,
        limits: AgentLimits,
    ) -> Result<AgentRun, AgentError> {
        let request = format!(
            "User request:\n{request}\n\nActive Scope context:\n{}",
            AgentContext::from_app(app).render()
        );
        let mut session = AgentSession::for_app(app).map_err(AgentError::Tool)?;
        self.run_session(&mut session, &request, limits, read_only_policy)
    }

    /// Host has already approved the exact snapshot paths. Model authority is
    /// limited to isolated creation/replacement; acceptance is never registered.
    pub fn run_proposal(
        &self,
        proposal: &mut crate::workspace::Proposal,
        request: &str,
        limits: AgentLimits,
    ) -> Result<AgentRun, AgentError> {
        let mut session = AgentSession::builder()
            .provider(crate::tool::ProposalToolProvider::new(proposal))
            .build()
            .map_err(AgentError::Tool)?;
        self.run_session(&mut session, request, limits, |definition, _| {
            definition.effect == crate::tool::ToolEffect::Read
                || matches!(
                    definition.name.as_str(),
                    "scope.document.create" | "scope.document.replace"
                )
        })
    }

    /// Legacy single-completion API; does not dispatch tools.
    pub fn answer(&self, app: &App, request: &str) -> Result<AgentResponse, AgentError> {
        let context = AgentContext::from_app(app);
        self.answer_with_context(context, request)
    }

    pub fn answer_workspace(
        &self,
        workspace: Workspace,
        request: &str,
        open_path: Option<ScopePath>,
    ) -> Result<AgentResponse, AgentError> {
        let mut app = App::new(workspace).map_err(AppError::Workspace)?;

        if let Some(path) = open_path {
            app.open_path(path)?;
        }

        self.answer(&app, request)
    }

    fn answer_with_context(
        &self,
        context: AgentContext,
        request: &str,
    ) -> Result<AgentResponse, AgentError> {
        let content = self
            .chat
            .complete(ChatRequest {
                messages: vec![
                    Message {
                        role: "system".to_string(),
                        content: SYSTEM_PROMPT.to_string(),
                    },
                    Message {
                        role: "user".to_string(),
                        content: format!(
                            "User request:\n{request}\n\nScope capability context:\n{}",
                            context.render()
                        ),
                    },
                ],
                temperature: Some(DEFAULT_TEMPERATURE),
                max_tokens: Some(DEFAULT_MAX_TOKENS),
            })
            .map_err(AgentError::Chat)?
            .content;

        Ok(AgentResponse { content })
    }
}

impl DecisionModel for CellAgent {
    fn decide(&mut self, messages: &[Message], max_tokens: u32) -> Result<String, String> {
        self.chat
            .complete(ChatRequest {
                messages: messages.to_vec(),
                temperature: Some(DEFAULT_TEMPERATURE),
                max_tokens: Some(max_tokens),
            })
            .map(|response| response.content)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentResponse {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentContext {
    workspace_root: String,
    files: Vec<String>,
    open_document: Option<OpenDocumentContext>,
}

impl AgentContext {
    pub(crate) fn from_app(app: &App) -> Self {
        let open_document = app.open_document().map(|document| {
            let analysis = app.open_analysis();
            let diagnostics = analysis
                .as_ref()
                .map(|analysis| {
                    analysis
                        .diagnostics
                        .iter()
                        .map(|diagnostic| {
                            format!(
                                "{}:{}-{}:{} {}",
                                diagnostic.range.start.line + 1,
                                diagnostic.range.start.column + 1,
                                diagnostic.range.end.line + 1,
                                diagnostic.range.end.column + 1,
                                diagnostic.message
                            )
                        })
                        .collect()
                })
                .unwrap_or_else(|error| vec![format!("<diagnostics unavailable: {error}>")]);

            let outline = analysis
                .as_ref()
                .map(|analysis| {
                    analysis
                        .symbols
                        .iter()
                        .map(|symbol| {
                            format!(
                                "{} {} {}:{}-{}:{}",
                                symbol.kind,
                                symbol.name,
                                symbol.range.start.line + 1,
                                symbol.range.start.column + 1,
                                symbol.range.end.line + 1,
                                symbol.range.end.column + 1
                            )
                        })
                        .collect()
                })
                .unwrap_or_else(|error| vec![format!("<outline unavailable: {error}>")]);

            OpenDocumentContext {
                path: document.path().display().to_string(),
                language: app.open_language().map(|language| {
                    format!("{} ({})", language.language_name(), language.language_id())
                }),
                text: document.text().to_string(),
                diagnostics,
                outline,
            }
        });

        Self {
            workspace_root: app.workspace().root().to_string(),
            files: app
                .files()
                .iter()
                .map(|entry| entry.path.display().to_string())
                .collect(),
            open_document,
        }
    }

    pub(crate) fn render(&self) -> String {
        let mut rendered = String::new();
        rendered.push_str("Available Scope commands: files, open <path>, show, language, outline, diagnostics. No shell command exists.\n");
        rendered.push_str(&format!("Workspace: {}\n\n", self.workspace_root));

        rendered.push_str("Files:\n");
        if self.files.is_empty() {
            rendered.push_str("<none>\n");
        } else {
            for file in &self.files {
                rendered.push_str("- ");
                rendered.push_str(file);
                rendered.push('\n');
            }
        }

        match &self.open_document {
            Some(document) => {
                rendered.push_str("\nOpen document:\n");
                rendered.push_str(&format!("Path: {}\n", document.path));
                rendered.push_str(&format!(
                    "Language: {}\n",
                    document.language.as_deref().unwrap_or("none")
                ));

                rendered.push_str("Diagnostics:\n");
                if document.diagnostics.is_empty() {
                    rendered.push_str("<clean>\n");
                } else {
                    for diagnostic in &document.diagnostics {
                        rendered.push_str("- ");
                        rendered.push_str(diagnostic);
                        rendered.push('\n');
                    }
                }

                rendered.push_str("Outline:\n");
                if document.outline.is_empty() {
                    rendered.push_str("<none>\n");
                } else {
                    for symbol in &document.outline {
                        rendered.push_str("- ");
                        rendered.push_str(symbol);
                        rendered.push('\n');
                    }
                }

                rendered.push_str("Contents:\n```\n");
                rendered.push_str(&document.text);
                if !document.text.ends_with('\n') {
                    rendered.push('\n');
                }
                rendered.push_str("```\n");
            }
            None => rendered.push_str("\nOpen document: <none>\n"),
        }

        rendered
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenDocumentContext {
    path: String,
    language: Option<String>,
    text: String,
    diagnostics: Vec<String>,
    outline: Vec<String>,
}

#[derive(Debug)]
pub enum AgentError {
    App(AppError),
    Chat(String),
    Tool(ToolError),
    Stopped(AgentStop),
    ConfigureChat(anyhow::Error),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::App(source) => source.fmt(formatter),
            Self::Tool(source) => source.fmt(formatter),
            Self::Stopped(stop) => write!(formatter, "agent stopped without completion: {stop:?}"),
            Self::Chat(message) => write!(formatter, "chat capability failed: {message}"),
            Self::ConfigureChat(source) => {
                write!(formatter, "failed to configure chat capability: {source}")
            }
        }
    }
}

impl std::error::Error for AgentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::App(source) => Some(source),
            Self::Chat(_) | Self::Stopped(_) => None,
            Self::Tool(source) => Some(source),
            Self::ConfigureChat(source) => Some(source.as_ref()),
        }
    }
}

impl From<AppError> for AgentError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempWorkspace;

    #[test]
    fn renders_agent_context_without_shell_authority() {
        let root = TempWorkspace::new("renders_agent_context_without_shell_authority");
        root.write_file("main.scope", "fn main() {\n  let answer = 42\n}\n");

        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("main.scope").unwrap();
        let rendered = AgentContext::from_app(&app).render();

        assert!(rendered.contains("Available Scope commands: files, open <path>, show, language, outline, diagnostics. No shell command exists."));
        assert!(rendered.contains("- main.scope"));
        assert!(rendered.contains("Language: Scope Simple (scope.simple)"));
        assert!(rendered.contains("fn main()"));
    }

    #[test]
    fn context_requests_one_combined_analysis() {
        use scope_language::{
            Diagnostic, LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, Symbol,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingLanguage(AtomicUsize);

        impl LanguageService for CountingLanguage {
            fn language_id(&self) -> &'static str {
                "test.counting"
            }
            fn language_name(&self) -> &'static str {
                "Counting"
            }
            fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
                path.extension() == Some("counting")
            }
            fn diagnostics(&self, _: LanguageDocument<'_>) -> Vec<Diagnostic> {
                panic!("context should request combined analysis")
            }
            fn outline(&self, _: LanguageDocument<'_>) -> Vec<Symbol> {
                panic!("context should request combined analysis")
            }
            fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
                self.0.fetch_add(1, Ordering::SeqCst);
                LanguageAnalysis::default()
            }
        }

        let language = Box::leak(Box::new(CountingLanguage(AtomicUsize::new(0))));
        let root = TempWorkspace::new("agent_combined_analysis");
        root.write_file("main.counting", "example");
        let mut app = App::builder(root.workspace())
            .language(language)
            .build()
            .unwrap();
        app.open_path("main.counting").unwrap();

        let context = AgentContext::from_app(&app);
        assert_eq!(language.0.load(Ordering::SeqCst), 1);
        let document = context.open_document.unwrap();
        assert!(document.diagnostics.is_empty());
        assert!(document.outline.is_empty());
    }

    #[test]
    fn lists_installed_chat_capability_without_contacting_endpoint() {
        let agent = CellAgent::new(AgentConfig::local(
            "test-model",
            "http://localhost:11434/v1",
        ))
        .unwrap();
        assert_eq!(agent.capabilities(), &["cell:openai/chat@0.1.0"]);
        assert_eq!(agent.capability_descriptors().len(), 1);
        assert_eq!(
            agent.capability_descriptors()[0].interface,
            "cell:openai/chat@0.1.0"
        );
        assert_eq!(
            agent.clone().capability_descriptors(),
            agent.capability_descriptors()
        );
        assert_eq!(agent.clone().capabilities(), agent.capabilities());
    }

    #[test]
    fn public_openai_agent_requires_host_configured_api_key() {
        let error = CellAgent::new(AgentConfig::new("gpt-4o-mini")).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("OpenAI public endpoint requires an API key")
        );
    }
}
