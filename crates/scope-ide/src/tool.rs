/// Reviewable full replacements produced by revision-bound operations.
pub mod editing;
pub mod inspection;
/// Discoverable, explicitly authorized operations bound to document revisions.
pub mod operations;
/// Language-provided semantic operations, bound to M1 source targets.
pub mod semantic;
#[cfg(test)]
mod semantic_tests;

use std::collections::BTreeMap;

use crate::app::App;
use crate::document::{Document, DocumentId};
use crate::text::{Position, TextEdit, TextRange};
use crate::workspace::WorkspaceError;
use std::time::{Duration, Instant};

/// A provider of namespaced semantic tools available to an agent session.
///
/// Tool providers are intentionally separate from Cell capabilities. A provider
/// describes and executes domain-level operations; it may use Cell capabilities
/// internally only where host authority is required.
pub trait ToolProvider {
    fn tools(&self) -> &[ToolDefinition];

    /// Optional discovery metadata; not an authority or routing identifier.
    fn provider_name(&self) -> Option<&str> {
        None
    }

    /// Describe successful output for a tool, if its contract is known.
    fn output_description(&self, _tool_name: &str) -> Option<&str> {
        None
    }

    /// Structured discovery data; never a capability or execution grant.
    fn metadata(&self, _tool_name: &str) -> Option<ToolValue> {
        None
    }

    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        Err(ToolError::execution(
            call.name,
            "provider requires a session app",
        ))
    }

    /// Execute with the session's app, borrowed only for this invocation.
    /// Providers must use the app's workspace APIs for host-authorized access;
    /// receiving an app does not grant additional filesystem or Cell authority.
    fn invoke_with_app(&mut self, call: ToolCall, _app: &mut App) -> Result<ToolResult, ToolError> {
        self.invoke(call)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolEffect {
    #[default]
    Read,
    Write,
}

/// Payload-free execution metadata. Observer callbacks are synchronous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolExecutionEvent {
    Started {
        call_id: u64,
        name: String,
    },
    Finished {
        call_id: u64,
        name: String,
        duration: Duration,
        outcome: ToolExecutionOutcome,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionOutcome {
    Success,
    UnknownTool,
    DuplicateTool,
    MissingArgument,
    InvalidArgument,
    Conflict,
    Denied,
    Execution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub effect: ToolEffect,
    pub name: String,
    pub description: String,
    pub inputs: Vec<ToolInputDefinition>,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            inputs: Vec::new(),
            effect: ToolEffect::Read,
        }
    }

    pub fn effect(mut self, effect: ToolEffect) -> Self {
        self.effect = effect;
        self
    }

    pub fn input(mut self, input: ToolInputDefinition) -> Self {
        self.inputs.push(input);
        self
    }
}

/// An owned discovery snapshot, independent of the session's lifetime.
/// Discovery describes installed tools, not policy authorization.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionCatalog {
    pub tools: Vec<ToolCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCatalogEntry {
    pub definition: ToolDefinition,
    pub provider: Option<String>,
    pub output_description: Option<String>,
    pub metadata: Option<ToolValue>,
}

impl SessionCatalog {
    pub fn lookup(&self, tool_name: &str) -> Option<&ToolCatalogEntry> {
        self.tools
            .iter()
            .find(|entry| entry.definition.name == tool_name)
    }

    /// Export as `{tools: [{definition, provider, output_description}]}`.
    /// Effects and input kinds use lowercase names; absent metadata is null.
    pub fn to_value(&self) -> ToolValue {
        ToolValue::object([(
            "tools",
            ToolValue::array(self.tools.iter().map(|entry| {
                let definition = &entry.definition;
                ToolValue::object([
                    (
                        "definition",
                        ToolValue::object([
                            ("name", definition.name.clone().into()),
                            ("description", definition.description.clone().into()),
                            (
                                "effect",
                                match definition.effect {
                                    ToolEffect::Read => "read",
                                    ToolEffect::Write => "write",
                                }
                                .into(),
                            ),
                            (
                                "inputs",
                                ToolValue::array(definition.inputs.iter().map(|input| {
                                    ToolValue::object([
                                        ("name", input.name.clone().into()),
                                        (
                                            "kind",
                                            match input.kind {
                                                ToolInputKind::String => "string",
                                                ToolInputKind::Integer => "integer",
                                                ToolInputKind::Boolean => "boolean",
                                                ToolInputKind::Object => "object",
                                                ToolInputKind::Array => "array",
                                            }
                                            .into(),
                                        ),
                                        ("description", input.description.clone().into()),
                                        ("required", input.required.into()),
                                    ])
                                })),
                            ),
                        ]),
                    ),
                    (
                        "metadata",
                        entry.metadata.clone().unwrap_or(ToolValue::Null),
                    ),
                    (
                        "provider",
                        entry
                            .provider
                            .clone()
                            .map(ToolValue::from)
                            .unwrap_or(ToolValue::Null),
                    ),
                    (
                        "output_description",
                        entry
                            .output_description
                            .clone()
                            .map(ToolValue::from)
                            .unwrap_or(ToolValue::Null),
                    ),
                ])
            })),
        )])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInputDefinition {
    pub name: String,
    pub kind: ToolInputKind,
    pub description: String,
    pub required: bool,
}

impl ToolInputDefinition {
    pub fn required(
        name: impl Into<String>,
        kind: ToolInputKind,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            description: description.into(),
            required: true,
        }
    }

    pub fn optional(
        name: impl Into<String>,
        kind: ToolInputKind,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            description: description.into(),
            required: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolInputKind {
    String,
    Integer,
    Boolean,
    Object,
    Array,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: ToolArguments,
}

impl ToolCall {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arguments: ToolArguments::new(),
        }
    }

    pub fn with_argument(mut self, name: impl Into<String>, value: ToolValue) -> Self {
        self.arguments.insert(name.into(), value);
        self
    }
}

pub type ToolArguments = BTreeMap<String, ToolValue>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub value: ToolValue,
}

impl ToolResult {
    pub fn new(value: ToolValue) -> Self {
        Self { value }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolValue {
    Null,
    Boolean(bool),
    Integer(i64),
    String(String),
    Array(Vec<ToolValue>),
    Object(BTreeMap<String, ToolValue>),
}

impl ToolValue {
    pub fn object(entries: impl IntoIterator<Item = (impl Into<String>, ToolValue)>) -> Self {
        Self::Object(
            entries
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        )
    }

    pub fn array(values: impl IntoIterator<Item = ToolValue>) -> Self {
        Self::Array(values.into_iter().collect())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }
}

impl From<&str> for ToolValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for ToolValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<usize> for ToolValue {
    fn from(value: usize) -> Self {
        Self::Integer(value as i64)
    }
}

impl From<u64> for ToolValue {
    fn from(value: u64) -> Self {
        Self::Integer(value as i64)
    }
}

impl From<i64> for ToolValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<bool> for ToolValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    Conflict {
        tool: String,
        path: String,
        current_revision: Option<String>,
        message: String,
    },
    Denied {
        tool: String,
    },
    UnknownTool {
        name: String,
    },
    DuplicateTool {
        name: String,
    },
    MissingArgument {
        tool: String,
        argument: String,
    },
    InvalidArgument {
        tool: String,
        argument: String,
        message: String,
    },
    Execution {
        tool: String,
        message: String,
    },
}

impl ToolError {
    fn unknown_tool(name: impl Into<String>) -> Self {
        Self::UnknownTool { name: name.into() }
    }

    fn execution(tool: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Execution {
            tool: tool.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict {
                tool,
                path,
                message,
                ..
            } => write!(formatter, "tool {tool} conflict at {path}: {message}"),
            Self::Denied { tool } => write!(formatter, "tool {tool} denied by policy"),
            Self::UnknownTool { name } => write!(formatter, "unknown tool: {name}"),
            Self::DuplicateTool { name } => write!(formatter, "duplicate tool registered: {name}"),
            Self::MissingArgument { tool, argument } => {
                write!(
                    formatter,
                    "tool {tool} is missing required argument {argument}"
                )
            }
            Self::InvalidArgument {
                tool,
                argument,
                message,
            } => write!(
                formatter,
                "tool {tool} has invalid argument {argument}: {message}"
            ),
            Self::Execution { tool, message } => write!(formatter, "tool {tool} failed: {message}"),
        }
    }
}

impl std::error::Error for ToolError {}

pub struct AgentSession<'a> {
    next_call_id: u64,
    providers: Vec<Box<dyn ToolProvider + 'a>>,
    app: Option<&'a mut App>,
}

impl<'a> AgentSession<'a> {
    pub fn builder() -> AgentSessionBuilder<'a> {
        AgentSessionBuilder::default()
    }

    /// Create an agent session with Scope's built-in project tools.
    ///
    /// Downstream projects can use this as the default agentic surface and add
    /// project-specific providers with [`AgentSession::builder`] when they need
    /// richer semantic operations.
    pub fn for_app(app: &'a mut App) -> Result<Self, ToolError> {
        Self::builder()
            .app(app)
            .provider(ScopeToolProvider::for_session())
            .build()
    }

    pub fn tools(&self) -> Vec<ToolDefinition> {
        self.providers
            .iter()
            .flat_map(|provider| provider.tools().iter().cloned())
            .collect()
    }

    /// Snapshot the definitions and metadata of the actual installed providers.
    pub fn catalog(&self) -> SessionCatalog {
        SessionCatalog {
            tools: self
                .providers
                .iter()
                .flat_map(|provider| {
                    provider.tools().iter().map(|definition| ToolCatalogEntry {
                        definition: definition.clone(),
                        metadata: provider.metadata(&definition.name),
                        provider: provider.provider_name().map(str::to_owned),
                        output_description: provider
                            .output_description(&definition.name)
                            .map(str::to_owned),
                    })
                })
                .collect(),
        }
    }

    /// Backward-compatible invocation permitting every registered tool.
    pub fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        self.invoke_with_policy(call, |_| true)
    }

    pub fn invoke_with_policy(
        &mut self,
        call: ToolCall,
        policy: impl FnOnce(&ToolDefinition) -> bool,
    ) -> Result<ToolResult, ToolError> {
        self.invoke_with_call_policy(call, |definition, _| policy(definition))
    }

    /// Evaluate the tool definition and call arguments before dispatch.
    pub fn invoke_with_call_policy(
        &mut self,
        call: ToolCall,
        policy: impl FnOnce(&ToolDefinition, &ToolCall) -> bool,
    ) -> Result<ToolResult, ToolError> {
        self.invoke_observed_with_call_policy(call, policy, |_| {})
    }

    pub fn invoke_observed(
        &mut self,
        call: ToolCall,
        observer: impl FnMut(&ToolExecutionEvent),
    ) -> Result<ToolResult, ToolError> {
        self.invoke_observed_with_policy(call, |_| true, observer)
    }

    /// Policy runs before dispatch. All normal returns, including denial and unknown
    /// tools, emit Finished; provider/observer panics are not intercepted.
    pub fn invoke_observed_with_policy(
        &mut self,
        call: ToolCall,
        policy: impl FnOnce(&ToolDefinition) -> bool,
        observer: impl FnMut(&ToolExecutionEvent),
    ) -> Result<ToolResult, ToolError> {
        self.invoke_observed_with_call_policy(call, |definition, _| policy(definition), observer)
    }

    /// Policy inspects the definition and call before dispatch. All normal returns,
    /// including denial and unknown tools, emit Finished; panics are not intercepted.
    pub fn invoke_observed_with_call_policy(
        &mut self,
        call: ToolCall,
        policy: impl FnOnce(&ToolDefinition, &ToolCall) -> bool,
        mut observer: impl FnMut(&ToolExecutionEvent),
    ) -> Result<ToolResult, ToolError> {
        let call_id = self.next_call_id;
        self.next_call_id = call_id.checked_add(1).expect("tool call id exhausted");
        let name = call.name.clone();
        let started = Instant::now();
        observer(&ToolExecutionEvent::Started {
            call_id,
            name: name.clone(),
        });
        let result = if let Some(provider) = self
            .providers
            .iter_mut()
            .find(|provider| provider.tools().iter().any(|tool| tool.name == name))
        {
            let definition = provider
                .tools()
                .iter()
                .find(|tool| tool.name == name)
                .unwrap();
            if !policy(definition, &call) {
                Err(ToolError::Denied { tool: name.clone() })
            } else {
                match self.app.as_deref_mut() {
                    Some(app) => provider.invoke_with_app(call, app),
                    None => provider.invoke(call),
                }
            }
        } else {
            Err(ToolError::unknown_tool(name.clone()))
        };
        let outcome = match &result {
            Ok(_) => ToolExecutionOutcome::Success,
            Err(ToolError::UnknownTool { .. }) => ToolExecutionOutcome::UnknownTool,
            Err(ToolError::DuplicateTool { .. }) => ToolExecutionOutcome::DuplicateTool,
            Err(ToolError::MissingArgument { .. }) => ToolExecutionOutcome::MissingArgument,
            Err(ToolError::InvalidArgument { .. }) => ToolExecutionOutcome::InvalidArgument,
            Err(ToolError::Conflict { .. }) => ToolExecutionOutcome::Conflict,
            Err(ToolError::Denied { .. }) => ToolExecutionOutcome::Denied,
            Err(ToolError::Execution { .. }) => ToolExecutionOutcome::Execution,
        };
        observer(&ToolExecutionEvent::Finished {
            call_id,
            name,
            duration: started.elapsed(),
            outcome,
        });
        result
    }
}

#[derive(Default)]
pub struct AgentSessionBuilder<'a> {
    providers: Vec<Box<dyn ToolProvider + 'a>>,
    app: Option<&'a mut App>,
}

impl<'a> AgentSessionBuilder<'a> {
    /// Borrow one host app for all context-aware providers in this session.
    pub fn app(mut self, app: &'a mut App) -> Self {
        self.app = Some(app);
        self
    }

    pub fn provider(mut self, provider: impl ToolProvider + 'a) -> Self {
        self.providers.push(Box::new(provider));
        self
    }

    pub fn build(self) -> Result<AgentSession<'a>, ToolError> {
        let mut names = BTreeMap::new();
        for provider in &self.providers {
            for tool in provider.tools() {
                if names.insert(tool.name.clone(), ()).is_some() {
                    return Err(ToolError::DuplicateTool {
                        name: tool.name.clone(),
                    });
                }
            }
        }

        Ok(AgentSession {
            next_call_id: 1,
            providers: self.providers,
            app: self.app,
        })
    }
}

/// Discovery over explicitly installed Meta providers. This adapter only requests
/// existing snapshots; it does not load source, refresh providers, or grant authority.
mod meta_surface;
pub use meta_surface::{MetaSourceResolver, ResolvedMetaSource, WorkspaceMetaSourceResolver};

pub struct MetaDiscoveryProvider {
    resolvers: BTreeMap<String, Box<dyn MetaSourceResolver>>,
    invalidated: std::collections::BTreeSet<String>,
    providers: Vec<(
        scope_meta::ProviderDescription,
        Box<dyn scope_meta::MetaProvider>,
    )>,
    tools: Vec<ToolDefinition>,
}

/// Construction fails on the first repeated identity in installation order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateMetaProvider {
    pub id: String,
}

impl std::fmt::Display for DuplicateMetaProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "duplicate Meta provider identity: {}", self.id)
    }
}

impl std::error::Error for DuplicateMetaProvider {}

impl MetaDiscoveryProvider {
    pub fn new(
        providers: Vec<Box<dyn scope_meta::MetaProvider>>,
    ) -> Result<Self, DuplicateMetaProvider> {
        let mut identities = std::collections::BTreeSet::new();
        let mut installed = Vec::with_capacity(providers.len());
        for provider in providers {
            let description = provider.description();
            if !identities.insert(description.id.clone()) {
                return Err(DuplicateMetaProvider { id: description.id });
            }
            installed.push((description, provider));
        }
        Ok(Self {
            providers: installed,
            resolvers: BTreeMap::new(),
            invalidated: Default::default(),
            tools: meta_surface::definitions(vec![ToolDefinition::new(
                "scope.meta.list_providers",
                "List explicitly installed Meta providers and their available snapshot registry types; does not refresh or read source.",
            )]),
        })
    }
}

fn meta_type_value(kind: &scope_meta::TypeId) -> ToolValue {
    ToolValue::object([
        ("name", kind.name.clone().into()),
        ("version", kind.version.clone().into()),
    ])
}

impl ToolProvider for MetaDiscoveryProvider {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    fn provider_name(&self) -> Option<&str> {
        Some("scope.meta")
    }

    fn output_description(&self, tool_name: &str) -> Option<&str> {
        if matches!(
            tool_name,
            "scope.meta.find" | "scope.meta.inspect" | "scope.meta.related" | "scope.meta.source"
        ) {
            return Some(
                "Revision-aware read-only result with provider, graph, revision, completeness; paginated nodes/edges or UTF-8 source text. Source requires separately registered authority.",
            );
        }
        (tool_name == "scope.meta.list_providers").then_some(
            "Object with providers in installation order: id, description, node_types and relation_types (name/version), and snapshot state complete, incomplete with warnings (code/message/locator), or unavailable with error (kind/message). Types are null when unavailable; identities and completeness are not authority or freshness guarantees.",
        )
    }

    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        if call.name != "scope.meta.list_providers" {
            return self.invoke_meta(call);
        }
        reject_unexpected_arguments(&call, &[])?;
        let providers = self.providers.iter().map(|(description, provider)| {
            let snapshot = if self.invalidated.contains(&description.id) {
                Err(scope_meta::ProviderError::new(
                    scope_meta::ProviderErrorKind::Unavailable,
                    "host invalidated snapshot",
                ))
            } else {
                provider.snapshot()
            };
            let (nodes, relations, state) = match snapshot {
                Ok(snapshot) => {
                    let registry = snapshot.graph.registry();
                    let state = match snapshot.completeness {
                        scope_meta::SnapshotCompleteness::Complete => {
                            ToolValue::object([("state", "complete".into())])
                        }
                        scope_meta::SnapshotCompleteness::Incomplete { warnings } => {
                            ToolValue::object([
                                ("state", "incomplete".into()),
                                (
                                    "warnings",
                                    ToolValue::array(warnings.into_iter().map(|warning| {
                                        ToolValue::object([
                                            ("code", warning.code.into()),
                                            ("message", warning.message.into()),
                                            (
                                                "locator",
                                                warning
                                                    .locator
                                                    .map(ToolValue::from)
                                                    .unwrap_or(ToolValue::Null),
                                            ),
                                        ])
                                    })),
                                ),
                            ])
                        }
                    };
                    (
                        ToolValue::array(registry.node_types().map(meta_type_value)),
                        ToolValue::array(
                            registry
                                .relation_types()
                                .map(|relation| meta_type_value(&relation.relation)),
                        ),
                        state,
                    )
                }
                Err(error) => {
                    let kind = match error.kind {
                        scope_meta::ProviderErrorKind::Unavailable => "unavailable",
                        scope_meta::ProviderErrorKind::InvalidSnapshot => "invalid_snapshot",
                        scope_meta::ProviderErrorKind::Other => "other",
                    };
                    (
                        ToolValue::Null,
                        ToolValue::Null,
                        ToolValue::object([
                            ("state", "unavailable".into()),
                            (
                                "error",
                                ToolValue::object([
                                    ("kind", kind.into()),
                                    ("message", error.message.into()),
                                ]),
                            ),
                        ]),
                    )
                }
            };
            ToolValue::object([
                ("id", description.id.clone().into()),
                ("description", description.description.clone().into()),
                ("node_types", nodes),
                ("relation_types", relations),
                ("snapshot", state),
            ])
        });
        Ok(ToolResult::new(ToolValue::object([(
            "providers",
            ToolValue::array(providers),
        )])))
    }
}

/// Isolated proposal tools only; never exposes baseline writes or acceptance.
pub struct ProposalToolProvider<'a> {
    proposal: &'a mut crate::workspace::Proposal,
    tools: Vec<ToolDefinition>,
}

impl<'a> ProposalToolProvider<'a> {
    pub fn new(proposal: &'a mut crate::workspace::Proposal) -> Self {
        let path = || {
            ToolInputDefinition::required(
                "path",
                ToolInputKind::String,
                "Exact approved relative file path",
            )
        };
        Self {
            proposal,
            tools: vec![
                ToolDefinition::new("scope.workspace.list", "List approved proposal paths"),
                ToolDefinition::new(
                    "scope.document.read",
                    "Read isolated text and proposal revision",
                )
                .input(path()),
                ToolDefinition::new(
                    "scope.document.create",
                    "Create UTF-8 text in isolation; refuses existing files",
                )
                .effect(ToolEffect::Write)
                .input(path())
                .input(ToolInputDefinition::required(
                    "text",
                    ToolInputKind::String,
                    "New file text",
                )),
                ToolDefinition::new(
                    "scope.document.replace",
                    "Replace a checked half-open range in isolated text",
                )
                .effect(ToolEffect::Write)
                .input(path())
                .input(ToolInputDefinition::required(
                    "expected_revision",
                    ToolInputKind::Integer,
                    "Proposal revision from read",
                ))
                .input(ToolInputDefinition::required(
                    "start_line",
                    ToolInputKind::Integer,
                    "Zero-based line",
                ))
                .input(ToolInputDefinition::required(
                    "start_column",
                    ToolInputKind::Integer,
                    "Zero-based UTF-8 byte column",
                ))
                .input(ToolInputDefinition::required(
                    "end_line",
                    ToolInputKind::Integer,
                    "Zero-based line",
                ))
                .input(ToolInputDefinition::required(
                    "end_column",
                    ToolInputKind::Integer,
                    "Zero-based UTF-8 byte column (exclusive)",
                ))
                .input(ToolInputDefinition::required(
                    "replacement",
                    ToolInputKind::String,
                    "Replacement text",
                )),
            ],
        }
    }
}

impl ToolProvider for ProposalToolProvider<'_> {
    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
    fn provider_name(&self) -> Option<&str> {
        Some("scope.proposal")
    }
    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let error = |message: String| ToolError::execution(call.name.clone(), message);
        if call.name == "scope.workspace.list" {
            reject_unexpected_arguments(&call, &[])?;
            return Ok(ToolResult::new(ToolValue::array(
                self.proposal
                    .paths()
                    .into_iter()
                    .map(|path| path.to_string().into()),
            )));
        }
        if !matches!(
            call.name.as_str(),
            "scope.document.read" | "scope.document.create" | "scope.document.replace"
        ) {
            return Err(ToolError::unknown_tool(call.name));
        }
        let path = crate::path::ScopePath::new(string_argument(&call, "path")?)
            .map_err(|err| error(err.to_string()))?;
        if call.name == "scope.document.read" {
            reject_unexpected_arguments(&call, &["path"])?;
            let (text, revision) = self
                .proposal
                .read(&path)
                .map_err(|err| error(err.to_string()))?;
            return Ok(ToolResult::new(ToolValue::object([
                ("text", text.into()),
                ("revision", revision.into()),
            ])));
        }
        let revision = if call.name == "scope.document.create" {
            reject_unexpected_arguments(&call, &["path", "text"])?;
            self.proposal
                .create(&path, string_argument(&call, "text")?.to_owned())
        } else {
            reject_unexpected_arguments(
                &call,
                &[
                    "path",
                    "expected_revision",
                    "start_line",
                    "start_column",
                    "end_line",
                    "end_column",
                    "replacement",
                ],
            )?;
            let revision = usize_argument(&call, "expected_revision")? as u64;
            let (text, current) = self
                .proposal
                .read(&path)
                .map_err(|err| error(err.to_string()))?;
            if revision != current {
                return Err(error("proposal revision conflict".into()));
            }
            let mut document = Document::from_workspace_file(
                DocumentId::new(0),
                crate::workspace::WorkspaceFile {
                    path: path.clone(),
                    contents: text.to_owned(),
                },
            );
            document
                .apply_edit(TextEdit::replace(
                    TextRange::new(
                        Position::new(
                            usize_argument(&call, "start_line")?,
                            usize_argument(&call, "start_column")?,
                        ),
                        Position::new(
                            usize_argument(&call, "end_line")?,
                            usize_argument(&call, "end_column")?,
                        ),
                    ),
                    string_argument(&call, "replacement")?.to_owned(),
                ))
                .map_err(|err| error(err.to_string()))?;
            self.proposal
                .replace(&path, document.text().to_owned(), revision)
        }
        .map_err(|err| error(err.to_string()))?;
        Ok(ToolResult::new(ToolValue::object([(
            "revision",
            revision.into(),
        )])))
    }
}

pub struct ScopeToolProvider<'a> {
    app: &'a mut App,
    tools: Vec<ToolDefinition>,
}

struct SessionScopeToolProvider {
    tools: Vec<ToolDefinition>,
}

impl ToolProvider for SessionScopeToolProvider {
    fn provider_name(&self) -> Option<&str> {
        Some("scope")
    }

    fn output_description(&self, tool_name: &str) -> Option<&str> {
        scope_output_description(tool_name)
    }

    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    fn invoke_with_app(&mut self, call: ToolCall, app: &mut App) -> Result<ToolResult, ToolError> {
        ScopeToolProvider::new(app).invoke(call)
    }
}

impl<'a> ScopeToolProvider<'a> {
    /// Built-in tools using the app supplied by `AgentSessionBuilder::app`.
    pub fn for_session() -> impl ToolProvider {
        SessionScopeToolProvider {
            tools: scope_tool_definitions(),
        }
    }

    pub fn new(app: &'a mut App) -> Self {
        Self {
            app,
            tools: scope_tool_definitions(),
        }
    }
}

impl ToolProvider for ScopeToolProvider<'_> {
    fn provider_name(&self) -> Option<&str> {
        Some("scope")
    }

    fn output_description(&self, tool_name: &str) -> Option<&str> {
        scope_output_description(tool_name)
    }

    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.name.as_str() {
            "scope.workspace.list" => self.list_workspace(call),
            "scope.document.read" => self.read_document(call),
            "scope.document.replace" => self.replace_document(call),
            "scope.language.symbols" => self.language_symbols(call),
            "scope.language.diagnostics" => self.language_diagnostics(call),
            _ => Err(ToolError::unknown_tool(call.name)),
        }
    }
}

impl ScopeToolProvider<'_> {
    // Analyze a detached snapshot so observation cannot replace the user's buffer.
    fn document_for_query(&self, call: &ToolCall) -> Result<Document, ToolError> {
        let path = string_argument(call, "path")?;
        reject_unexpected_arguments(call, &["path"])?;
        let file = self
            .app
            .workspace()
            .load_file(path)
            .map_err(|error| ToolError::execution(call.name.clone(), error.to_string()))?;
        Ok(Document::from_workspace_file(DocumentId::new(0), file))
    }

    fn list_workspace(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        reject_unexpected_arguments(&call, &[])?;
        let files = self
            .app
            .workspace()
            .list_files()
            .map_err(|error| ToolError::execution(call.name.clone(), error.to_string()))?
            .into_iter()
            .map(|entry| {
                ToolValue::object([
                    ("path", ToolValue::from(entry.path.to_string())),
                    ("kind", ToolValue::from("file")),
                ])
            });

        Ok(ToolResult::new(ToolValue::object([(
            "files",
            ToolValue::array(files),
        )])))
    }

    fn read_document(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let path = string_argument(&call, "path")?;
        reject_unexpected_arguments(&call, &["path"])?;
        if let Some(document) = self.app.open_document()
            && document.path().to_string() == path
        {
            return Ok(ToolResult::new(ToolValue::object([
                ("path", ToolValue::from(document.path().to_string())),
                ("text", ToolValue::from(document.text().to_owned())),
                (
                    "revision",
                    document
                        .disk_revision()
                        .map(ToolValue::from)
                        .unwrap_or(ToolValue::Null),
                ),
                ("document_id", document.id().as_u64().into()),
                ("version", document.version().into()),
            ])));
        }
        let file = self
            .app
            .workspace()
            .load_file_revision(path)
            .map_err(|error| ToolError::execution(call.name.clone(), error.to_string()))?;

        Ok(ToolResult::new(ToolValue::object([
            ("path", ToolValue::from(file.path.to_string())),
            ("text", ToolValue::from(file.contents)),
            ("revision", file.revision.into()),
        ])))
    }

    fn replace_document(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let path = string_argument(&call, "path")?;
        let expected_revision = string_argument(&call, "expected_revision")?;
        let snapshot = match (
            call.arguments.contains_key("expected_document_id"),
            call.arguments.contains_key("expected_version"),
        ) {
            (false, false) => None,
            (true, true) => Some((
                usize_argument(&call, "expected_document_id")? as u64,
                usize_argument(&call, "expected_version")? as u64,
            )),
            _ => {
                return Err(ToolError::InvalidArgument {
                    tool: call.name.clone(),
                    argument: "expected_document_id/expected_version".into(),
                    message: "snapshot arguments must be supplied together".into(),
                });
            }
        };
        let start_line = usize_argument(&call, "start_line")?;
        let start_column = usize_argument(&call, "start_column")?;
        let end_line = usize_argument(&call, "end_line")?;
        let end_column = usize_argument(&call, "end_column")?;
        let replacement = string_argument(&call, "replacement")?.to_owned();
        let save = optional_bool_argument(&call, "save")?.unwrap_or(true);
        reject_unexpected_arguments(
            &call,
            &[
                "path",
                "start_line",
                "start_column",
                "end_line",
                "end_column",
                "replacement",
                "save",
                "expected_revision",
                "expected_document_id",
                "expected_version",
            ],
        )?;

        let active = self
            .app
            .open_document()
            .filter(|document| document.path().to_string() == path);
        let conflict = |revision: Option<&str>, message: &str| ToolError::Conflict {
            tool: call.name.clone(),
            path: path.to_owned(),
            current_revision: revision.map(str::to_owned),
            message: message.to_owned(),
        };
        let mut candidate = if let Some(document) = active {
            if document.disk_revision() != Some(expected_revision) {
                return Err(conflict(
                    document.disk_revision(),
                    "buffer baseline revision changed",
                ));
            }
            if let Some((id, version)) = snapshot {
                if document.id().as_u64() != id || document.version() != version {
                    return Err(conflict(
                        document.disk_revision(),
                        "active buffer identity or version changed",
                    ));
                }
            } else {
                return Err(ToolError::MissingArgument {
                    tool: call.name.clone(),
                    argument: "expected_document_id/expected_version".into(),
                });
            }
            document.clone()
        } else {
            if snapshot.is_some() || self.app.open_document().is_some_and(Document::is_dirty) {
                return Err(conflict(
                    None,
                    "cannot edit another path with a snapshot or dirty active buffer",
                ));
            }
            if !save {
                return Err(ToolError::InvalidArgument {
                    tool: call.name.clone(),
                    argument: "save".into(),
                    message: "save=false requires the matching active document".into(),
                });
            }
            let file = self
                .app
                .workspace()
                .load_file_revision(path)
                .map_err(|error| workspace_tool_error(&call, error))?;
            if file.revision != expected_revision {
                return Err(conflict(Some(&file.revision), "disk revision changed"));
            }
            Document::from_revisioned_workspace_file(DocumentId::new(0), file)
        };
        // Check disk even for unsaved edits; never commit a staged edit on failure.
        let disk = self
            .app
            .workspace()
            .load_file_revision(path)
            .map_err(|error| workspace_tool_error(&call, error))?;
        if disk.revision != expected_revision {
            return Err(conflict(Some(&disk.revision), "disk revision changed"));
        }
        candidate
            .apply_edit(TextEdit::replace(
                TextRange::new(
                    Position::new(start_line, start_column),
                    Position::new(end_line, end_column),
                ),
                replacement,
            ))
            .map_err(|error| ToolError::execution(call.name.clone(), error.to_string()))?;
        if save {
            let revision = self
                .app
                .workspace()
                .save_file_checked(path, candidate.text(), expected_revision)
                .map_err(|error| workspace_tool_error(&call, error))?;
            candidate.mark_saved_revision(revision);
        }
        let result = ToolResult::new(ToolValue::object([
            ("path", candidate.path().to_string().into()),
            (
                "revision",
                candidate
                    .disk_revision()
                    .map(ToolValue::from)
                    .unwrap_or(ToolValue::Null),
            ),
            (
                "document_id",
                if active.is_some() {
                    candidate.id().as_u64().into()
                } else {
                    ToolValue::Null
                },
            ),
            ("version", candidate.version().into()),
            ("saved", (!candidate.is_dirty()).into()),
            ("text", candidate.text().into()),
        ]));
        if active.is_some() {
            *self.app.open_document_mut().unwrap() = candidate;
        }
        Ok(result)
    }

    fn language_symbols(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let document = self.document_for_query(&call)?;
        let symbols = self
            .app
            .semantic_service_for_document(&document)
            .outline(document.as_language_document())
            .into_iter()
            .map(|symbol| {
                ToolValue::object([
                    ("name", ToolValue::from(symbol.name)),
                    ("kind", ToolValue::from(symbol.kind.to_string())),
                    ("range", range_value(symbol.range)),
                ])
            });

        Ok(ToolResult::new(ToolValue::object([(
            "symbols",
            ToolValue::array(symbols),
        )])))
    }

    fn language_diagnostics(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let document = self.document_for_query(&call)?;
        let diagnostics = self
            .app
            .semantic_service_for_document(&document)
            .diagnostics(document.as_language_document())
            .into_iter()
            .map(|diagnostic| {
                ToolValue::object([
                    ("message", ToolValue::from(diagnostic.message)),
                    ("severity", ToolValue::from(diagnostic.severity.to_string())),
                    ("range", range_value(diagnostic.range)),
                    (
                        "source",
                        diagnostic
                            .source
                            .map(ToolValue::from)
                            .unwrap_or(ToolValue::Null),
                    ),
                    (
                        "code",
                        diagnostic
                            .code
                            .map(ToolValue::from)
                            .unwrap_or(ToolValue::Null),
                    ),
                ])
            });

        Ok(ToolResult::new(ToolValue::object([(
            "diagnostics",
            ToolValue::array(diagnostics),
        )])))
    }
}

fn scope_output_description(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "scope.workspace.list" => Some(
            "Object with files: array of objects containing path (string) and kind (string, always file).",
        ),
        "scope.document.read" => Some(
            "Object with path and text (strings), and revision (string or null). Reading the active buffer also includes document_id and version (integers) and returns unsaved text; other reads return disk text without those fields.",
        ),
        "scope.document.replace" => Some(
            "Object with path and text (strings), revision (string or null), document_id (integer for an active buffer, otherwise null), version (integer), and saved (boolean).",
        ),
        "scope.language.symbols" => Some(
            "Object with symbols: array of objects containing name and kind (strings) and range (object with start_line, start_column, end_line, end_column: zero-based integers, columns in UTF-8 bytes). Analyzes workspace text, not the unsaved buffer.",
        ),
        "scope.language.diagnostics" => Some(
            "Object with diagnostics: array of objects containing message and severity (strings), source and code (strings or null), and range (object with start_line, start_column, end_line, end_column: zero-based integers, columns in UTF-8 bytes). Analyzes workspace text, not the unsaved buffer.",
        ),
        _ => None,
    }
}

fn scope_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "scope.workspace.list",
            "List files visible in the current Scope workspace.",
        ),
        ToolDefinition::new(
            "scope.document.read",
            "Read a UTF-8 document from the Scope workspace.",
        )
        .input(ToolInputDefinition::required(
            "path",
            ToolInputKind::String,
            "Workspace-relative Scope path to read.",
        )),
        ToolDefinition::new(
            "scope.document.replace",
            "Replace a range with revision checks. Active buffers require paired identity/version; nonactive edits save without opening. save=false requires an active buffer.",
        )
        .input(ToolInputDefinition::required(
            "path",
            ToolInputKind::String,
            "Workspace-relative Scope path to edit.",
        ))
        .effect(ToolEffect::Write)
        .input(ToolInputDefinition::required("expected_revision", ToolInputKind::String, "Revision returned by read."))
        .input(ToolInputDefinition::optional("expected_document_id", ToolInputKind::Integer, "Active document identity; required with expected_version for active edits."))
        .input(ToolInputDefinition::optional("expected_version", ToolInputKind::Integer, "Active buffer version paired with expected_document_id."))
        .input(ToolInputDefinition::required(
            "start_line",
            ToolInputKind::Integer,
            "Zero-based start line.",
        ))
        .input(ToolInputDefinition::required(
            "start_column",
            ToolInputKind::Integer,
            "Zero-based UTF-8 byte column at the start line.",
        ))
        .input(ToolInputDefinition::required(
            "end_line",
            ToolInputKind::Integer,
            "Zero-based end line.",
        ))
        .input(ToolInputDefinition::required(
            "end_column",
            ToolInputKind::Integer,
            "Zero-based UTF-8 byte column at the end line.",
        ))
        .input(ToolInputDefinition::required(
            "replacement",
            ToolInputKind::String,
            "Replacement text.",
        ))
        .input(ToolInputDefinition::optional(
            "save",
            ToolInputKind::Boolean,
            "Whether to save the edited document back to the workspace. Defaults to true.",
        )),
        ToolDefinition::new(
            "scope.language.symbols",
            "Return language symbols for a workspace document.",
        )
        .input(ToolInputDefinition::required(
            "path",
            ToolInputKind::String,
            "Workspace-relative Scope path to analyze.",
        )),
        ToolDefinition::new(
            "scope.language.diagnostics",
            "Return diagnostics for a workspace document.",
        )
        .input(ToolInputDefinition::required(
            "path",
            ToolInputKind::String,
            "Workspace-relative Scope path to analyze.",
        )),
    ]
}

fn workspace_tool_error(call: &ToolCall, error: WorkspaceError) -> ToolError {
    match error {
        WorkspaceError::Conflict {
            path,
            current_revision,
        } => ToolError::Conflict {
            tool: call.name.clone(),
            path: path.to_string(),
            current_revision: Some(current_revision),
            message: "disk revision changed".into(),
        },
        error => ToolError::execution(call.name.clone(), error.to_string()),
    }
}

fn string_argument<'a>(call: &'a ToolCall, name: &str) -> Result<&'a str, ToolError> {
    required_argument(call, name)?
        .as_str()
        .ok_or_else(|| ToolError::InvalidArgument {
            tool: call.name.clone(),
            argument: name.to_owned(),
            message: "expected string".to_owned(),
        })
}

fn usize_argument(call: &ToolCall, name: &str) -> Result<usize, ToolError> {
    let value =
        required_argument(call, name)?
            .as_i64()
            .ok_or_else(|| ToolError::InvalidArgument {
                tool: call.name.clone(),
                argument: name.to_owned(),
                message: "expected integer".to_owned(),
            })?;

    usize::try_from(value).map_err(|_| ToolError::InvalidArgument {
        tool: call.name.clone(),
        argument: name.to_owned(),
        message: "expected non-negative integer".to_owned(),
    })
}

fn optional_bool_argument(call: &ToolCall, name: &str) -> Result<Option<bool>, ToolError> {
    call.arguments
        .get(name)
        .map(|value| {
            value.as_bool().ok_or_else(|| ToolError::InvalidArgument {
                tool: call.name.clone(),
                argument: name.to_owned(),
                message: "expected boolean".to_owned(),
            })
        })
        .transpose()
}

fn required_argument<'a>(call: &'a ToolCall, name: &str) -> Result<&'a ToolValue, ToolError> {
    call.arguments
        .get(name)
        .ok_or_else(|| ToolError::MissingArgument {
            tool: call.name.clone(),
            argument: name.to_owned(),
        })
}

fn reject_unexpected_arguments(call: &ToolCall, allowed: &[&str]) -> Result<(), ToolError> {
    for argument in call.arguments.keys() {
        if !allowed.contains(&argument.as_str()) {
            return Err(ToolError::InvalidArgument {
                tool: call.name.clone(),
                argument: argument.clone(),
                message: "unexpected argument".to_owned(),
            });
        }
    }

    Ok(())
}

fn range_value(range: TextRange) -> ToolValue {
    ToolValue::object([
        ("start_line", ToolValue::from(range.start.line)),
        ("start_column", ToolValue::from(range.start.column)),
        ("end_line", ToolValue::from(range.end.line)),
        ("end_column", ToolValue::from(range.end.column)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempWorkspace;

    struct TestMetaProvider {
        id: &'static str,
        graph: scope_meta::MetaGraph,
        state: Option<scope_meta::SnapshotCompleteness>,
        calls: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl scope_meta::MetaProvider for TestMetaProvider {
        fn description(&self) -> scope_meta::ProviderDescription {
            scope_meta::ProviderDescription::new(self.id, "test metadata")
        }

        fn snapshot(&self) -> Result<scope_meta::MetaSnapshot<'_>, scope_meta::ProviderError> {
            self.calls.set(self.calls.get() + 1);
            match &self.state {
                Some(state) => Ok(scope_meta::MetaSnapshot::new(&self.graph, state.clone())),
                None => Err(scope_meta::ProviderError::new(
                    scope_meta::ProviderErrorKind::Unavailable,
                    "not ready",
                )),
            }
        }
    }

    fn meta_fixture(
        id: &'static str,
        state: Option<scope_meta::SnapshotCompleteness>,
        calls: &std::rc::Rc<std::cell::Cell<usize>>,
    ) -> Box<dyn scope_meta::MetaProvider> {
        use scope_meta::*;
        let kind = TypeId::new("test.item", "1");
        let mut registry = TypeRegistry::new();
        registry.register_node(kind.clone()).unwrap();
        registry
            .register_relation(TypeId::new("test.link", "2"), kind.clone(), kind.clone())
            .unwrap();
        Box::new(TestMetaProvider {
            id,
            graph: MetaGraph::new(
                GraphId::new("private"),
                RevisionId::new("private"),
                registry,
                vec![MetaNode {
                    id: "secret".into(),
                    kind,
                    label: "secret source".into(),
                    source: None,
                }],
                vec![],
            )
            .unwrap(),
            state,
            calls: calls.clone(),
        })
    }

    #[test]
    fn meta_discovery_catalog_dispatch_and_states() {
        use scope_meta::*;
        fn field<'a>(value: &'a ToolValue, key: &str) -> &'a ToolValue {
            super::tests::field(value, key).expect("discovery field")
        }
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let provider = MetaDiscoveryProvider::new(vec![
            meta_fixture("complete", Some(SnapshotCompleteness::Complete), &calls),
            meta_fixture(
                "partial",
                Some(SnapshotCompleteness::Incomplete {
                    warnings: vec![
                        SnapshotWarning::new("partial", "missing data").with_locator("opaque"),
                    ],
                }),
                &calls,
            ),
            meta_fixture("offline", None, &calls),
        ])
        .unwrap();
        let mut session = AgentSession::builder().provider(provider).build().unwrap();
        let catalog = session.catalog();
        assert_eq!(catalog.tools.len(), 5);
        let entry = catalog.lookup("scope.meta.list_providers").unwrap();
        assert_eq!(entry.provider.as_deref(), Some("scope.meta"));
        assert_eq!(entry.definition.effect, ToolEffect::Read);
        assert!(entry.definition.inputs.is_empty());
        assert!(entry.output_description.is_some());
        assert_eq!(calls.get(), 0);
        assert!(matches!(
            session.invoke_with_policy(ToolCall::new("scope.meta.list_providers"), |_| false),
            Err(ToolError::Denied { .. })
        ));
        assert!(matches!(
            session.invoke(
                ToolCall::new("scope.meta.list_providers").with_argument("refresh", true.into())
            ),
            Err(ToolError::InvalidArgument { .. })
        ));
        assert!(matches!(
            session.invoke(ToolCall::new("scope.meta.find")),
            Err(ToolError::MissingArgument { .. })
        ));
        assert_eq!(calls.get(), 0);
        let result = session
            .invoke(ToolCall::new("scope.meta.list_providers"))
            .unwrap();
        assert_eq!(calls.get(), 3);
        let ToolValue::Array(providers) = field(&result.value, "providers") else {
            panic!("providers array")
        };
        assert_eq!(providers.len(), 3);
        assert_eq!(field(&providers[0], "id"), &ToolValue::from("complete"));
        assert_eq!(
            field(&providers[0], "description"),
            &ToolValue::from("test metadata")
        );
        assert_eq!(
            field(&providers[0], "node_types"),
            &ToolValue::array([meta_type_value(&TypeId::new("test.item", "1"))])
        );
        assert_eq!(
            field(&providers[0], "relation_types"),
            &ToolValue::array([meta_type_value(&TypeId::new("test.link", "2"))])
        );
        assert_eq!(
            field(&providers[0], "snapshot"),
            &ToolValue::object([("state", "complete".into())])
        );
        assert_eq!(
            field(&providers[1], "snapshot"),
            &ToolValue::object([
                ("state", "incomplete".into()),
                (
                    "warnings",
                    ToolValue::array([ToolValue::object([
                        ("code", "partial".into()),
                        ("message", "missing data".into()),
                        ("locator", "opaque".into()),
                    ])])
                ),
            ])
        );
        assert_eq!(field(&providers[2], "node_types"), &ToolValue::Null);
        assert_eq!(field(&providers[2], "relation_types"), &ToolValue::Null);
        assert_eq!(
            field(&providers[2], "snapshot"),
            &ToolValue::object([
                ("state", "unavailable".into()),
                (
                    "error",
                    ToolValue::object([
                        ("kind", "unavailable".into()),
                        ("message", "not ready".into())
                    ])
                ),
            ])
        );
        assert!(!format!("{:?}", result.value).contains("secret"));
        assert!(!format!("{:?}", result.value).contains("private"));
    }

    #[test]
    fn meta_discovery_empty_and_duplicate_identities() {
        let mut session = AgentSession::builder()
            .provider(MetaDiscoveryProvider::new(vec![]).unwrap())
            .build()
            .unwrap();
        assert_eq!(
            session
                .invoke(ToolCall::new("scope.meta.list_providers"))
                .unwrap()
                .value,
            ToolValue::object([("providers", ToolValue::array([]))])
        );
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let result = MetaDiscoveryProvider::new(vec![
            meta_fixture("same", None, &calls),
            meta_fixture("same", None, &calls),
        ]);
        assert!(matches!(result, Err(DuplicateMetaProvider { id }) if id == "same"));
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn empty_catalog() {
        let session = AgentSession::builder().build().unwrap();
        let catalog = session.catalog();
        assert_eq!(catalog, SessionCatalog::default());
        assert_eq!(catalog.lookup("missing"), None);
        assert_eq!(
            catalog.to_value(),
            ToolValue::object([("tools", ToolValue::array([]))])
        );
    }

    #[test]
    fn custom_catalog_preserves_definitions_and_exports_all_input_kinds() {
        struct Custom(Vec<ToolDefinition>);
        impl ToolProvider for Custom {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }
            fn provider_name(&self) -> Option<&str> {
                Some("custom")
            }
            fn output_description(&self, name: &str) -> Option<&str> {
                (name == "custom.edit").then_some("An object.")
            }
        }
        let kinds = [
            ToolInputKind::String,
            ToolInputKind::Integer,
            ToolInputKind::Boolean,
            ToolInputKind::Object,
            ToolInputKind::Array,
        ];
        let mut definition =
            ToolDefinition::new("custom.edit", "Custom edit").effect(ToolEffect::Write);
        for (index, kind) in kinds.into_iter().enumerate() {
            definition.inputs.push(ToolInputDefinition {
                name: format!("input{index}"),
                kind,
                description: format!("description{index}"),
                required: index % 2 == 0,
            });
        }
        let session = AgentSession::builder()
            .provider(Custom(vec![definition.clone()]))
            .provider(FakeGlyphToolProvider::default())
            .build()
            .unwrap();
        let catalog = session.catalog();
        assert_eq!(
            catalog
                .tools
                .iter()
                .map(|entry| entry.definition.clone())
                .collect::<Vec<_>>(),
            session.tools()
        );
        assert_eq!(
            catalog.lookup("custom.edit"),
            Some(&ToolCatalogEntry {
                definition: definition.clone(),
                provider: Some("custom".into()),
                output_description: Some("An object.".into()),
                metadata: None,
            })
        );
        assert_eq!(catalog.lookup("missing"), None);
        let legacy = catalog.lookup("glyph.axiom.list").unwrap();
        assert_eq!(legacy.provider, None);
        assert_eq!(legacy.output_description, None);
        let exported = catalog.to_value();
        let Some(ToolValue::Array(entries)) = field(&exported, "tools") else {
            panic!("expected tools array")
        };
        assert_eq!(entries.len(), 2);
        let expected_inputs = ["string", "integer", "boolean", "object", "array"]
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                ToolValue::object([
                    ("name", format!("input{index}").into()),
                    ("kind", kind.into()),
                    ("description", format!("description{index}").into()),
                    ("required", (index % 2 == 0).into()),
                ])
            });
        assert_eq!(
            entries[0],
            ToolValue::object([
                (
                    "definition",
                    ToolValue::object([
                        ("name", definition.name.into()),
                        ("description", definition.description.into()),
                        ("effect", "write".into()),
                        ("inputs", ToolValue::array(expected_inputs)),
                    ])
                ),
                ("provider", "custom".into()),
                ("output_description", "An object.".into()),
                ("metadata", ToolValue::Null),
            ])
        );
        assert_eq!(field(&entries[1], "provider"), Some(&ToolValue::Null));
        assert_eq!(
            field(&entries[1], "output_description"),
            Some(&ToolValue::Null)
        );
        assert_eq!(
            field(field(&entries[1], "definition").unwrap(), "effect"),
            Some(&ToolValue::from("read"))
        );
        drop(session);
        assert!(catalog.lookup("custom.edit").is_some());
    }

    #[test]
    fn builtin_catalog_matches_both_scope_provider_variants() {
        let root = TempWorkspace::new("builtin_catalog");
        let mut app = App::new(root.workspace()).unwrap();
        let catalog = {
            let session = AgentSession::for_app(&mut app).unwrap();
            let catalog = session.catalog();
            assert_eq!(
                catalog
                    .tools
                    .iter()
                    .map(|entry| entry.definition.clone())
                    .collect::<Vec<_>>(),
                session.tools()
            );
            catalog
        };
        let direct = AgentSession::builder()
            .provider(ScopeToolProvider::new(&mut app))
            .build()
            .unwrap();
        assert_eq!(catalog, direct.catalog());
        assert_eq!(catalog.tools.len(), 5);
        for entry in &catalog.tools {
            assert_eq!(entry.provider.as_deref(), Some("scope"));
            assert!(
                entry
                    .output_description
                    .as_deref()
                    .is_some_and(|description| !description.is_empty())
            );
            assert_eq!(catalog.lookup(&entry.definition.name), Some(entry));
        }
        assert_eq!(
            catalog
                .lookup("scope.document.replace")
                .unwrap()
                .definition
                .effect,
            ToolEffect::Write
        );
        assert_eq!(
            ScopeToolProvider::for_session().output_description("unknown"),
            None
        );
    }

    #[test]
    fn session_providers_share_active_unsaved_document() {
        struct PathlangTools(Vec<ToolDefinition>);
        impl ToolProvider for PathlangTools {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }

            fn invoke_with_app(
                &mut self,
                _call: ToolCall,
                app: &mut App,
            ) -> Result<ToolResult, ToolError> {
                let document = app.open_document().unwrap();
                Ok(ToolResult::new(ToolValue::object([
                    ("path", document.path().to_string().into()),
                    ("text", document.text().into()),
                ])))
            }
        }

        let root = TempWorkspace::new("session_providers_share_active_unsaved_document");
        root.write_file("main.scope", "fn old() {}\n");
        root.write_file("other.scope", "fn other() {}\n");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("main.scope").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(TextEdit::replace(
                TextRange::new(Position::new(0, 3), Position::new(0, 6)),
                "unsaved",
            ))
            .unwrap();
        let before = app.open_document().unwrap().clone();
        {
            let mut session = AgentSession::builder()
                .app(&mut app)
                .provider(ScopeToolProvider::for_session())
                .provider(PathlangTools(vec![ToolDefinition::new(
                    "pathlang.goals",
                    "Inspect goals",
                )]))
                .provider(FakeGlyphToolProvider::default())
                .build()
                .unwrap();
            let scope = session
                .invoke(
                    ToolCall::new("scope.document.read").with_argument("path", "main.scope".into()),
                )
                .unwrap();
            let goals = session.invoke(ToolCall::new("pathlang.goals")).unwrap();
            assert_eq!(field(&scope.value, "text"), field(&goals.value, "text"));
            assert_eq!(
                field(&scope.value, "text"),
                Some(&"fn unsaved() {}\n".into())
            );
            session.invoke(ToolCall::new("glyph.axiom.list")).unwrap();
            let other = session
                .invoke(
                    ToolCall::new("scope.document.read")
                        .with_argument("path", "other.scope".into()),
                )
                .unwrap();
            assert_eq!(field(&other.value, "text"), Some(&"fn other() {}\n".into()));
            assert!(matches!(
                session.invoke(ToolCall::new("unknown")),
                Err(ToolError::UnknownTool { .. })
            ));
        }
        assert_eq!(app.open_document(), Some(&before));
        assert_eq!(root.read_file("main.scope"), "fn old() {}\n");
    }

    #[test]
    fn creates_default_scope_session_for_app() {
        let root = TempWorkspace::new("creates_default_scope_session_for_app");
        root.write_file("main.scope", "fn main() {}\n");
        let mut app = App::new(root.workspace()).unwrap();

        let session = AgentSession::for_app(&mut app).unwrap();
        let tool_names = session
            .tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();

        assert!(tool_names.contains(&"scope.workspace.list".to_owned()));
        assert!(tool_names.contains(&"scope.document.read".to_owned()));
        assert!(tool_names.contains(&"scope.document.replace".to_owned()));
    }

    #[test]
    fn session_discovers_scope_and_domain_tools() {
        let root = TempWorkspace::new("session_discovers_scope_and_domain_tools");
        root.write_file("main.scope", "fn main() {}\n");
        let mut app = App::new(root.workspace()).unwrap();

        let session = AgentSession::builder()
            .provider(ScopeToolProvider::new(&mut app))
            .provider(FakeGlyphToolProvider::default())
            .build()
            .unwrap();
        let tool_names = session
            .tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();

        assert!(tool_names.contains(&"scope.workspace.list".to_owned()));
        assert!(tool_names.contains(&"scope.document.read".to_owned()));
        assert!(tool_names.contains(&"glyph.axiom.list".to_owned()));
    }

    #[test]
    fn session_routes_calls_to_scope_and_domain_providers() {
        let root = TempWorkspace::new("session_routes_calls_to_scope_and_domain_providers");
        root.write_file("main.scope", "fn main() {}\n");
        let mut app = App::new(root.workspace()).unwrap();

        let mut session = AgentSession::builder()
            .provider(ScopeToolProvider::new(&mut app))
            .provider(FakeGlyphToolProvider::default())
            .build()
            .unwrap();

        let scope_result = session
            .invoke(ToolCall::new("scope.document.read").with_argument("path", "main.scope".into()))
            .unwrap();
        assert_eq!(
            field(&scope_result.value, "text"),
            Some(&ToolValue::from("fn main() {}\n"))
        );

        let glyph_result = session.invoke(ToolCall::new("glyph.axiom.list")).unwrap();
        assert_eq!(
            field(&glyph_result.value, "axioms"),
            Some(&ToolValue::array([ToolValue::from("identity")]))
        );
    }

    #[test]
    fn scope_provider_can_replace_and_save_document_text() {
        let root = TempWorkspace::new("scope_provider_can_replace_and_save_document_text");
        root.write_file("main.scope", "fn old() {}\n");
        let mut app = App::new(root.workspace()).unwrap();

        let revision = app
            .workspace()
            .load_file_revision("main.scope")
            .unwrap()
            .revision;
        let mut session = AgentSession::builder()
            .provider(ScopeToolProvider::new(&mut app))
            .build()
            .unwrap();
        let result = session
            .invoke(
                ToolCall::new("scope.document.replace")
                    .with_argument("expected_revision", revision.into())
                    .with_argument("path", "main.scope".into())
                    .with_argument("start_line", 0usize.into())
                    .with_argument("start_column", 3usize.into())
                    .with_argument("end_line", 0usize.into())
                    .with_argument("end_column", 6usize.into())
                    .with_argument("replacement", "main".into()),
            )
            .unwrap();

        assert_eq!(field(&result.value, "saved"), Some(&ToolValue::from(true)));
        assert_eq!(root.read_file("main.scope"), "fn main() {}\n");
    }

    #[test]
    fn language_queries_preserve_unsaved_document_and_identity() {
        let root = TempWorkspace::new("language_queries_preserve_unsaved_document");
        root.write_file("main.scope", "fn old() {}\n");
        root.write_file("other.scope", "fn other() {}\n");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("main.scope").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(TextEdit::replace(
                TextRange::new(Position::new(0, 3), Position::new(0, 6)),
                "unsaved",
            ))
            .unwrap();
        let before = app.open_document().unwrap().clone();

        for tool in ["scope.language.symbols", "scope.language.diagnostics"] {
            for path in ["main.scope", "other.scope"] {
                let result = ScopeToolProvider::new(&mut app)
                    .invoke(ToolCall::new(tool).with_argument("path", path.into()))
                    .unwrap();
                assert!(matches!(result.value, ToolValue::Object(_)));
                assert_eq!(app.open_document(), Some(&before));
            }
        }
        assert_eq!(root.read_file("main.scope"), "fn old() {}\n");
        app.open_path("other.scope").unwrap();
        assert_eq!(
            app.open_document().unwrap().id().as_u64(),
            before.id().as_u64() + 1
        );
    }

    #[test]
    fn language_queries_do_not_open_a_document() {
        let root = TempWorkspace::new("language_queries_do_not_open_a_document");
        root.write_file("main.scope", "fn main() {}\n");
        let mut app = App::new(root.workspace()).unwrap();
        for tool in ["scope.language.symbols", "scope.language.diagnostics"] {
            ScopeToolProvider::new(&mut app)
                .invoke(ToolCall::new(tool).with_argument("path", "main.scope".into()))
                .unwrap();
            assert!(app.open_document().is_none());
        }
    }

    #[test]
    fn builder_rejects_duplicate_tool_names() {
        let error = match AgentSession::builder()
            .provider(FakeGlyphToolProvider::default())
            .provider(FakeGlyphToolProvider::default())
            .build()
        {
            Ok(_) => panic!("duplicate tool names should be rejected"),
            Err(error) => error,
        };

        assert_eq!(
            error,
            ToolError::DuplicateTool {
                name: "glyph.axiom.list".to_owned()
            }
        );
    }

    #[derive(Default)]
    struct FakeGlyphToolProvider {
        tools: Vec<ToolDefinition>,
    }

    impl ToolProvider for FakeGlyphToolProvider {
        fn tools(&self) -> &[ToolDefinition] {
            if self.tools.is_empty() {
                static TOOLS: std::sync::OnceLock<Vec<ToolDefinition>> = std::sync::OnceLock::new();
                TOOLS.get_or_init(|| {
                    vec![ToolDefinition::new(
                        "glyph.axiom.list",
                        "List Glyph axioms available in the active proof context.",
                    )]
                })
            } else {
                &self.tools
            }
        }

        fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
            match call.name.as_str() {
                "glyph.axiom.list" => Ok(ToolResult::new(ToolValue::object([(
                    "axioms",
                    ToolValue::array([ToolValue::from("identity")]),
                )]))),
                _ => Err(ToolError::unknown_tool(call.name)),
            }
        }
    }

    fn edit_call(path: &str, revision: &str) -> ToolCall {
        ToolCall::new("scope.document.replace")
            .with_argument("path", path.into())
            .with_argument("expected_revision", revision.into())
            .with_argument("start_line", 0usize.into())
            .with_argument("start_column", 3usize.into())
            .with_argument("end_line", 0usize.into())
            .with_argument("end_column", 6usize.into())
            .with_argument("replacement", "new".into())
    }

    #[test]
    fn stale_revision_and_invalid_edits_leave_state_unchanged() {
        let root = TempWorkspace::new("tool_stale_revision");
        root.write_file("main.scope", "fn old() {}\n");
        let mut app = App::new(root.workspace()).unwrap();
        let revision = app
            .workspace()
            .load_file_revision("main.scope")
            .unwrap()
            .revision;
        root.write_file("main.scope", "fn external() {}\n");
        assert!(matches!(
            ScopeToolProvider::new(&mut app).invoke(edit_call("main.scope", &revision)),
            Err(ToolError::Conflict { .. })
        ));
        assert!(app.open_document().is_none());
        app.open_path("main.scope").unwrap();
        let before = app.open_document().unwrap().clone();
        let call = edit_call("main.scope", before.disk_revision().unwrap())
            .with_argument("expected_document_id", before.id().as_u64().into())
            .with_argument("expected_version", before.version().into())
            .with_argument("end_line", 999usize.into());
        assert!(ScopeToolProvider::new(&mut app).invoke(call).is_err());
        assert_eq!(app.open_document(), Some(&before));
        assert_eq!(root.read_file("main.scope"), "fn external() {}\n");
    }

    #[test]
    fn dirty_buffer_is_preserved_and_snapshot_checked() {
        let root = TempWorkspace::new("tool_dirty_buffer");
        root.write_file("main.scope", "fn old() {}\n");
        root.write_file("other.scope", "fn old() {}\n");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("main.scope").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(TextEdit::replace(
                TextRange::new(Position::new(0, 3), Position::new(0, 6)),
                "dirty",
            ))
            .unwrap();
        let before = app.open_document().unwrap().clone();
        let revision = app
            .workspace()
            .load_file_revision("other.scope")
            .unwrap()
            .revision;
        assert!(matches!(
            ScopeToolProvider::new(&mut app).invoke(edit_call("other.scope", &revision)),
            Err(ToolError::Conflict { .. })
        ));
        let call = edit_call("main.scope", before.disk_revision().unwrap())
            .with_argument("expected_document_id", before.id().as_u64().into())
            .with_argument("expected_version", (before.version() + 1).into());
        assert!(matches!(
            ScopeToolProvider::new(&mut app).invoke(call.clone()),
            Err(ToolError::Conflict { .. })
        ));
        assert_eq!(app.open_document(), Some(&before));
        let call = call
            .with_argument("expected_version", before.version().into())
            .with_argument("end_column", 8usize.into())
            .with_argument("save", false.into());
        ScopeToolProvider::new(&mut app).invoke(call).unwrap();
        let after = app.open_document().unwrap();
        assert_eq!(after.id(), before.id());
        assert_eq!(after.text(), "fn new() {}\n");
        assert!(after.is_dirty());
        assert_eq!(root.read_file("main.scope"), "fn old() {}\n");
    }

    #[test]
    fn policy_prevents_dispatch_and_observes_all_outcomes() {
        struct NeverDispatch(Vec<ToolDefinition>);
        impl ToolProvider for NeverDispatch {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                panic!("denied call dispatched")
            }
        }
        let mut session = AgentSession::builder()
            .provider(NeverDispatch(vec![
                ToolDefinition::new("write", "write").effect(ToolEffect::Write),
            ]))
            .provider(FakeGlyphToolProvider::default())
            .build()
            .unwrap();
        let mut events = Vec::new();
        assert!(matches!(
            session.invoke_observed_with_policy(
                ToolCall::new("write"),
                |tool| tool.effect == ToolEffect::Read,
                |event| events.push(event.clone())
            ),
            Err(ToolError::Denied { .. })
        ));
        session
            .invoke_observed(ToolCall::new("glyph.axiom.list"), |event| {
                events.push(event.clone())
            })
            .unwrap();
        assert!(
            session
                .invoke_observed(ToolCall::new("missing"), |event| events.push(event.clone()))
                .is_err()
        );
        assert_eq!(events.len(), 6);
        for (index, outcome) in [
            ToolExecutionOutcome::Denied,
            ToolExecutionOutcome::Success,
            ToolExecutionOutcome::UnknownTool,
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                matches!(&events[index * 2], ToolExecutionEvent::Started { call_id, .. } if *call_id == index as u64 + 1)
            );
            assert!(
                matches!(&events[index * 2 + 1], ToolExecutionEvent::Finished { call_id, outcome: actual, .. } if *call_id == index as u64 + 1 && *actual == outcome)
            );
        }
    }

    #[test]
    fn call_policy_sees_arguments_before_dispatch() {
        struct Echo(Vec<ToolDefinition>);
        impl ToolProvider for Echo {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }
            fn invoke(&mut self, call: ToolCall) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::new(ToolValue::Object(call.arguments)))
            }
        }
        let mut session = AgentSession::builder()
            .provider(Echo(vec![ToolDefinition::new("echo", "echo")]))
            .build()
            .unwrap();
        let expected = ToolCall::new("echo").with_argument("path", "allowed.txt".into());
        let captured = String::from("allowed.txt");
        let result = session
            .invoke_with_call_policy(expected.clone(), move |definition, call| {
                assert_eq!(definition.name, "echo");
                assert_eq!(call.name, "echo");
                assert_eq!(
                    call.arguments.get("path").unwrap().as_str(),
                    Some(captured.as_str())
                );
                drop(captured);
                true
            })
            .unwrap();
        assert_eq!(result.value, ToolValue::Object(expected.arguments));
    }

    #[test]
    fn call_policy_denial_never_dispatches_and_is_observed() {
        struct NeverDispatch(Vec<ToolDefinition>);
        impl ToolProvider for NeverDispatch {
            fn tools(&self) -> &[ToolDefinition] {
                &self.0
            }
            fn invoke(&mut self, _: ToolCall) -> Result<ToolResult, ToolError> {
                panic!("denied call dispatched")
            }
        }
        let mut session = AgentSession::builder()
            .provider(NeverDispatch(vec![ToolDefinition::new("write", "write")]))
            .build()
            .unwrap();
        let call = ToolCall::new("write").with_argument("path", "blocked.txt".into());
        let policy = |definition: &ToolDefinition, call: &ToolCall| {
            assert_eq!(definition.name, call.name);
            call.arguments.get("path").unwrap().as_str() != Some("blocked.txt")
        };
        assert!(matches!(
            session.invoke_with_call_policy(call.clone(), policy),
            Err(ToolError::Denied { tool }) if tool == "write"
        ));
        let mut events = Vec::new();
        assert!(matches!(
            session.invoke_observed_with_call_policy(call, policy, |event| {
                events.push(event.clone())
            }),
            Err(ToolError::Denied { tool }) if tool == "write"
        ));
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            ToolExecutionEvent::Started { call_id: 2, name } if name == "write"
        ));
        assert!(matches!(
            &events[1],
            ToolExecutionEvent::Finished { call_id: 2, name, outcome: ToolExecutionOutcome::Denied, .. }
                if name == "write"
        ));
    }

    fn field<'a>(value: &'a ToolValue, name: &str) -> Option<&'a ToolValue> {
        match value {
            ToolValue::Object(fields) => fields.get(name),
            _ => None,
        }
    }
}
