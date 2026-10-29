use std::io::{BufRead, IsTerminal, Write};

#[cfg(all(test, feature = "agent"))]
mod agent_inspection_tests;
#[cfg(feature = "agent")]
mod agent_semantics;
#[cfg(feature = "agent")]
pub use agent_semantics::{
    TerminalSemanticConfig, run_terminal_agent_model,
    run_terminal_agent_model_with_projected_arguments,
};
#[cfg(feature = "agent")]
mod retained_agent;
#[cfg(feature = "agent")]
pub use retained_agent::{
    RetainedScratchAgentConfig, RetainedScratchAgentRun, run_retained_scratch_agent,
};
mod buffer_editor;
mod script_context;

/// Read-only admission of a complete accumulated Script source snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptSnapshotAdmission {
    /// Complete and safe to replay in full, with no print or host effects.
    CompletePure,
    /// Complete, but full-snapshot execution requires explicit user consent.
    ExplicitRunRequired,
    Incomplete,
    Invalid(String),
}
pub mod composition;
pub mod workspace_command;
#[cfg(feature = "tui")]
mod workspace_ui;
pub use composition::{
    BindingTransition, Composition, CompositionReport, CompositionStep, SelectedMetadata,
};
mod context;
mod downstream;
#[cfg(all(test, feature = "agent"))]
mod retained_agent_shell_tests;
use context::{print_app_target, print_buffer_target, print_source_context};
pub use downstream::{DownstreamError, DownstreamOutcome};
#[cfg(feature = "tools")]
mod inspection;
mod source_proposals;

use crate::history::{CommandHistory, HistoryStore};

#[cfg(feature = "agent")]
use crate::agent::{AgentConfig, CellAgent};
use crate::app::App;
use crate::language::{InputRoute, InteractiveSession, LanguageRegistry, LanguageService};
use crate::path::ScopePath;
use crate::scratch::ScratchBuffers;
use crate::text::{Position, TextEdit, TextRange};
#[cfg(feature = "tui")]
use crate::tui;
use crate::workspace::{SnapshotOptions, Workspace, WorkspaceMeta};

#[cfg(not(feature = "agent"))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentConfig;

const AGENT_FEATURE_REQUIRED: &str = "agent support is disabled in this binary; from the Scope repository run:\n  cargo run --locked -p scope-workbench --features agent -- shell\nCargo's --features agent must appear before --; runtime options cannot enable it. For the standalone CLI use -p scope-ide instead. Downstream hosts must enable scope-ide/agent.";

#[cfg(feature = "agent")]
const DEFAULT_AGENT_MODEL: &str = "qwen3-30b-a3b";
#[cfg(feature = "agent")]
const DEFAULT_AGENT_BASE_URL: &str = "http://localhost:1234/v1";

pub fn run_from_env() -> Result<(), Box<dyn std::error::Error>> {
    run_from_args(std::env::args().skip(1))
}

pub fn run_from_args(
    args: impl IntoIterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    run_with_languages(args, [])
}

/// Runs standard CLI argument/workspace startup with an explicitly installed host callback.
/// Shell scratch editing/completion and `:scope script run` share this callback.
/// Non-shell commands retain their normal behavior and do not invoke it.
pub fn run_from_args_with_command(
    args: impl IntoIterator<Item = String>,
    command: &mut dyn ReadOnlyCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    run_with_registry_and_command(args, LanguageRegistry::builtins(), command)
}

/// Runs standard CLI startup with an explicit language registry and host callback.
/// Reuses argument parsing, workspace authority and terminal selection. Registration
/// grants no additional filesystem, semantic, editing or source-application authority;
/// callbacks must authorize concrete operations using their existing host policy.
/// Non-shell commands retain their normal behavior and do not invoke the callback.
pub fn run_with_registry_and_command(
    args: impl IntoIterator<Item = String>,
    registry: LanguageRegistry,
    command: &mut dyn ReadOnlyCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match Args::parse(args)? {
        ParseResult::Help => {
            print_usage();
            Ok(())
        }
        ParseResult::Run(args) => run_configured(
            args,
            registry,
            #[cfg(feature = "agent")]
            None,
            Some(command),
        ),
    }
}

/// Standard launcher with both a read-only command host (including contextual
/// actions) and independently authorized contracted semantic editing.
/// The editing contract lives with the workspace's retained proposal controller.
/// Only shell and ordinary workspace startup support this configuration.
#[cfg(feature = "agent")]
pub fn run_with_registry_and_command_and_editing(
    args: impl IntoIterator<Item = String>,
    registry: LanguageRegistry,
    command: &mut dyn ReadOnlyCommand,
    editing: InteractiveEditConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    match Args::parse(args)? {
        ParseResult::Help => {
            print_usage();
            Ok(())
        }
        ParseResult::Run(args) => {
            let supported = matches!(args.command, Command::Shell);
            #[cfg(feature = "tui")]
            let supported = supported || matches!(args.command, Command::Workspace);
            if !supported {
                return Err("interactive editing configuration requires shell or workspace".into());
            }
            run_configured(args, registry, Some(editing), Some(command))
        }
    }
}

pub fn run_with_languages(
    args: impl IntoIterator<Item = String>,
    languages: impl IntoIterator<Item = &'static dyn LanguageService>,
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = LanguageRegistry::builtins().with_languages(languages);

    run_with_registry(args, registry)
}

/// Runs the CLI with independently registered analysis and interactive providers.
pub fn run_with_registry(
    args: impl IntoIterator<Item = String>,
    registry: LanguageRegistry,
) -> Result<(), Box<dyn std::error::Error>> {
    match Args::parse(args)? {
        ParseResult::Help => {
            print_usage();
            Ok(())
        }
        ParseResult::Run(args) => run(args, registry),
    }
}

#[cfg(feature = "agent")]
pub use source_proposals::InteractiveEditConfig;

/// Runs a shell with a fixed host-owned contracted editing workflow.
/// Existing entry points remain proposal-only. Configured edits never downgrade.
#[cfg(feature = "agent")]
pub fn run_with_registry_and_editing(
    args: impl IntoIterator<Item = String>,
    registry: LanguageRegistry,
    editing: InteractiveEditConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    match Args::parse(args)? {
        ParseResult::Help => {
            print_usage();
            Ok(())
        }
        ParseResult::Run(args) => {
            if !matches!(args.command, Command::Shell) {
                return Err("interactive editing configuration requires the shell command".into());
            }
            run_configured(args, registry, Some(editing), None)
        }
    }
}

fn run(args: Args, languages: LanguageRegistry) -> Result<(), Box<dyn std::error::Error>> {
    run_configured(
        args,
        languages,
        #[cfg(feature = "agent")]
        None,
        None,
    )
}

fn run_configured(
    args: Args,
    languages: LanguageRegistry,
    #[cfg(feature = "agent")] editing: Option<InteractiveEditConfig>,
    host_command: Option<&mut dyn ReadOnlyCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "agent")]
    if args.agent_config.launcher_binding() {
        let supported = matches!(args.command, Command::Shell);
        #[cfg(feature = "tui")]
        let supported = supported || matches!(args.command, Command::Workspace);
        if !supported {
            return Err("--agent-local-binding requires shell or workspace; selected Script only, no App fallback".into());
        }
    }
    let root = std::path::Path::new(&args.workspace_root);
    if !root.is_dir() {
        return Err(format!(
            "workspace must be an existing directory: {}",
            root.display()
        )
        .into());
    }
    let workspace = if !args.writable || matches!(&args.command, Command::Meta { .. }) {
        Workspace::read_only(args.workspace_root)?
    } else {
        Workspace::new(args.workspace_root)?
    };

    match args.command {
        Command::List => {
            println!("workspace: {}", workspace.root());

            for entry in workspace.list_files()? {
                println!("{}", entry.path.display());
            }
        }
        Command::Capabilities => {
            print_capabilities(&workspace, &mut std::io::stdout().lock())?;
        }
        Command::Authority => {
            print_authority(&workspace, &mut std::io::stdout().lock())?;
        }
        Command::Meta { options } => {
            print_meta(&workspace, options, &mut std::io::stdout().lock())?;
        }
        Command::Show { path } => {
            let file = workspace.load_file(path)?;
            print!("{}", file.contents);
        }
        Command::Shell => {
            println!(
                "Workspace access: {}",
                if args.writable {
                    "writable (explicit --writable)"
                } else {
                    "read-only (restart with --writable to enable saves)"
                }
            );
            let app = App::builder(workspace)
                .language_registry(languages.clone())
                .build()?;
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            let interactive = stdin.is_terminal() && stdout.is_terminal();
            run_shell_configured(
                app,
                args.agent_config,
                stdin.lock(),
                stdout.lock(),
                interactive,
                #[cfg(feature = "agent")]
                editing,
                host_command,
            )?;
        }
        #[cfg(feature = "agent")]
        Command::Agent { request, open_path } => {
            let agent = CellAgent::new(args.agent_config)?;
            let mut app = App::builder(workspace)
                .language_registry(languages.clone())
                .build()?;
            if let Some(path) = open_path {
                app.open_path(path)?;
            }
            let response = run_cli_agent(&agent, &mut app, None, &request)?;
            print!("{}", response.content);
            if !response.content.ends_with('\n') {
                println!();
            }
        }
        #[cfg(feature = "tui")]
        Command::Workspace => {
            let mut app = App::builder(workspace)
                .language_registry(languages.clone())
                .build()?;
            workspace_ui::run(
                &mut app,
                host_command,
                #[cfg(feature = "agent")]
                editing,
                #[cfg(feature = "agent")]
                args.agent_config,
            )?;
        }
        #[cfg(feature = "tui")]
        Command::Tui => {
            let app = App::builder(workspace)
                .language_registry(languages.clone())
                .build()?;
            tui::run(app)?;
        }
    }

    Ok(())
}

#[cfg(feature = "agent")]
fn run_cli_agent(
    agent: &CellAgent,
    app: &mut App,
    interactive: Option<&InteractiveSession>,
    request: &str,
) -> Result<crate::agent::AgentResponse, crate::agent::AgentError> {
    run_cli_agent_model(&mut agent.clone(), app, interactive, request)
}

#[cfg(feature = "agent")]
fn run_cli_agent_model(
    model: &mut impl crate::agent::DecisionModel,
    app: &mut App,
    interactive: Option<&InteractiveSession>,
    request: &str,
) -> Result<crate::agent::AgentResponse, crate::agent::AgentError> {
    run_terminal_agent_model(
        app,
        interactive,
        model,
        request,
        crate::agent::AgentLimits::default(),
        None,
    )
}

#[cfg(feature = "agent")]
fn run_default_terminal_agent_model(
    model: &mut impl crate::agent::DecisionModel,
    app: &mut App,
    interactive: Option<&InteractiveSession>,
    request: &str,
    limits: crate::agent::AgentLimits,
) -> Result<crate::agent::AgentResponse, crate::agent::AgentError> {
    use crate::agent::{AgentError, AgentResponse, AgentStop};
    let request = format!(
        "User request:\n{request}\n\nActive Scope context:\n{}",
        crate::agent::AgentContext::from_app(app).render()
    );
    let mut builder = crate::tool::AgentSession::builder()
        .app(app)
        .provider(crate::tool::ScopeToolProvider::for_session());
    if let Some(interactive) = interactive {
        builder = builder.provider(crate::tool::inspection::InteractiveInspectionProvider::new(
            interactive,
        ));
    }
    let mut tools = builder.build().map_err(AgentError::Tool)?;
    let run = crate::agent::run_session(
        model,
        &mut tools,
        &request,
        limits,
        crate::agent::read_only_policy,
    )?;
    if run.stop != AgentStop::Completed {
        return Err(AgentError::Stopped(run.stop));
    }
    Ok(AgentResponse {
        content: run.content,
    })
}

fn print_meta(
    workspace: &Workspace,
    options: SnapshotOptions,
    output: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve the entire snapshot before emitting anything, including its header.
    let snapshot = WorkspaceMeta::new(workspace.clone(), options)?;
    let graph = snapshot.graph()?;
    let truncated = snapshot.truncated_paths()?;
    writeln!(
        output,
        "graph: {} revision: {} nodes: {} edges: {}",
        graph.graph_id(),
        graph.revision(),
        graph.nodes().len(),
        graph.edges().len()
    )?;
    for node in graph.nodes() {
        writeln!(
            output,
            "node: {:?} kind: {} label: {:?} source: {:?}",
            node.id.as_str(),
            node.kind,
            node.label,
            node.source
        )?;
    }
    for edge in graph.edges() {
        writeln!(
            output,
            "edge: {:?} --{}--> {:?} provenance: {:?}",
            edge.from.as_str(),
            edge.relation,
            edge.to.as_str(),
            edge.provenance
        )?;
    }
    for path in truncated {
        writeln!(
            output,
            "depth truncation: {:?} (directory not scanned; max_depth={})",
            path.as_str(),
            options.max_depth
        )?;
    }
    if !truncated.is_empty() {
        writeln!(
            output,
            "snapshot is not complete: depth bound left directories unscanned"
        )?;
    }
    Ok(())
}

#[cfg(test)]
fn run_shell(
    app: App,
    #[cfg_attr(not(feature = "agent"), allow(unused_variables))] agent_config: AgentConfig,
    input: impl BufRead,
    output: impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    run_shell_inner(app, agent_config, input, output, false)
}

#[cfg(test)]
fn run_shell_inner(
    app: App,
    agent_config: AgentConfig,
    input: impl BufRead,
    output: impl Write,
    interactive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    run_shell_configured(
        app,
        agent_config,
        input,
        output,
        interactive,
        #[cfg(feature = "agent")]
        None,
        None,
    )
}

/// Host-owned per-invocation semantic agent configuration. No authority is inferred
/// from registration or Read classification. Native services are trusted; resource
/// limits beyond agent iteration/operation budgets remain the host's responsibility.
#[cfg(feature = "agent")]
pub struct SemanticAgentBinding<'a> {
    pub service: &'a dyn scope_language::LanguageService,
    pub allowed_operation_ids: &'a [String],
    pub authorize:
        &'a mut dyn for<'b> FnMut(crate::tool::semantic::SemanticAuthorization<'b>) -> bool,
    pub limits: crate::agent::AgentLimits,
    /// Optional sensitive protocol sink, bounded by max_conversation_bytes.
    /// Also receives trace on model failure; never printed by the shell.
    pub trace: Option<&'a mut dyn Write>,
    /// None uses the shell's existing model and endpoint configuration.
    pub model: Option<&'a mut dyn crate::agent::DecisionModel>,
    /// Called once for a completed loop, including budget/correction stops.
    /// Copy any desired attribution/trace data before this invocation-local borrow ends.
    pub observe: &'a mut dyn FnMut(&RetainedScratchAgentRun<'_>),
}

/// Owned output of a direct Script invocation, independent of live buffers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptPresentation {
    pub source_identity: scope_language::SourceId,
    pub source_revision: scope_language::RevisionId,
    pub source_text: String,
    /// Formatted values in execution order, including prints before a failure.
    pub prints: Vec<String>,
    pub final_value: Option<String>,
    pub error: Option<ScriptPresentationError>,
}

/// An interpreter failure located in the presentation's source snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptPresentationError {
    pub message: String,
    pub byte_offset: usize,
    /// One-based line and column; columns count Unicode scalars.
    pub line: usize,
    pub column: usize,
}

/// A trusted host operation borrowing the live document for this invocation only.
/// No workspace or runtime access is supplied. Hosts remain responsible for
/// concrete operation authorization and for the purity of their native code.
/// Errors are displayed by the shell; they do not alter runtime lifecycle.
/// Syntax-only reduction result. Preconditions are not semantic validation evidence.
#[derive(Clone, Debug)]
pub struct ScriptReduction {
    pub original: scope_language::SourceSnapshot,
    pub edits: Vec<crate::text::TextEdit>,
    pub rule: String,
    pub checked: Vec<String>,
}

pub trait ReadOnlyCommand {
    /// Reduce an immutable evaluation snapshot without execution, operations or IO.
    /// The IDE separately binds the returned edits to the original scratch owner.
    fn reduce_script_snapshot(
        &self,
        _snapshot: scope_language::SourceSnapshot,
    ) -> Result<ScriptReduction, String> {
        Err("Script reduction is unsupported by this host".into())
    }
    /// Parse and classify the entire accumulated Script snapshot without execution.
    /// Purity must account for all prior statements and transitive function calls:
    /// print and host calls must never be admitted for automatic replay.
    /// Unsupported hosts deny by default. No persistent interpreter is supplied.
    fn admit_script_snapshot(&self, _source: &str) -> ScriptSnapshotAdmission {
        ScriptSnapshotAdmission::Invalid(
            "Script snapshot admission is unsupported by this host".into(),
        )
    }
    /// Contextual F1 shortcuts installed by the ordinary workspace launcher.
    /// At most 64 registrations are accepted, using `WorkspaceTui`'s metadata
    /// validation. Fixed literal arguments only; registration grants no authority.
    /// Ignored by shell and non-workspace commands; existing hosts default to none.
    #[cfg(feature = "tui")]
    fn downstream_actions(&self) -> Vec<crate::tui::DownstreamAction> {
        Vec::new()
    }

    /// Bounded declarative workflows exposed only as `:scope NAME COMMAND`.
    /// Registrations are validated and snapshotted before the shell starts.
    fn compositions(&self) -> &[composition::Composition] {
        &[]
    }
    /// Receives owned metadata snapshots after a successful or canceled composition.
    /// Not called on errors; this notification grants no operation authorization.
    fn composition_completed(&mut self, _report: &composition::CompositionReport) {}

    /// Legacy hosts may retain generic App agents. Document-only hosts can deny
    /// that routing even when no semantic binding is installed; denial must not
    /// construct a backend or silently substitute another source owner.
    #[cfg(feature = "agent")]
    fn allow_app_agent_fallback(&self) -> bool {
        true
    }

    /// Opt in to the normal agent command against the independently bound scratch.
    /// Returning Some forbids App/editor/disk fallback, even on resolution failure.
    #[cfg(feature = "agent")]
    fn semantic_agent(&mut self) -> Option<SemanticAgentBinding<'_>> {
        None
    }
    /// Explicit namespaces exposed by `:scope downstream`. At most 64 names,
    /// each 1–64 ASCII bytes matching `[a-z][a-z0-9_-]*`; duplicates are errors.
    /// Metadata is snapshotted and validated once before the shell starts.
    fn downstream_namespaces(&self) -> &[&str] {
        &[]
    }

    /// Dispatches whitespace-delimited arguments without quoting or expansion.
    /// The independently bound semantic target is an invocation-local,
    /// immutable live-memory borrow. No binding is `None`;
    /// a closed explicit target errors before invocation. Native hosts remain
    /// trusted and responsible for authorization and any external effects.
    fn invoke_downstream(
        &mut self,
        _namespace: &str,
        _command: &str,
        _args: &[&str],
        _semantic_target: Option<&crate::scratch::ScratchBuffer>,
        _output: &mut dyn Write,
    ) -> Result<DownstreamOutcome, DownstreamError> {
        Ok(DownstreamOutcome::Unhandled)
    }
    /// Whether this host explicitly supports target-free, structured Script execution.
    fn supports_direct_script(&self) -> bool {
        false
    }

    /// Executes only the supplied scratch, without a semantic target or output writer.
    /// Unsupported hosts deny by default, independently of the support query.
    /// Dispatch/validation failures are returned; interpreter failures belong in
    /// the owned presentation alongside any prints produced before failure.
    fn invoke_script_direct(
        &mut self,
        _source: &crate::scratch::ScratchBuffer,
    ) -> Result<ScriptPresentation, Box<dyn std::error::Error>> {
        Err("direct script execution is unsupported by this read-only host command".into())
    }

    /// Runs selected scratch source without an App document or active session.
    /// The source borrow is invocation-local. Hosts must explicitly authorize
    /// execution and remain responsible for all native effects.
    fn invoke_script(
        &mut self,
        _source: &crate::scratch::ScratchBuffer,
        _output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Err("script execution is unsupported by this read-only host command".into())
    }
    /// Executes selected Script source with an independently host-bound scratch.
    /// Both immutable borrows pin their revisions for this invocation. The target
    /// is resolved from live memory, never disk; `None` means no binding. Failed
    /// explicit resolution is reported before dispatch. No mutable source, App,
    /// workspace, save, or apply interface is supplied. Native callbacks remain
    /// trusted and must enforce concrete semantic operation authorization.
    /// Existing callbacks retain their Script-only behavior by default.
    fn invoke_script_with_target(
        &mut self,
        source: &crate::scratch::ScratchBuffer,
        _target: Option<&crate::scratch::ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.invoke_script(source, output)
    }

    /// Continuous-context presentation. Legacy hosts retain their existing output.
    fn invoke_script_context(
        &mut self,
        source: &crate::scratch::ScratchBuffer,
        target: Option<&crate::scratch::ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.invoke_script_with_target(source, target, output)
    }

    /// Inspect the latest owned Script record without evaluating live source.
    fn inspect_script_evidence(
        &self,
        _output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Err("Script evidence inspection is unsupported by this host".into())
    }

    /// Inspects the selected scratch's current editor draft without evaluation.
    /// `cursor` and completion replacement ranges are UTF-8 byte offsets in `draft`,
    /// not in the committed scratch text. Borrows are invocation-local; hosts are
    /// responsible for keeping native inspection read-only. Defaults to no suggestions.
    fn complete_source(
        &self,
        _source: &crate::scratch::ScratchBuffer,
        _draft: &str,
        _cursor: usize,
    ) -> Vec<crate::language::CompletionItem> {
        Vec::new()
    }

    fn invoke(
        &mut self,
        document: &crate::document::Document,
        selection: TextRange,
        operation: &str,
        output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Runs an operation on selected scratch source with a separate App target.
    /// The selection is a zero-based, half-open UTF-8 byte range in `document`,
    /// not in the independently selected `source`. Both borrows are invocation-local and read-only.
    /// Existing hosts deny source operations unless they explicitly opt in.
    fn invoke_source(
        &mut self,
        _source: &crate::scratch::ScratchBuffer,
        _document: &crate::document::Document,
        _selection: TextRange,
        _operation: &str,
        _output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Err("source operations are unsupported by this read-only host command".into())
    }
}

/// Runs the owned shell with one explicitly installed read-only host callback.
/// Invoke with `:scope host NAME --selection LINE:BYTE..LINE:BYTE` (zero-based).
/// `:scope run NAME --selection LINE:BYTE..LINE:BYTE` instead supplies the selected
/// scratch as source and the open App document as a separate target.
/// `:scope script run` supplies the selected scratch and optional independently
/// bound scratch (`:scope semantic-target ID`, `none` to clear), without a session.
/// All source borrows end before the next shell submission. A host may construct
/// a command-local `DocumentSource` using its registered language service.
pub fn run_with_session_and_command(
    app: App,
    session: InteractiveSession,
    command: &mut dyn ReadOnlyCommand,
    input: impl BufRead,
    output: impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    run_shell_owned(
        app,
        session,
        #[cfg(feature = "agent")]
        AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL),
        #[cfg(not(feature = "agent"))]
        AgentConfig,
        input,
        output,
        false,
        #[cfg(feature = "agent")]
        None,
        Some(command),
    )
}

/// Runs the terminal shell with the supplied session and read-only host command.
/// Unlike `run_with_session_and_command`, this explicitly uses ambient terminal
/// stdin/stdout, raw-mode shell input, and persistent history configuration.
/// The selected scratch editor remains transactional and cooked-mode; its
/// `:complete [INDEX]` command uses the host's source completion hook, not the
/// active session's runtime. No session or provider is installed implicitly.
pub fn run_terminal_with_session_and_command(
    app: App,
    session: InteractiveSession,
    command: &mut dyn ReadOnlyCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{self, IsTerminal};
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("terminal shell requires terminal stdin and stdout".into());
    }
    run_shell_owned(
        app,
        session,
        #[cfg(feature = "agent")]
        AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL),
        #[cfg(not(feature = "agent"))]
        AgentConfig,
        io::stdin().lock(),
        io::stdout(),
        true,
        #[cfg(feature = "agent")]
        None,
        Some(command),
    )
}

/// Temporarily runs the normal terminal shell using caller-owned application state.
///
/// Returns on `:scope quit`, terminal EOF, or error without clearing or dropping
/// the supplied session. Explicit session commands may still replace, reset, or
/// clear it. App changes and runtime state remain available to the caller.
/// `command` optionally installs the same host hooks as the owned shell.
///
/// The caller must suspend its terminal UI and release all terminal input readers
/// before calling, then restore its UI even if this returns an error. This uses
/// ambient terminal stdin/stdout, the existing raw-mode input reader, and history
/// configuration; it does not manage an alternate screen or resume a TUI.
/// Scratch buffers, pending proposals, inspection state, and submission capture
/// are local to this invocation and are discarded on return (never auto-saved).
/// No provider or editing authority is installed implicitly; agent configuration
/// uses the same defaults as `run_terminal_with_session_and_command`.
pub fn run_terminal_borrowed(
    app: &mut App,
    session: &mut InteractiveSession,
    command: Option<&mut dyn ReadOnlyCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("terminal shell requires terminal stdin and stdout".into());
    }
    run_shell_borrowed(
        app,
        session,
        #[cfg(feature = "agent")]
        AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL),
        #[cfg(not(feature = "agent"))]
        AgentConfig,
        io::stdin().lock(),
        io::stdout(),
        true,
        #[cfg(feature = "agent")]
        None,
        command,
    )
}

/// Runs the normal borrowed terminal shell with caller-owned scratch state.
/// Scratch identities, revisions, selection, binding and closed-target state survive
/// quit, EOF and errors. No save, evaluation or authorization is implicit.
pub fn run_terminal_borrowed_with_sources(
    app: &mut App,
    session: &mut InteractiveSession,
    command: Option<&mut dyn ReadOnlyCommand>,
    sources: &mut ScratchBuffers,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("terminal shell requires terminal stdin and stdout".into());
    }
    run_shell_borrowed_with_sources(
        app,
        session,
        #[cfg(feature = "agent")]
        AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL),
        #[cfg(not(feature = "agent"))]
        AgentConfig,
        io::stdin().lock(),
        io::stdout(),
        true,
        #[cfg(feature = "agent")]
        None,
        command,
        sources,
    )
}

/// Stream-driven equivalent of the retained borrowed terminal shell. Uses only
/// supplied I/O, without terminal history or ambient readers. Session and source
/// mutations made before an I/O error remain with their caller-owned owners.
pub fn run_borrowed_with_sources(
    app: &mut App,
    session: &mut InteractiveSession,
    command: Option<&mut dyn ReadOnlyCommand>,
    sources: &mut ScratchBuffers,
    input: impl BufRead,
    output: impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    run_shell_borrowed_with_sources(
        app,
        session,
        #[cfg(feature = "agent")]
        AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL),
        #[cfg(not(feature = "agent"))]
        AgentConfig,
        input,
        output,
        false,
        #[cfg(feature = "agent")]
        None,
        command,
        sources,
    )
}

/// Runs the generic shell with a host-created application and session owner.
///
/// Takes ownership of both. REPL and explicit buffer evaluation use this exact
/// controller. Input is read exclusively from `input`, including at EOF; terminal
/// history and ambient terminal readers are disabled. Commands are identical to
/// the normal shell. Owned sessions close on quit, EOF, or I/O failure (best effort
/// on error/Drop). Hosts may retain typed runtime handles through their adapters.
/// No providers, filesystem capabilities, or agent tools are registered implicitly.
pub fn run_with_session(
    app: App,
    session: InteractiveSession,
    input: impl BufRead,
    output: impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    run_shell_owned(
        app,
        session,
        #[cfg(feature = "agent")]
        AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL),
        #[cfg(not(feature = "agent"))]
        AgentConfig,
        input,
        output,
        false,
        #[cfg(feature = "agent")]
        None,
        None,
    )
}

fn run_shell_configured(
    app: App,
    agent_config: AgentConfig,
    input: impl BufRead,
    output: impl Write,
    interactive: bool,
    #[cfg(feature = "agent")] editing: Option<InteractiveEditConfig>,
    host_command: Option<&mut dyn ReadOnlyCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    run_shell_owned(
        app,
        InteractiveSession::new(),
        agent_config,
        input,
        output,
        interactive,
        #[cfg(feature = "agent")]
        editing,
        host_command,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Shell orchestration accepts independent host state, I/O, and optional feature configuration"
)]
fn run_shell_owned(
    mut app: App,
    mut session: InteractiveSession,
    #[cfg_attr(not(feature = "agent"), allow(unused_variables))] agent_config: AgentConfig,
    input: impl BufRead,
    output: impl Write,
    interactive: bool,
    #[cfg(feature = "agent")] editing: Option<InteractiveEditConfig>,
    host_command: Option<&mut dyn ReadOnlyCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    run_shell_borrowed(
        &mut app,
        &mut session,
        agent_config,
        input,
        output,
        interactive,
        #[cfg(feature = "agent")]
        editing,
        host_command,
    )?;
    session.clear()?;
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "Shell orchestration accepts independent host state, I/O, and optional feature configuration"
)]
fn run_shell_borrowed(
    app: &mut App,
    session: &mut InteractiveSession,
    #[cfg_attr(not(feature = "agent"), allow(unused_variables))] agent_config: AgentConfig,
    input: impl BufRead,
    output: impl Write,
    interactive: bool,
    #[cfg(feature = "agent")] editing: Option<InteractiveEditConfig>,
    host_command: Option<&mut dyn ReadOnlyCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    run_shell_borrowed_with_sources(
        app,
        session,
        agent_config,
        input,
        output,
        interactive,
        #[cfg(feature = "agent")]
        editing,
        host_command,
        &mut ScratchBuffers::default(),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_shell_borrowed_with_sources(
    app: &mut App,
    session: &mut InteractiveSession,
    agent_config: AgentConfig,
    input: impl BufRead,
    output: impl Write,
    interactive: bool,
    #[cfg(feature = "agent")] editing: Option<InteractiveEditConfig>,
    host_command: Option<&mut dyn ReadOnlyCommand>,
    scratches: &mut ScratchBuffers,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "agent")]
    let mut proposals = source_proposals::Proposals::configured(editing)?;
    #[cfg(not(feature = "agent"))]
    let mut proposals = source_proposals::Proposals::default();
    run_shell_with_proposals(
        app,
        session,
        agent_config,
        input,
        output,
        interactive,
        host_command,
        scratches,
        &mut proposals,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_shell_with_proposals(
    #[cfg_attr(not(feature = "tools"), allow(unused_mut))] app: &mut App,
    session: &mut InteractiveSession,
    #[cfg_attr(not(feature = "agent"), allow(unused_variables))] agent_config: AgentConfig,
    mut input: impl BufRead,
    mut output: impl Write,
    interactive: bool,
    mut host_command: Option<&mut dyn ReadOnlyCommand>,
    scratches: &mut ScratchBuffers,
    source_proposals: &mut source_proposals::Proposals,
) -> Result<(), Box<dyn std::error::Error>> {
    let downstream_namespaces = downstream::registrations(host_command.as_deref())?;
    let compositions = composition::registrations(host_command.as_deref())?;
    let max_history = if interactive {
        std::env::var("SCOPE_HISTORY_LIMIT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1000)
    } else {
        1000
    };
    let mut history = CommandHistory::new(max_history);
    let store = if interactive {
        std::env::var("SCOPE_HISTORY_FILE")
            .ok()
            .filter(|path| !path.is_empty())
            .and_then(|path| ScopePath::new(path).ok())
            .map(|path| HistoryStore::new(app.workspace().clone(), path))
    } else {
        None
    };
    if let Some(store) = &store {
        store.load(&mut history);
    }
    writeln!(output, "Scope — {}", app.workspace().root())?;
    writeln!(
        output,
        "Type `help` for commands, `quit` to exit. Workspace files change only on explicit commands."
    )?;
    writeln!(
        output,
        "Discover: :scope languages | :scope session | :scope context | :scope inspect"
    )?;
    writeln!(
        output,
        "Scratch edits: :scope scratch new <language-id>, then :scope edit (in memory; not evaluation)."
    )?;
    writeln!(
        output,
        "Script: :scope script enter / leave selects continuous context; :scope script run evaluates selected scratch. Standalone: cargo run -p scope-script -- --help"
    )?;
    let mut proposal: Option<crate::workspace::Proposal> = None;
    #[cfg(feature = "tools")]
    let mut inspections = inspection::Inspections::default();

    #[cfg(feature = "agent")]
    if source_proposals.requires_evaluation() {
        writeln!(
            output,
            "Agent editing: host-configured contract required; no unevaluated fallback; proposals require explicit review and acceptance."
        )?;
    }
    let mut last_submission: Option<(String, String)> = None;
    let mut script_context = script_context::ScriptContext::default();

    loop {
        source_proposals.observe(scratches);
        #[cfg(feature = "tools")]
        inspections.observe(app);
        let script_active = script_context.is_active();
        let prompt = if script_active {
            format!("{} ", script_context.prompt().trim_end())
        } else {
            format!("{}> ", session.language_id().unwrap_or("scope"))
        };
        write!(output, "{prompt}")?;
        output.flush()?;

        let line = if interactive {
            let line = if script_active {
                crate::shell_input::read_line_with(
                    &mut history,
                    &mut output,
                    &prompt,
                    true,
                    |_| crate::language::Completeness::Complete,
                    |line, cursor| {
                        script_context.complete(line, cursor, scratches, host_command.as_deref())
                    },
                )?
            } else {
                crate::shell_input::read_line(&mut history, &mut output, session)?
            };
            let Some(line) = line else {
                break;
            };
            line
        } else {
            let mut line = String::new();
            if input.read_line(&mut line)? == 0 {
                writeln!(output)?;
                break;
            }
            let line = line.strip_suffix('\n').unwrap_or(&line);
            line.strip_suffix('\r').unwrap_or(line).to_owned()
        };
        let line = line.as_str();
        if interactive
            && history.record(line)
            && let Some(store) = &store
            && let Err(error) = store.save(&history)
        {
            writeln!(
                output,
                "warning: command history could not be saved: {error}"
            )?;
        }
        if line.trim().is_empty() {
            continue;
        }

        if script_context.is_active() && matches!(line.trim(), ":compact" | ":scope compact") {
            let result = script_context.compact(
                source_proposals,
                scratches,
                host_command.as_deref(),
                &mut output,
            );
            if let Err(error) = result {
                writeln!(output, "error: {error}")?;
            }
            continue;
        }
        let routed_line =
            match script_context.route(line, scratches, host_command.as_deref(), &mut output) {
                Ok(Some(line)) => line,
                Ok(None) => continue,
                Err(error) => {
                    writeln!(output, "error: {error}")?;
                    continue;
                }
            };
        let line = routed_line.as_str();
        let command = match session.route(line) {
            InputRoute::Source(source) => {
                last_submission =
                    Some((session.language_id().unwrap().to_owned(), source.to_owned()));
                match session.evaluate(source) {
                    Ok(result) => {
                        print_evaluation(result, &mut output)?;
                    }
                    Err(error) => writeln!(output, "error: {error}")?,
                }
                continue;
            }
            InputRoute::ScopeCommand(command) => command,
        };
        // Session controls exist only in the explicit namespace, preserving legacy parsing.
        if line == ":scope" || line.starts_with(":scope ") {
            let mut parts = command.split_whitespace();
            if let Some(namespace) = parts.clone().next()
                && compositions
                    .iter()
                    .any(|entry| entry.namespace == namespace)
            {
                let tokens: Vec<_> = command.split_whitespace().collect();
                if tokens.len() == 2
                    && let Some(entry) = compositions
                        .iter()
                        .find(|entry| entry.namespace == tokens[0] && entry.command == tokens[1])
                {
                    if let Err(error) = composition::execute(
                        entry,
                        scratches,
                        app.workspace(),
                        app.languages(),
                        &mut host_command,
                        &mut input,
                        &mut output,
                    ) {
                        writeln!(output, "error: {error}")?;
                    }
                } else {
                    writeln!(
                        output,
                        "error: expected registered :scope {namespace} COMMAND (no arguments)"
                    )?;
                }
                continue;
            }
            match parts.next() {
                Some("downstream") => {
                    if let Err(error) = downstream::dispatch(
                        command.trim_start().strip_prefix("downstream").unwrap(),
                        &downstream_namespaces,
                        &mut host_command,
                        scratches,
                        &mut output,
                    ) {
                        writeln!(output, "error: {error}")?;
                    }
                    continue;
                }
                Some("line") => {
                    if let Err(error) = replace_scratch_line(
                        command.trim_start().strip_prefix("line").unwrap(),
                        scratches,
                    ) {
                        writeln!(output, "error: {error}")?;
                    } else {
                        writeln!(output, "scratch line replaced (no save or evaluation)")?;
                    }
                    continue;
                }
                Some("edit") if matches!(command.trim(), "edit" | "edit --commands") => {
                    if command.trim() == "edit" && scope_terminal::document_editing_available() {
                        if let Err(error) = buffer_editor::run_keyboard(scratches, &mut output) {
                            writeln!(output, "error: {error}")?;
                        }
                        writeln!(output, "Returned to REPL (no save or evaluation).")?;
                        continue;
                    }
                    writeln!(
                        output,
                        "Editing draft; :help for commands, :done to keep, :cancel to cancel. Nothing is executed or saved."
                    )?;
                    if let Err(error) = buffer_editor::run_with_completion(
                        scratches,
                        &mut input,
                        &mut output,
                        &|source, draft, cursor| {
                            host_command.as_deref().map_or_else(Vec::new, |host| {
                                host.complete_source(source, draft, cursor)
                            })
                        },
                    ) {
                        writeln!(output, "error: {error}")?;
                    }
                    writeln!(output, "Returned to REPL (no save or evaluation).")?;
                    continue;
                }
                Some(
                    operation @ ("open" | "buffers" | "buffer" | "show" | "save" | "save-as"
                    | "reload" | "reload-discard" | "close" | "discard"),
                ) => {
                    let mapped = match operation {
                        "buffers" => "list",
                        "buffer" if command.split_whitespace().nth(1) == Some("eval") => "eval",
                        "buffer" => "select",
                        other => other,
                    };
                    let arguments = command.trim_start().strip_prefix(operation).unwrap();
                    let arguments = if operation == "buffer" && mapped == "eval" {
                        arguments.trim_start().strip_prefix("eval").unwrap()
                    } else {
                        arguments
                    };
                    if operation == "show" {
                        if !arguments.trim().is_empty() {
                            writeln!(
                                output,
                                "error: show requires no arguments; use :scope edit --commands then :show START to page"
                            )?;
                        } else if let Err(error) = print_buffer(scratches, &mut output) {
                            writeln!(output, "error: {error}")?;
                        }
                    } else if operation == "buffer"
                        && mapped == "eval"
                        && arguments.trim().is_empty()
                        && scratches.selected().is_ok()
                        && session.language_id().is_none()
                    {
                        writeln!(
                            output,
                            "error: no interactive language selected; use :scope session <id>"
                        )?;
                    } else if let Err(error) = run_scratch_command(
                        &format!("{mapped}{arguments}"),
                        scratches,
                        session,
                        last_submission.as_ref(),
                        app.workspace(),
                        app.languages(),
                        &mut output,
                    ) {
                        writeln!(output, "error: {error}")?;
                    }
                    continue;
                }
                Some("semantic-target") => {
                    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                        let argument = parts.next();
                        if parts.next().is_some() {
                            return Err("usage: :scope semantic-target [ID|none]".into());
                        }
                        if let Some(argument) = argument {
                            let id = if argument == "none" {
                                None
                            } else {
                                Some(
                                    argument
                                        .parse::<u64>()
                                        .map_err(|_| "usage: :scope semantic-target [ID|none]")?,
                                )
                            };
                            scratches.bind_semantic_target(id)?;
                        }
                        match scratches.semantic_target_id() {
                            Some(id) => writeln!(output, "Semantic target: scratch {id}")?,
                            None => writeln!(output, "Semantic target: none")?,
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        writeln!(output, "error: {error}")?;
                    }
                    continue;
                }
                Some("scratch") => {
                    if let Err(error) = run_scratch_command(
                        command.trim_start().strip_prefix("scratch").unwrap(),
                        scratches,
                        session,
                        last_submission.as_ref(),
                        app.workspace(),
                        app.languages(),
                        &mut output,
                    ) {
                        writeln!(output, "error: {error}")?;
                    }
                    continue;
                }
                Some(SESSION_COMMAND) => {
                    let id = parts.next();
                    if let Some(extra) = parts.next() {
                        writeln!(output, "error: unexpected argument: {extra}")?;
                    } else if let Some(id) = id {
                        let result = if id == "none" {
                            session.clear()
                        } else {
                            session.select(app.languages(), id)
                        };
                        match result {
                            Ok(()) => writeln!(
                                output,
                                "Active language: {}",
                                session.language_id().unwrap_or("none")
                            )?,
                            Err(error) => writeln!(output, "error: {error}")?,
                        }
                    } else {
                        writeln!(
                            output,
                            "Active language: {}",
                            session.language_id().unwrap_or("none")
                        )?;
                    }
                    continue;
                }
                Some(mode @ ("describe" | "effect" | "where")) => {
                    match (parts.next(), parts.next()) {
                        (None, _) => writeln!(output, "error: {mode} requires a symbol")?,
                        (_, Some(extra)) => writeln!(
                            output,
                            "error: unexpected argument: {}",
                            inspection_text(extra)
                        )?,
                        (Some(name), None) => {
                            print_inspection(mode, name, session.inspect(name), &mut output)?
                        }
                    }
                    continue;
                }
                Some(SESSION_RESET_COMMAND) => {
                    if let Some(extra) = parts.next() {
                        writeln!(output, "error: unexpected argument: {extra}")?;
                    } else {
                        match session.reset() {
                            Ok(()) => writeln!(output, "session reset")?,
                            Err(error) => writeln!(output, "error: {error}")?,
                        }
                    }
                    continue;
                }
                _ => {}
            }
        }
        match ShellCommand::parse(command) {
            Ok(
                command @ (ShellCommand::Inspect
                | ShellCommand::InspectOperation(_)
                | ShellCommand::InspectConfirm(_)
                | ShellCommand::InspectCancel
                | ShellCommand::InspectResult { .. }),
            ) => {
                #[cfg(feature = "tools")]
                inspections.run(command, app, scratches, &mut output)?;
                #[cfg(not(feature = "tools"))]
                {
                    let _ = command;
                    writeln!(
                        output,
                        "error: semantic inspection requires feature `tools` (enabled by default)"
                    )?;
                }
            }
            Ok(ShellCommand::Host {
                operation,
                selection,
            }) => {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    if !line.starts_with(":scope ") {
                        return Err("host command requires explicit :scope namespace".into());
                    }
                    let callback = host_command
                        .as_deref_mut()
                        .ok_or("no read-only host command installed")?;
                    let document = app.open_document().ok_or("no open document")?;
                    document.buffer().span_for_range(selection)?;
                    callback.invoke(document, selection, &operation, &mut output)
                })();
                if let Err(error) = result {
                    writeln!(output, "error: {error}")?;
                }
            }
            Ok(ShellCommand::ScriptEvidence) => {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    if !line.starts_with(":scope ") {
                        return Err("script command requires explicit :scope namespace".into());
                    }
                    host_command
                        .as_deref()
                        .ok_or("no read-only host command installed")?
                        .inspect_script_evidence(&mut output)
                })();
                if let Err(error) = result {
                    writeln!(output, "error: {error}")?;
                }
            }
            Ok(ShellCommand::ScriptRun) => {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    if !line.starts_with(":scope ") {
                        return Err("script command requires explicit :scope namespace".into());
                    }
                    let callback = host_command
                        .as_deref_mut()
                        .ok_or("no read-only host command installed")?;
                    if script_context.is_active() {
                        callback.invoke_script_context(
                            scratches.selected()?,
                            scratches.semantic_target()?,
                            &mut output,
                        )
                    } else {
                        callback.invoke_script_with_target(
                            scratches.selected()?,
                            scratches.semantic_target()?,
                            &mut output,
                        )
                    }
                })();
                if let Err(error) = result {
                    writeln!(output, "error: {error}")?;
                }
            }
            Ok(ShellCommand::Run {
                operation,
                selection,
            }) => {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    if !line.starts_with(":scope ") {
                        return Err("run command requires explicit :scope namespace".into());
                    }
                    let callback = host_command
                        .as_deref_mut()
                        .ok_or("no read-only host command installed")?;
                    let source = scratches.selected()?;
                    let document = app.open_document().ok_or("no open document")?;
                    document.buffer().span_for_range(selection)?;
                    callback.invoke_source(source, document, selection, &operation, &mut output)
                })();
                if let Err(error) = result {
                    writeln!(output, "error: {error}")?;
                }
            }
            #[cfg(feature = "agent")]
            Ok(ShellCommand::Source(source_proposals::Command::AgentEdit(request))) => {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    let document_only = agent_config.launcher_binding()
                        || host_command
                            .as_deref()
                            .is_some_and(|host| !host.allow_app_agent_fallback());
                    let request = if document_only {
                        if !source_proposals.requires_evaluation() {
                            return Err("selected-document proposals require explicit --allow-edit and a host editing contract".into());
                        }
                        format!("@buffer:{} {request}", scratches.selected()?.id())
                    } else {
                        request
                    };
                    // Resolve authorization before configuring or invoking a model.
                    source_proposals.validate_request(&request, scratches)?;
                    print_buffer_target(scratches.selected()?, &mut output)?;
                    let mut agent = CellAgent::new(agent_config.clone())?;
                    source_proposals.generate(&mut agent, &request, scratches, &mut output)
                })();
                if let Err(error) = result {
                    writeln!(output, "error: {error}; source unchanged")?;
                }
            }
            Ok(ShellCommand::Source(command)) => {
                if let Err(error) = source_proposals.run(command, scratches, &mut output) {
                    writeln!(output, "error: {error}")?;
                }
            }
            Ok(ShellCommand::Proposal { paths }) => {
                if proposal.is_some() {
                    writeln!(
                        output,
                        "error: accept or discard the current proposal first"
                    )?;
                } else {
                    match app.proposal(&paths, crate::workspace::ProposalLimits::default()) {
                        Ok(snapshot) => {
                            proposal = Some(snapshot);
                            writeln!(
                                output,
                                "isolated workspace-path proposal created; baseline unchanged"
                            )?;
                        }
                        Err(error) => writeln!(output, "error: {error}")?,
                    }
                }
            }
            Ok(ShellCommand::ProposalReview) => match proposal.as_ref() {
                Some(snapshot) => match snapshot.changes() {
                    Ok(changes) => {
                        writeln!(
                            output,
                            "{} changed files (workspace-path proposal snapshot; full before/after text)",
                            changes.len()
                        )?;
                        for change in changes {
                            writeln!(
                                output,
                                "--- {}\n{}\n+++ {}\n{}",
                                change.path,
                                change.before.as_deref().unwrap_or("[absent]"),
                                change.path,
                                change.after
                            )?;
                        }
                    }
                    Err(error) => writeln!(output, "error: {error}")?,
                },
                None => writeln!(output, "error: no proposal")?,
            },
            Ok(ShellCommand::ProposalDiscard) => {
                if let Some(mut snapshot) = proposal.take() {
                    snapshot.discard();
                }
                writeln!(output, "proposal discarded; baseline unchanged")?;
            }
            Ok(ShellCommand::ProposalAccept) => match proposal.as_mut() {
                Some(snapshot) => {
                    let result = app.accept_proposal(snapshot);
                    writeln!(output, "applied paths: {:?}", result.applied)?;
                    if let Some(error) = result.error {
                        writeln!(
                            output,
                            "acceptance failed (may be partially applied): {error}"
                        )?;
                    } else {
                        proposal = None;
                        app.refresh_files()?;
                        writeln!(
                            output,
                            "accepted; reopen any open file to see accepted text"
                        )?;
                    }
                }
                None => writeln!(output, "error: no proposal")?,
            },
            #[cfg(not(feature = "agent"))]
            Ok(ShellCommand::ProposalAgent { .. }) => {
                writeln!(output, "error: {AGENT_FEATURE_REQUIRED}")?
            }
            #[cfg(feature = "agent")]
            Ok(ShellCommand::ProposalAgent { .. })
                if agent_config.launcher_binding()
                    || host_command
                        .as_deref()
                        .is_some_and(|host| !host.allow_app_agent_fallback()) =>
            {
                writeln!(
                    output,
                    "error: document-only agent cannot use workspace-path proposals; use agent edit with explicit editing authority"
                )?;
            }
            #[cfg(feature = "agent")]
            Ok(ShellCommand::ProposalAgent { request }) => match proposal.as_mut() {
                Some(snapshot) => match CellAgent::new(agent_config.clone()) {
                    Ok(agent) => match agent.run_proposal(
                        snapshot,
                        &request,
                        crate::agent::AgentLimits::default(),
                    ) {
                        Ok(run) => writeln!(
                            output,
                            "Workspace-path proposal agent: {:?}: {} (isolated snapshot; baseline unchanged; review then accept/discard)",
                            run.stop, run.content
                        )?,
                        Err(error) => {
                            writeln!(output, "error: {error}; proposal retained for review")?
                        }
                    },
                    Err(error) => writeln!(output, "error: {error}")?,
                },
                None => writeln!(output, "error: create a proposal with approved paths first")?,
            },
            Ok(ShellCommand::Context) => {
                print_source_context(app, scratches, session, &mut output)?
            }
            Ok(ShellCommand::Languages) => print_languages(app.languages(), session, &mut output)?,
            Ok(ShellCommand::Help) => print_shell_help(&mut output)?,
            Ok(ShellCommand::Files) => print_files(app, &mut output)?,
            Ok(ShellCommand::Capabilities) => print_capabilities(app.workspace(), &mut output)?,
            Ok(ShellCommand::Authority) => print_authority(app.workspace(), &mut output)?,
            Ok(ShellCommand::Open { path }) => match app.open_path(&path) {
                Ok(document) => {
                    writeln!(output, "opened {}", document.path().display())?;
                    print_app_target(app, &mut output)?;
                }
                Err(error) => writeln!(output, "error: {error}")?,
            },
            Ok(ShellCommand::OpenSelected) => match app.open_selected() {
                Ok(document) => {
                    writeln!(output, "opened {}", document.path().display())?;
                    print_app_target(app, &mut output)?;
                }
                Err(error) => writeln!(output, "error: {error}")?,
            },
            Ok(ShellCommand::Show) => match app.open_document() {
                Some(document) => {
                    print_app_target(app, &mut output)?;
                    write!(output, "{}", document.text())?;
                }
                None => writeln!(output, "error: no document is open")?,
            },
            Ok(ShellCommand::Lines { start, end }) => print_lines(app, start, end, &mut output)?,
            Ok(ShellCommand::Replace { edit }) => match app.open_document_mut() {
                Some(document) => match document.apply_edit(edit) {
                    Ok(()) => writeln!(output, "replaced")?,
                    Err(error) => writeln!(output, "error: {error}")?,
                },
                None => writeln!(output, "error: no document is open")?,
            },
            Ok(ShellCommand::Language) => print_language(app, &mut output)?,
            Ok(ShellCommand::Outline) => print_outline(app, &mut output)?,
            Ok(ShellCommand::Diagnostics) => print_diagnostics(app, &mut output)?,
            Ok(ShellCommand::Dirty) => match app.open_document() {
                Some(document) => writeln!(
                    output,
                    "{} {}",
                    document.path().display(),
                    if document.is_dirty() {
                        "dirty"
                    } else {
                        "clean"
                    }
                )?,
                None => writeln!(output, "error: no document is open")?,
            },
            Ok(ShellCommand::Save) => match app.save_open_document() {
                Ok(()) => {
                    writeln!(output, "saved")?;
                    print_app_target(app, &mut output)?;
                }
                Err(error) => writeln!(output, "error: {error}")?,
            },
            Ok(ShellCommand::Revert) => match app.revert_open_document() {
                Ok(()) => writeln!(output, "reverted")?,
                Err(error) => writeln!(output, "error: {error}")?,
            },
            Ok(ShellCommand::Refresh) => match app.refresh_files() {
                Ok(()) => writeln!(output, "refreshed {} files", app.files().len())?,
                Err(error) => writeln!(output, "error: {error}")?,
            },
            #[cfg(not(feature = "agent"))]
            Ok(ShellCommand::Agent { .. }) => {
                writeln!(output, "error: {AGENT_FEATURE_REQUIRED}")?;
            }
            #[cfg(feature = "agent")]
            Ok(ShellCommand::Agent { request }) => {
                let binding = host_command
                    .as_deref_mut()
                    .and_then(|host| host.semantic_agent());
                if let Some(binding) = binding {
                    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                        let source = if agent_config.launcher_binding() {
                            scratches.selected()?
                        } else {
                            scratches
                                .semantic_target()?
                                .ok_or("semantic agent requires a bound semantic target")?
                        };
                        // Validate before constructing or invoking even the default model.
                        if source.language_id() != binding.service.language_id() {
                            return Err(
                                "semantic agent target language does not match service".into()
                            );
                        }
                        let mut default_model;
                        let model = match binding.model {
                            Some(model) => model,
                            None => {
                                default_model = CellAgent::new(agent_config.clone())?;
                                &mut default_model
                            }
                        };
                        let outcome = run_retained_scratch_agent(
                            source,
                            binding.service,
                            model,
                            &request,
                            binding.limits,
                            RetainedScratchAgentConfig {
                                allowed_operation_ids: binding.allowed_operation_ids,
                                authorize: binding.authorize,
                                trace: binding.trace,
                            },
                        )?;
                        (binding.observe)(&outcome);
                        writeln!(
                            output,
                            "Semantic agent: {:?}: {}",
                            outcome.run.stop, outcome.run.content
                        )?;
                        Ok(())
                    })();
                    if let Err(error) = result {
                        writeln!(output, "error: {error}")?;
                    }
                    continue;
                }
                if agent_config.launcher_binding()
                    || host_command
                        .as_deref()
                        .is_some_and(|host| !host.allow_app_agent_fallback())
                {
                    writeln!(
                        output,
                        "error: selected-document agent requires a configured semantic host and explicit agent opt-in; no App fallback"
                    )?;
                    continue;
                }
                match CellAgent::new(agent_config.clone()) {
                    Ok(agent) => {
                        print_app_target(app, &mut output)?;
                        match run_cli_agent(&agent, app, Some(session), &request) {
                            Ok(response) => writeln!(output, "{}", response.content)?,
                            Err(error) => writeln!(output, "error: {error}")?,
                        }
                    }
                    Err(error) => writeln!(output, "error: {error}")?,
                }
            }
            Ok(ShellCommand::Quit) => break,
            Err(error) => writeln!(output, "error: {error}")?,
        }
    }

    Ok(())
}

fn print_evaluation(
    result: scope_language::EvaluationResult,
    output: &mut impl Write,
) -> std::io::Result<()> {
    for text in result.output {
        write!(output, "{text}")?;
        if !text.ends_with('\n') {
            writeln!(output)?;
        }
    }
    for diagnostic in result.diagnostics {
        if let Some(location) = diagnostic.locations.first() {
            writeln!(
                output,
                "{} [attributed source revision] {}:{}: {}",
                diagnostic_label(&diagnostic),
                location.range.start.line + 1,
                location.range.start.column + 1,
                diagnostic.message
            )?;
            continue;
        }
        writeln!(
            output,
            "{} {}:{}: {}",
            diagnostic_label(&diagnostic),
            diagnostic.range.start.line + 1,
            diagnostic.range.start.column + 1,
            diagnostic.message
        )?;
    }
    match result.status {
        scope_language::EvaluationStatus::Success => {}
        scope_language::EvaluationStatus::Failed => writeln!(output, "evaluation failed")?,
        scope_language::EvaluationStatus::Cancelled { requires_reset } => {
            writeln!(
                output,
                "evaluation cancelled{}",
                if requires_reset {
                    "; session reset required"
                } else {
                    ""
                }
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod source_command_tests {
    use super::*;
    use crate::{document::Document, scratch::ScratchBuffer, test_support::TempWorkspace};

    #[derive(Default)]
    struct Host {
        calls: Vec<String>,
    }
    impl ReadOnlyCommand for Host {
        fn invoke(
            &mut self,
            document: &Document,
            selection: TextRange,
            operation: &str,
            output: &mut dyn Write,
        ) -> Result<(), Box<dyn std::error::Error>> {
            assert_eq!(document.buffer().as_str(), "target");
            assert_eq!(
                selection,
                TextRange::new(Position::new(0, 0), Position::new(0, 6))
            );
            self.calls.push(format!("host:{operation}"));
            writeln!(output, "host invoked")?;
            Ok(())
        }
        fn invoke_source(
            &mut self,
            source: &ScratchBuffer,
            document: &Document,
            selection: TextRange,
            operation: &str,
            output: &mut dyn Write,
        ) -> Result<(), Box<dyn std::error::Error>> {
            assert_eq!(source.text(), "éx");
            assert_eq!(source.language_id(), "text");
            assert_eq!(document.buffer().as_str(), "target");
            assert_eq!(
                selection,
                TextRange::new(Position::new(0, 0), Position::new(0, 2))
            );
            self.calls.push(format!("run:{operation}"));
            writeln!(output, "source invoked")?;
            Ok(())
        }
    }

    #[test]
    fn continuous_script_uses_existing_execution_dispatch() {
        struct Runner(Vec<String>);
        impl ReadOnlyCommand for Runner {
            fn supports_direct_script(&self) -> bool {
                true
            }
            fn admit_script_snapshot(&self, source: &str) -> ScriptSnapshotAdmission {
                if source.contains("print") {
                    ScriptSnapshotAdmission::ExplicitRunRequired
                } else {
                    ScriptSnapshotAdmission::CompletePure
                }
            }
            fn invoke(
                &mut self,
                _: &Document,
                _: TextRange,
                _: &str,
                _: &mut dyn Write,
            ) -> Result<(), Box<dyn std::error::Error>> {
                panic!("must use ScriptRun");
            }
            fn invoke_script(
                &mut self,
                source: &ScratchBuffer,
                _: &mut dyn Write,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.0.push(source.text().to_owned());
                Ok(())
            }
        }
        let root = TempWorkspace::new("continuous-script-routing");
        let mut host = Runner(Vec::new());
        let mut output = Vec::new();
        run_with_session_and_command(
            App::new(root.workspace()).unwrap(),
            InteractiveSession::new(),
            &mut host,
            std::io::Cursor::new(
                ":scope script enter\n1\nprint(2)\n3\n:run\n:scope script leave\nquit\n",
            ),
            &mut output,
        )
        .unwrap();
        assert_eq!(host.0, ["1", "1;\nprint(2);\n3"]);
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("Source updated. Run required (:run, full snapshot).")
        );
    }

    #[test]
    fn script_routes_without_document_or_session_and_denies_by_default() {
        struct ScriptHost(Vec<String>);
        impl ReadOnlyCommand for ScriptHost {
            fn invoke(
                &mut self,
                _: &Document,
                _: TextRange,
                _: &str,
                _: &mut dyn Write,
            ) -> Result<(), Box<dyn std::error::Error>> {
                panic!("script must not use document invocation")
            }
            fn invoke_script(
                &mut self,
                source: &ScratchBuffer,
                output: &mut dyn Write,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.0.push(source.text().to_owned());
                writeln!(output, "script invoked")?;
                Ok(())
            }
        }
        let root = TempWorkspace::new("script-command-routing");
        let mut host = ScriptHost(Vec::new());
        let mut output = Vec::new();
        run_with_session_and_command(
            App::new(root.workspace()).unwrap(), InteractiveSession::new(), &mut host,
            std::io::Cursor::new(":scope script run\n:scope scratch new text\n:scope scratch replace 1 1 1 1 éx\nscript run\n:scope script run\nquit\n"), &mut output,
        ).unwrap();
        assert_eq!(host.0, ["éx"]);
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("script command requires explicit :scope namespace"));
        assert!(output.contains("script invoked"));
        let mut scratches = ScratchBuffers::default();
        scratches.create("text", "source");
        assert!(
            Host::default()
                .invoke_script(scratches.selected().unwrap(), &mut Vec::new())
                .is_err()
        );
    }

    #[test]
    fn semantic_target_resolution_never_falls_back_after_close() {
        struct TargetHost(Vec<Option<u64>>);
        impl ReadOnlyCommand for TargetHost {
            fn invoke(
                &mut self,
                _: &Document,
                _: TextRange,
                _: &str,
                _: &mut dyn Write,
            ) -> Result<(), Box<dyn std::error::Error>> {
                panic!("must not invoke App target")
            }
            fn invoke_script_with_target(
                &mut self,
                source: &ScratchBuffer,
                target: Option<&ScratchBuffer>,
                _: &mut dyn Write,
            ) -> Result<(), Box<dyn std::error::Error>> {
                assert_eq!(source.id(), 1);
                self.0.push(target.map(ScratchBuffer::id));
                Ok(())
            }
        }
        let root = TempWorkspace::new("semantic-target-close");
        let mut host = TargetHost(Vec::new());
        let mut output = Vec::new();
        run_with_session_and_command(
            App::new(root.workspace()).unwrap(),
            InteractiveSession::new(),
            &mut host,
            std::io::Cursor::new(concat!(
                ":scope semantic-target 99\n",
                ":scope scratch new text\n",
                ":scope semantic-target 0\n",
                ":scope scratch new scope-script\n",
                ":scope semantic-target 99\n",
                ":scope script run\n",
                ":scope scratch select 0\n",
                ":scope discard\n",
                ":scope script run\n",
                ":scope semantic-target\n",
                ":scope semantic-target none\n",
                ":scope script run\nquit\n",
            )),
            &mut output,
        )
        .unwrap();
        assert_eq!(host.0, [Some(0), None]);
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Semantic target: scratch 0"), "{output}");
        assert!(output.contains("error:"), "{output}");
    }

    #[test]
    fn script_parser_and_discovery() {
        assert!(matches!(
            ShellCommand::parse("script run"),
            Ok(ShellCommand::ScriptRun)
        ));
        for input in ["script", "script other", "script run extra"] {
            assert!(ShellCommand::parse(input).is_err());
        }
        assert!(
            complete_commands(":scope scri", 11, false)
                .unwrap()
                .iter()
                .any(|item| item.insert_text == "script")
        );
        assert!(
            !complete_commands("scri", 4, false)
                .unwrap()
                .iter()
                .any(|item| item.insert_text == "script")
        );
        let mut output = Vec::new();
        print_shell_help(&mut output).unwrap();
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains(":scope script run")
        );
    }

    #[test]
    fn source_command_routes_selected_scratch_and_preserves_target() {
        let root = TempWorkspace::new("source-command-routing");
        root.write_file("target.txt", "target");
        let mut host = Host::default();
        let mut output = Vec::new();
        run_with_session_and_command(
            App::new(root.workspace()).unwrap(),
            InteractiveSession::new(),
            &mut host,
            std::io::Cursor::new(concat!(
                ":scope run inspect --selection 0:0..0:2\n",
                "open target.txt\n",
                ":scope run inspect --selection 0:0..0:2\n",
                ":scope scratch new text\n",
                ":scope scratch replace 1 1 1 1 unused\n",
                ":scope scratch new text\n",
                ":scope scratch replace 1 1 1 1 éx\n",
                ":scope run inspect --selection 0:0..0:2\n",
                ":scope run inspect --selection 0:9..0:10\n",
                ":scope run inspect --selection 0:2..0:0\n",
                ":scope run inspect --selection 0:0..0:7\n",
                "run inspect --selection 0:0..0:2\n",
                ":scope host old --selection 0:0..0:6\n",
                "show\ndirty\nquit\n"
            )),
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(host.calls, ["run:inspect", "host:old"]);
        assert_eq!(output.matches("error:").count(), 6, "{output}");
        assert!(output.contains("no scratch buffer selected"));
        assert!(output.contains("explicit :scope namespace"));
        assert_eq!(root.read_file("target.txt"), "target");
    }

    #[test]
    fn source_command_defaults_to_denial_for_existing_hosts() {
        struct Legacy;
        impl ReadOnlyCommand for Legacy {
            fn invoke(
                &mut self,
                _: &Document,
                _: TextRange,
                _: &str,
                _: &mut dyn Write,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }
        }
        let mut legacy = Legacy;
        assert!(!legacy.supports_direct_script());
        let mut buffers = crate::scratch::ScratchBuffers::default();
        buffers.create("scope-script", "42");
        assert!(
            legacy
                .invoke_script_direct(buffers.selected().unwrap())
                .unwrap_err()
                .to_string()
                .contains("unsupported")
        );
        let root = TempWorkspace::new("source-command-default");
        root.write_file("target.txt", "target");
        let mut output = Vec::new();
        run_with_session_and_command(
            App::new(root.workspace()).unwrap(), InteractiveSession::new(), &mut Legacy,
            std::io::Cursor::new("open target.txt\n:scope scratch new text\n:scope run inspect --selection 0:0..0:0\nquit\n"), &mut output,
        ).unwrap();
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("source operations are unsupported")
        );
    }

    #[test]
    fn source_command_parser_and_completion_are_explicit() {
        assert!(matches!(
            ShellCommand::parse("run inspect --selection 0:0..0:2"),
            Ok(ShellCommand::Run { .. })
        ));
        for command in [
            "run",
            "run inspect",
            "run inspect --selection bad",
            "run inspect --selection 0:0..0:2 extra",
        ] {
            assert!(ShellCommand::parse(command).is_err());
        }
        assert!(
            complete_commands(":scope ru", 9, false)
                .unwrap()
                .iter()
                .any(|item| item.insert_text == "run")
        );
        assert!(
            !complete_commands("ru", 2, false)
                .unwrap()
                .iter()
                .any(|item| item.insert_text == "run")
        );
    }
}

#[cfg(test)]
mod line_tests {
    use super::*;

    #[test]
    fn line_preserves_terminators_unicode_and_final_empty_line() {
        for (text, command, expected) in [
            ("é🙂\nnext\n", r#"1 "雪  \"""#, "雪  \"\nnext\n"),
            ("first\r\nlast\r\n", "1 \"new\"", "new\r\nlast\r\n"),
            ("first\r\nlast", "2 \"é🙂\"", "first\r\né🙂"),
            ("first\n", "2 \"last\"", "first\nlast"),
            ("first\r\n", "2 \"last\"", "first\r\nlast"),
            ("first\nlast", "2 \"\"", "first\n"),
            ("\r\n", "1 \"\"", "\r\n"),
            ("", "1 \"new\"", "new"),
            ("last", "1 \"\"", ""),
            ("a\nb\r\nc\n", "2 \"x\"", "a\nx\r\nc\n"),
        ] {
            let mut buffers = ScratchBuffers::default();
            buffers.create("test", text);
            let version = buffers.selected().unwrap().version();
            replace_scratch_line(command, &mut buffers).unwrap();
            assert_eq!(buffers.selected().unwrap().text(), expected, "{command}");
            assert_eq!(buffers.selected().unwrap().version(), version + 1);
        }
    }

    #[test]
    fn line_rejects_invalid_input_without_mutation() {
        let mut buffers = ScratchBuffers::default();
        assert!(replace_scratch_line("1 \"x\"", &mut buffers).is_err());
        buffers.create("test", "é🙂\r\n");
        let version = buffers.selected().unwrap().version();
        let dirty = buffers.selected().unwrap().is_dirty();
        for command in [
            "",
            "1",
            "0 \"x\"",
            "-1 \"x\"",
            "x \"x\"",
            "3 \"x\"",
            "999999999999999999999999999 \"x\"",
            "1 unquoted",
            "1 null",
            "1 42",
            "1 []",
            "1 \"unterminated",
            "1 \"x\" extra",
            "1 \"bad\\q\"",
            "1 \"a\\nb\"",
            "1 \"a\\rb\"",
            "1 \"\\u000a\"",
            "1 \"\\u000d\"",
            "1 \"a\nb\"",
            "1 \"a\rb\"",
        ] {
            assert!(
                replace_scratch_line(command, &mut buffers).is_err(),
                "{command:?}"
            );
            assert_eq!(buffers.selected().unwrap().text(), "é🙂\r\n");
            assert_eq!(buffers.selected().unwrap().version(), version);
            assert_eq!(buffers.selected().unwrap().is_dirty(), dirty);
        }
    }

    #[test]
    fn line_completion_is_explicit_and_help_describes_semantics() {
        for active in [false, true] {
            let source = ":scope li";
            let completions = complete_commands(source, source.len(), active).unwrap();
            assert!(completions.iter().any(|item| item.label == "line"));
        }
        assert!(
            !complete_commands("li", 2, false)
                .unwrap()
                .iter()
                .any(|item| item.label == "line")
        );
        assert!(complete_commands("li", 2, true).is_none());
        let mut help = Vec::new();
        print_shell_help(&mut help).unwrap();
        let help = String::from_utf8(help).unwrap();
        assert!(help.contains(":scope line N JSON_STRING"));
        assert!(help.contains("final empty line"));
    }
}

fn replace_scratch_line(
    arguments: &str,
    buffers: &mut ScratchBuffers,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::text::{Span, TextBuffer};

    let arguments = arguments.trim_start();
    let (number, json) = arguments
        .split_once(char::is_whitespace)
        .ok_or("line requires N JSON_STRING")?;
    let number = number
        .parse::<usize>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or("line number must be a positive integer")?;
    let replacement: String = serde_json::from_str(json.trim())?;
    if replacement.contains(['\r', '\n']) {
        return Err("line replacement must not contain CR or LF".into());
    }
    let buffer = buffers.selected_mut()?;
    let text = buffer.text();
    // split, unlike lines, retains the editable empty line after a final LF.
    let mut start = 0;
    let mut lines = text.split('\n');
    for _ in 1..number {
        let line = lines.next().ok_or("line number is out of range")?;
        start += line.len() + 1;
    }
    let line = lines.next().ok_or("line number is out of range")?;
    let mut end = start + line.len();
    if end < text.len() && line.ends_with('\r') {
        end -= 1;
    }
    let range = TextBuffer::new(text).range_for_span(Span::new(start, end))?;
    buffer.apply_edit(TextEdit::replace(range, replacement))?;
    Ok(())
}

fn run_scratch_command(
    rest: &str,
    buffers: &mut ScratchBuffers,
    session: &mut InteractiveSession,
    last_submission: Option<&(String, String)>,
    workspace: &Workspace,
    registry: &LanguageRegistry,
    output: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let rest = rest.trim_start();
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let operation = &rest[..end];
    let arguments = &rest[end..];
    if operation == "replace" {
        let ShellCommand::Replace { edit } = parse_shell_replace(arguments)? else {
            unreachable!()
        };
        buffers.selected_mut()?.apply_edit(edit)?;
        writeln!(output, "scratch replaced")?;
        return Ok(());
    }
    let arity = match operation {
        "new" | "open" | "select" | "destination" | "promote" | "save-as" => 1,
        "list" | "show" | "capture" | "eval" | "save" | "discard" | "close" | "reload"
        | "reload-discard" => 0,
        "" => return Err("scratch requires an operation; use :scope help".into()),
        _ => return Err(format!("unknown scratch operation: {operation}; use :scope help").into()),
    };
    let args: Vec<_> = arguments.split_whitespace().collect();
    if args.len() != arity {
        return Err(format!("scratch {operation} requires exactly {arity} argument(s)").into());
    }
    match operation {
        "new" => {
            let id = buffers.create(args[0], "");
            writeln!(output, "scratch {id} created ({})", args[0])?;
        }
        "open" => {
            let id = buffers.open(workspace, registry, args[0])?;
            writeln!(output, "scratch {id} opened: {}", args[0])?;
            print_buffer_target(buffers.selected()?, output)?;
        }
        "list" => {
            let selected = buffers.selected().ok().map(|buffer| buffer.id());
            for buffer in buffers.list() {
                writeln!(
                    output,
                    "{} scratch {} language={} {} version={} destination={}",
                    if selected == Some(buffer.id()) {
                        "*"
                    } else {
                        " "
                    },
                    buffer.id(),
                    buffer.language_id(),
                    if buffer.is_dirty() { "dirty" } else { "clean" },
                    buffer.version(),
                    buffer
                        .destination_path()
                        .map_or("[none]", ScopePath::as_str)
                )?;
            }
        }
        "select" => {
            let id = args[0]
                .parse::<u64>()
                .map_err(|_| "scratch ID must be a nonnegative integer")?;
            buffers.select(id)?;
            writeln!(output, "scratch {id} selected")?;
        }
        "show" => {
            print_buffer_target(buffers.selected()?, output)?;
            write!(output, "{}", buffers.selected()?.text())?;
        }
        "capture" => {
            let (language, source) = last_submission.ok_or("no REPL submission to capture")?;
            let id = buffers.create(language.clone(), source.clone());
            writeln!(output, "scratch {id} captured ({language})")?;
        }
        "eval" => {
            let buffer = buffers.selected()?;
            let result = buffer.evaluate(session)?;
            print_buffer_target(buffer, output)?;
            print_evaluation(result, output)?;
        }
        "destination" => {
            buffers
                .selected_mut()?
                .bind_destination(workspace, args[0])?;
            writeln!(
                output,
                "scratch destination bound: {} (baseline read; no write)",
                args[0]
            )?;
        }
        "save" => {
            buffers.selected_mut()?.save(workspace)?;
            writeln!(output, "scratch saved")?;
            print_buffer_target(buffers.selected()?, output)?;
        }
        "promote" => {
            buffers.selected_mut()?.promote(workspace, args[0])?;
            writeln!(output, "scratch promoted: {}", args[0])?;
            print_buffer_target(buffers.selected()?, output)?;
        }
        "save-as" => {
            buffers.save_as_selected(workspace, args[0])?;
            writeln!(output, "scratch saved as: {}", args[0])?;
            print_buffer_target(buffers.selected()?, output)?;
        }
        "close" => {
            buffers.close_selected(false)?;
            writeln!(output, "scratch closed (no write)")?;
        }
        "reload" | "reload-discard" => {
            buffers
                .selected_mut()?
                .reload(workspace, operation == "reload-discard")?;
            writeln!(output, "scratch reloaded (no write)")?;
        }
        "discard" => {
            buffers.discard_selected()?;
            writeln!(output, "scratch discarded (no write)")?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn print_buffer(
    buffers: &ScratchBuffers,
    output: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = buffers.selected()?;
    print_buffer_target(buffer, output)?;
    writeln!(
        output,
        "buffer {} {} {}",
        buffer.id(),
        buffer
            .destination_path()
            .map_or("[scratch]", ScopePath::as_str),
        if buffer.is_dirty() { "dirty" } else { "clean" }
    )?;
    for (index, line) in buffer.text().split('\n').take(40).enumerate() {
        let preview: String = line
            .chars()
            .take(160)
            .flat_map(char::escape_debug)
            .collect();
        writeln!(
            output,
            "{:>5} | {}{}",
            index + 1,
            preview,
            if line.chars().count() > 160 {
                " …"
            } else {
                ""
            }
        )?;
    }
    writeln!(
        output,
        "Preview limited to 40 lines / 160 characters; :scope edit --commands then :show START for more."
    )?;
    Ok(())
}

fn print_capabilities(workspace: &Workspace, output: &mut impl Write) -> std::io::Result<()> {
    for interface in workspace.capabilities() {
        writeln!(output, "{interface}")?;
    }
    Ok(())
}

fn print_authority(workspace: &Workspace, output: &mut impl Write) -> std::io::Result<()> {
    writeln!(
        output,
        "Advisory authority metadata only; not an authorization check or a guarantee of operation success."
    )?;
    for descriptor in workspace.capability_descriptors() {
        writeln!(output, "interface: {}", descriptor.interface)?;
        match &descriptor.authority {
            cell::AuthorityDescription::Unspecified => {
                writeln!(output, "  authority: unspecified (not a denial or grant)")?;
            }
            cell::AuthorityDescription::Filesystem { roots } => {
                writeln!(output, "  authority: filesystem")?;
                for root in roots {
                    writeln!(output, "    logical root: {:?}", root.name)?;
                    writeln!(output, "      read policy: {:?}", root.read_policy)?;
                    writeln!(output, "      write policy: {:?}", root.write_policy)?;
                    writeln!(output, "      permitted operations: {:?}", root.operations)?;
                }
            }
            _ => writeln!(output, "  authority: unsupported metadata kind")?,
        }
    }
    Ok(())
}

fn print_files(app: &App, output: &mut impl Write) -> std::io::Result<()> {
    for (index, entry) in app.files().iter().enumerate() {
        let marker = if Some(index) == app.selected_index() {
            ">"
        } else {
            " "
        };
        writeln!(output, "{marker} {}", entry.path.display())?;
    }

    Ok(())
}

fn print_lines(
    app: &App,
    start: usize,
    end: usize,
    output: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(document) = app.open_document() else {
        writeln!(output, "error: no document is open")?;
        return Ok(());
    };

    if start == 0 || end == 0 || start > end {
        writeln!(output, "error: line range must be one-based and increasing")?;
        return Ok(());
    }

    for (index, line) in document.text().lines().enumerate() {
        let line_number = index + 1;
        if line_number >= start && line_number <= end {
            writeln!(output, "{line_number:>4}  {line}")?;
        }
    }

    Ok(())
}

fn print_language(app: &App, output: &mut impl Write) -> std::io::Result<()> {
    let Some(document) = app.open_document() else {
        writeln!(output, "error: no document is open")?;
        return Ok(());
    };

    match app.open_language() {
        Some(language) => writeln!(
            output,
            "{}: {} ({})",
            document.path().display(),
            language.language_name(),
            language.language_id()
        ),
        None => writeln!(output, "{}: no language service", document.path().display()),
    }
}

fn print_outline(app: &App, output: &mut impl Write) -> std::io::Result<()> {
    match app.open_outline() {
        Ok(symbols) if symbols.is_empty() => writeln!(output, "outline: no symbols"),
        Ok(symbols) => {
            for symbol in symbols {
                writeln!(
                    output,
                    "{} {} {}:{}-{}:{}",
                    symbol.kind,
                    symbol.name,
                    symbol.range.start.line + 1,
                    symbol.range.start.column + 1,
                    symbol.range.end.line + 1,
                    symbol.range.end.column + 1
                )?;
            }
            Ok(())
        }
        Err(error) => writeln!(output, "error: {error}"),
    }
}

fn print_diagnostics(app: &App, output: &mut impl Write) -> std::io::Result<()> {
    match app.open_diagnostics() {
        Ok(diagnostics) if diagnostics.is_empty() => writeln!(output, "diagnostics: clean"),
        Ok(diagnostics) => {
            for diagnostic in diagnostics {
                writeln!(
                    output,
                    "{}:{}:{}: {}: {}",
                    diagnostic.range.start.line + 1,
                    diagnostic.range.start.column + 1,
                    diagnostic.range.end.column + 1,
                    diagnostic_label(&diagnostic),
                    diagnostic.message
                )?;
            }
            Ok(())
        }
        Err(error) => writeln!(output, "error: {error}"),
    }
}

fn diagnostic_label(diagnostic: &crate::language::Diagnostic) -> String {
    match (&diagnostic.source, &diagnostic.code) {
        (Some(source), Some(code)) => format!("{}[{code}]: {}", source, diagnostic.severity),
        (Some(source), None) => format!("{source}: {}", diagnostic.severity),
        (None, Some(code)) => format!("{}[{code}]", diagnostic.severity),
        (None, None) => diagnostic.severity.to_string(),
    }
}

fn print_languages(
    registry: &LanguageRegistry,
    session: &InteractiveSession,
    output: &mut impl Write,
) -> std::io::Result<()> {
    writeln!(output, "Analysis services (not evaluators):")?;
    if registry.services().is_empty() {
        writeln!(output, "  none registered")?;
    }
    for service in registry.services() {
        writeln!(
            output,
            "  {} — {}",
            inspection_text(service.language_id()),
            inspection_text(service.language_name())
        )?;
        let operations = service.semantic_operations();
        if operations.is_empty() {
            writeln!(output, "    semantic operations: none installed")?;
        } else {
            for operation in operations {
                writeln!(
                    output,
                    "    semantic operation: {} (availability/eligibility: :scope inspect)",
                    inspection_text(&operation.id)
                )?;
            }
        }
    }
    writeln!(
        output,
        "Fallback analysis: scope.neutral — Language Neutral (unmatched files; no semantic operations)"
    )?;
    writeln!(output, "Interactive evaluators:")?;
    if registry.interactive_providers().is_empty() {
        writeln!(output, "  none registered")?;
    }
    for provider in registry.interactive_providers() {
        writeln!(output, "  {}", inspection_text(provider.language_id()))?;
    }
    writeln!(
        output,
        "Active session: {}",
        inspection_text(session.language_id().unwrap_or("none"))
    )?;
    writeln!(
        output,
        "Select: :scope session <id>; context: :scope context. Registration is not authorization."
    )
}

fn print_shell_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(
        output,
        "Discovery: languages (or :scope languages) lists analysis services, installed semantic operations, evaluators and current session. Use :scope context for source owners; :scope inspect for eligible document reads. Scratch edits stay in memory; :scope script run executes selected scratch. Continuous Script: :scope script enter / leave; source appends to selected scope-script scratch, pure snapshots run automatically, effects require :run; :cancel drops pending input; :edit reuses the editor. Standalone: cargo run -p scope-script -- --help."
    )?;
    writeln!(
        output,
        "Downstream host commands (explicit namespace only):\n  :scope downstream                    List registered namespaces\n  :scope downstream NAME COMMAND ARGS   Dispatch optional immutable live semantic target\nArguments are whitespace-delimited only; quotes and backslashes are rejected. No expansion is performed."
    )?;
    writeln!(
        output,
        "Interactive sessions:\n  :scope session <id|none> Select a registered evaluator or close it\n  :scope session          Show the active interactive language\n  :scope session-reset    Reset the active session\n  :scope describe <symbol> Show symbol metadata\n  :scope effect <symbol>   Show the symbol signature (including effects)\n  :scope where <symbol>    Show origin, location, and definition\nSymbol inspection requires an active session and never evaluates source. These controls require :scope even without a session.\nWith a session active, use :scope <command> for every Scope command; other input is source.\nEnter submits complete input or continues incomplete source. Ctrl+J inserts a newline; Ctrl+S submits invalid (not incomplete) source; Ctrl+C abandons input. Ctrl+P/N recall history. Ctrl+K deletes from the cursor to the current line's end (preserving the newline). In-flight terminal cancellation is unsupported."
    )?;
    writeln!(
        output,
        "Read-only host commands (explicit namespace required):\n  :scope script run                              Selected scratch execution (host opt-in; no App target/session)\n  :scope semantic-target [ID|none]                Bind/report independent live scratch; none clears (no authority grant)\n  :scope host NAME --selection LINE:BYTE..LINE:BYTE  Open App document\n  :scope run NAME --selection LINE:BYTE..LINE:BYTE   Selected scratch source, separate open App target\nCoordinates are zero-based lines/UTF-8 byte columns, half-open ranges. Source operations require host opt-in.\n\nSource context:\n  :scope context    Show both source owners, identities, versions, services and command targets\n  context           Same command without an active language session\nBare open/show/save use App documents; :scope open/show/save use separate shell buffers. Context never changes selection or renews authorization."
    )?;
    writeln!(
        output,
        "Semantic inspection (App-owned document, not the selected shell buffer):\n  :scope inspect                    Discover eligible semantic reads\n  :scope inspect operation <id>     Capture a read request; does not execute (ID may be a JSON string)\n  :scope inspect confirm <token>    Authorize exactly that captured request once\n  :scope inspect cancel             Drop the pending capture\n  :scope inspect result [--full]    Last result summary or lossless native details\nWithout a language session, inspect also works without :scope. App source is opened by bare open <path> (first use :scope session none if active); :scope open selects a separate shell buffer. Only argument-free document Read operations are supported. Read is not an OS sandbox guarantee. Confirmation grants no agent permission."
    )?;
    writeln!(
        output,
        "Workspace buffers (explicit namespace, including in language sessions):\n  :scope open <path> | buffers | buffer <id> | show | edit [--commands]\n  :scope line N JSON_STRING Replace one whole line in the selected shell buffer (1-based)\n                     Preserves LF/CRLF; a trailing newline includes a final empty line.\n                     JSON string only; no CR/LF in replacement. Never saves or evaluates.\n  :scope buffer eval      Evaluate current in-memory source in the matching active session\n  :scope save | save-as <path> | reload | close\n  :scope reload-discard | discard (explicitly lose unsaved contents)\nTTY editing: arrows navigate, Home/End move within a line, Enter inserts a newline; Backspace/Delete edit, bracketed paste preserves layout. Ctrl-S keeps the draft; Ctrl-C/Escape cancels. Nothing saves or evaluates implicitly.\nUse :scope edit --commands for explicit editing (also the default with redirected input): :at LINE COLUMN, :insert TEXT, :delete COUNT, :show [START], :done, :cancel, :help.\nCoordinates are one-based Unicode scalars. Literal lines insert at the cursor with a newline; escapes in :insert are \\n, \\t, \\\\. Editing is a draft: :done keeps changes in memory, :cancel/EOF drops only the draft. Save separately.\n\nScratch buffers (explicit :scope namespace only):\n  :scope scratch new <language-id>\n  :scope scratch open <path> Open and select an existing workspace file\n  :scope scratch list | select <id> | show\n  :scope scratch replace <start-line> <start-column> <end-line> <end-column> <text>\n                     Same coordinates and escapes as replace below\n  :scope scratch capture   Copy the last exact REPL submission and language\n  :scope scratch eval      Evaluate in the matching active session; never switches\n  :scope scratch destination <path> Read an existing file baseline without writing\n  :scope scratch save      Checked save to the bound destination\n  :scope scratch promote <path> Checked replacement of an existing file\n  :scope scratch save-as <path> Checked replacement or exclusive new-file creation\n  :scope scratch close     Close selected buffer only if clean; never saves\n  :scope scratch reload    Reload the destination only if clean\n  :scope scratch reload-discard Explicitly discard edits and reload\n  :scope scratch discard   Drop selected buffer, including unsaved edits, without writing\nBuffers are local to this shell; nothing is persisted implicitly. Binding and promotion require existing files. Save As can exclusively create a file in an existing authorized parent (Unix; parent must not have hostile writers; no fsync guarantee). Replacement is atomic; revision checks are best effort, not atomic compare-and-swap."
    )?;
    writeln!(
        output,
        "Source proposals (selected scratch/workspace buffer):\n  :scope agent edit \"<instruction>\" Propose edits (feature agent); unevaluated by default, contracted only with explicit host configuration\n  source-propose <JSON> JSON edits and optional string rationale\n  source-proposals     List pending source proposals\n  source-proposal <id> Validate and show proposal details\n  source-diff <id>     Validate and show bounded, terminal-escaped diff\n  source-accept <id>   Revalidate and apply in memory; never save or execute\n  source-reject <id>   Reject without changing the buffer\nJSON: {{edits:[{{range:{{start:{{line,column}},end:{{line,column}}}},replacement:string}}],rationale?:string}}; quote all JSON keys. Coordinates are zero-based lines/UTF-8 byte columns, half-open ranges. With an active session prefix commands with :scope.\n\nProposal commands:\n  proposal <paths...> Exact host-approved snapshot paths (including new files)\n  proposal-agent <request> Bounded isolated create/replace; no baseline writes\n  proposal-review     List changed files with full before/after text\n  proposal-accept     Human-only checked sequential apply (not atomic)\n  proposal-discard    Drop snapshot without baseline writes"
    )?;
    writeln!(
        output,
        "Commands:\n  capabilities       List installed workspace interfaces (host metadata)\n  authority          Show advisory workspace authority metadata\n  files              List workspace files\n  open <path>        Open a workspace-relative file\n  open               Open the currently selected file\n  show               Print the open document\n  lines <start> <end> Print one-based inclusive lines from the open document\n  replace <start-line> <start-column> <end-line> <end-column> <text>\n                     Replace a half-open range in memory (1-based lines/byte columns)\n                     Unquoted text; omitted text deletes; trailing spaces retained\n                     Escapes: \\n, \\t, \\\\; save writes changes to disk\n  language           Show the detected language for the open document\n  outline            Show symbols from the open document\n  diagnostics        Show language diagnostics for the open document\n  dirty              Show whether the open document has unsaved changes\n  save               Save the open document to the workspace\n  revert             Revert the open document to its last saved text\n  refresh            Refresh the workspace file list\n  agent <request>    Ask the Cell-backed agent using only Scope context (requires feature `agent`)\n  help               Show this help\n  quit               Exit the shell"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ShellCommand {
    Inspect,
    InspectOperation(String),
    InspectConfirm(u64),
    InspectCancel,
    InspectResult {
        full: bool,
    },
    ScriptRun,
    ScriptEvidence,
    Host {
        operation: String,
        selection: TextRange,
    },
    Run {
        operation: String,
        selection: TextRange,
    },
    Help,
    Languages,
    Context,
    Source(source_proposals::Command),
    Proposal {
        paths: Vec<ScopePath>,
    },
    ProposalReview,
    ProposalAccept,
    ProposalDiscard,
    ProposalAgent {
        request: String,
    },
    Capabilities,
    Authority,
    Files,
    Open {
        path: ScopePath,
    },
    OpenSelected,
    Show,
    Lines {
        start: usize,
        end: usize,
    },
    Replace {
        edit: TextEdit,
    },
    Language,
    Outline,
    Diagnostics,
    Dirty,
    Save,
    Revert,
    Refresh,
    Agent {
        request: String,
    },
    Quit,
}

#[cfg(test)]
#[test]
fn shell_proposal_lifecycle_requires_explicit_commands() {
    use crate::test_support::TempWorkspace;
    let fixture = TempWorkspace::new("shell-proposal");
    fixture.write_file("a", "user edits");
    let app = App::new(fixture.workspace()).unwrap();
    #[cfg(feature = "agent")]
    let config = crate::agent::AgentConfig::local("unused", "http://localhost:1234/v1");
    #[cfg(not(feature = "agent"))]
    let config = AgentConfig;
    let mut output = Vec::new();
    run_shell(app, config, std::io::Cursor::new(
        "proposal a new\nproposal-review\nproposal-discard\nproposal a\nproposal-accept\nquit\n"
    ), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("baseline unchanged"));
    assert!(output.contains("0 changed files"));
    assert!(output.contains("proposal discarded"));
    assert!(output.contains("accepted;"));
    assert_eq!(fixture.read_file("a"), "user edits");
    assert!(!fixture.host_path_exists("new"));
}

// The parser and completion share one declaration, including command aliases.
macro_rules! shell_commands {
    ($line:ident, $trimmed:ident, $parts:ident, $command:ident; $( $($name:literal)|+ => $body:expr ),* $(,)?) => {
        impl ShellCommand {
            fn names() -> &'static [&'static str] {
                &[$($($name),+),*]
            }
            fn parse($line: &str) -> Result<Self, ShellError> {
                let $trimmed = $line.trim();
                let mut $parts = $trimmed.split_whitespace();
                let $command = $parts.next().ok_or(ShellError::EmptyCommand)?;
                match $command {
                    $($($name)|+ => $body,)*
                    _ => Err(ShellError::UnknownCommand($command.to_owned())),
                }
            }
        }
    };
}

shell_commands! { line, trimmed, parts, command;
            "inspect" => {
                let usage = || ShellError::UnexpectedArgument(
                    "usage: inspect [operation ID | confirm TOKEN | cancel | result [--full]]".into());
                let result = match parts.next() {
                    None => Self::Inspect,
                    Some("operation") => {
                        let rest = trimmed[command.len()..].trim_start()["operation".len()..].trim();
                        let id = if rest.starts_with('"') {
                            serde_json::from_str::<String>(rest).map_err(|_| usage())?
                        } else {
                            let id = parts.next().ok_or_else(usage)?.to_owned();
                            if parts.next().is_some() { return Err(usage()); }
                            id
                        };
                        return Ok(Self::InspectOperation(id));
                    },
                    Some("confirm") => Self::InspectConfirm(parts.next().ok_or_else(usage)?.parse().map_err(|_| usage())?),
                    Some("cancel") => Self::InspectCancel,
                    Some("result") => Self::InspectResult { full: match parts.next() {
                        None => false,
                        Some("--full") => true,
                        _ => return Err(usage()),
                    } },
                    _ => return Err(usage()),
                };
                if parts.next().is_some() { return Err(usage()); }
                Ok(result)
            },
            "script" => {
                let command = match parts.next() {
                    Some("run") => Self::ScriptRun,
                    Some("evidence") => Self::ScriptEvidence,
                    _ => return Err(ShellError::UnexpectedArgument("usage: :scope script run | evidence".into())),
                };
                if parts.next().is_some() {
                    return Err(ShellError::UnexpectedArgument("usage: :scope script run | evidence".into()));
                }
                Ok(command)
            },
            "host" | "run" => {
                let usage = || ShellError::UnexpectedArgument(
                    format!("usage: :scope {command} NAME --selection LINE:BYTE..LINE:BYTE (zero-based)"));
                let operation = parts.next().ok_or_else(usage)?.to_owned();
                if parts.next() != Some("--selection") { return Err(usage()); }
                let coordinates = parts.next().ok_or_else(usage)?;
                reject_shell_extra(parts)?;
                let (start, end) = coordinates.split_once("..").ok_or_else(usage)?;
                let position = |value: &str| -> Result<Position, ShellError> {
                    let (line, column) = value.split_once(':').ok_or_else(usage)?;
                    Ok(Position::new(line.parse().map_err(|_| usage())?, column.parse().map_err(|_| usage())?))
                };
                let selection = TextRange::new(position(start)?, position(end)?);
                Ok(if command == "run" {
                    Self::Run { operation, selection }
                } else {
                    Self::Host { operation, selection }
                })
            },
            "source-propose" | "source-proposals" | "source-proposal" | "source-diff" | "source-accept" | "source-reject" => {
                source_proposals::Command::parse(command, &trimmed[command.len()..])
                    .map(Self::Source).map_err(ShellError::UnexpectedArgument)
            },
            "proposal" => {
                let paths = parts
                    .map(|path| ScopePath::new(path).map_err(ShellError::InvalidPath))
                    .collect::<Result<Vec<_>, _>>()?;
                if paths.is_empty() {
                    return Err(ShellError::MissingAgentRequest);
                }
                Ok(Self::Proposal { paths })
            },
            "proposal-review" => reject_shell_extra(parts).map(|()| Self::ProposalReview),
            "proposal-accept" => reject_shell_extra(parts).map(|()| Self::ProposalAccept),
            "proposal-discard" => reject_shell_extra(parts).map(|()| Self::ProposalDiscard),
            "proposal-agent" => {
                let request = trimmed[command.len()..].trim();
                if request.is_empty() {
                    return Err(ShellError::MissingAgentRequest);
                }
                Ok(Self::ProposalAgent {
                    request: request.into(),
                })
            },
            "languages" => reject_shell_extra(parts).map(|()| Self::Languages),
            "context" => reject_shell_extra(parts).map(|()| Self::Context),
            "help" | "?" => reject_shell_extra(parts).map(|()| Self::Help),
            "files" | "ls" => reject_shell_extra(parts).map(|()| Self::Files),
            "capabilities" => reject_shell_extra(parts).map(|()| Self::Capabilities),
            "authority" => reject_shell_extra(parts).map(|()| Self::Authority),
            "open" => match parts.next() {
                Some(path) => {
                    reject_shell_extra(parts)?;
                    Ok(Self::Open {
                        path: ScopePath::new(path).map_err(ShellError::InvalidPath)?,
                    })
                }
                None => Ok(Self::OpenSelected),
            },
            "show" | "cat" => reject_shell_extra(parts).map(|()| Self::Show),
            "lines" => {
                let start = parts.next().ok_or(ShellError::MissingLineStart)?;
                let end = parts.next().ok_or(ShellError::MissingLineEnd)?;
                reject_shell_extra(parts)?;
                Ok(Self::Lines {
                    start: start.parse().map_err(|_| ShellError::InvalidLineNumber)?,
                    end: end.parse().map_err(|_| ShellError::InvalidLineNumber)?,
                })
            },
            "replace" => parse_shell_replace(&line.trim_start()[command.len()..]),
            "language" | "lang" => reject_shell_extra(parts).map(|()| Self::Language),
            "outline" | "symbols" => reject_shell_extra(parts).map(|()| Self::Outline),
            "diagnostics" | "diag" => reject_shell_extra(parts).map(|()| Self::Diagnostics),
            "dirty" => reject_shell_extra(parts).map(|()| Self::Dirty),
            "save" | "write" | "w" => reject_shell_extra(parts).map(|()| Self::Save),
            "revert" => reject_shell_extra(parts).map(|()| Self::Revert),
            "refresh" => reject_shell_extra(parts).map(|()| Self::Refresh),
            "agent" | "ask" => {
                let request = trimmed[command.len()..].trim();
                if request.is_empty() {
                    Err(ShellError::MissingAgentRequest)
                } else if request == "edit" || request.starts_with("edit ") {
                    let instruction = request[4..].trim();
                    let instruction: String = serde_json::from_str(instruction)
                        .map_err(|_| ShellError::UnexpectedArgument("agent edit requires one JSON-quoted instruction string; targets the selected :scope buffer".into()))?;
                    if instruction.trim().is_empty() {
                        return Err(ShellError::MissingAgentRequest);
                    }
                    Ok(Self::Source(source_proposals::Command::AgentEdit(instruction)))
                } else {
                    Ok(Self::Agent {
                        request: request.to_string(),
                    })
                }
            },
            "quit" | "exit" => reject_shell_extra(parts).map(|()| Self::Quit),
}

/// `Some`, including an empty list, identifies Scope command context.
pub(crate) fn complete_commands(
    source: &str,
    cursor: usize,
    active: bool,
) -> Option<Vec<crate::language::CompletionItem>> {
    use crate::language::CompletionItem;
    if !source.is_char_boundary(cursor) {
        return Some(Vec::new());
    }
    if cursor == source.len() && !source.is_empty() && ":scope".starts_with(source) {
        return Some(vec![CompletionItem {
            label: ":scope".into(),
            replacement: 0..cursor,
            insert_text: ":scope ".into(),
            detail: None,
        }]);
    }
    let explicit = source.starts_with(":scope ");
    if !explicit && active {
        return None;
    }
    let offset = if explicit { 6 } else { 0 };
    if cursor < offset {
        return Some(Vec::new());
    }
    let rest = &source[offset..];
    let start = offset + rest.len() - rest.trim_start().len();
    let tokens: Vec<_> = source[start..].split_whitespace().collect();
    if tokens.first() == Some(&"inspect") && source[start..].contains(char::is_whitespace) {
        let (argument_start, names): (usize, &[&str]) = if tokens.get(1) == Some(&"result")
            && source[start..]
                .trim_start_matches("inspect")
                .trim_start()
                .starts_with("result ")
        {
            let after_inspect = start + "inspect".len();
            let result_start = after_inspect + source[after_inspect..].len()
                - source[after_inspect..].trim_start().len();
            (result_start + "result".len(), &["--full"])
        } else {
            (
                start + "inspect".len(),
                &["operation", "confirm", "cancel", "result"],
            )
        };
        let argument_start = argument_start + source[argument_start..].len()
            - source[argument_start..].trim_start().len();
        let end = source[argument_start..]
            .find(char::is_whitespace)
            .map_or(source.len(), |i| argument_start + i);
        if cursor < argument_start || cursor > end {
            return Some(Vec::new());
        }
        let prefix = &source[argument_start..cursor];
        return Some(
            names
                .iter()
                .filter(|name| name.starts_with(prefix))
                .map(|name| CompletionItem {
                    label: (*name).into(),
                    replacement: argument_start..end,
                    insert_text: (*name).into(),
                    detail: None,
                })
                .collect(),
        );
    }
    let end = source[start..]
        .find(char::is_whitespace)
        .map_or(source.len(), |i| start + i);
    if cursor < start || cursor > end {
        return Some(Vec::new());
    }
    let prefix = &source[start..cursor];
    let mut names = ShellCommand::names().to_vec();
    // Preserve the existing scratch completion priority for the shared `scr` prefix.
    names.retain(|name| *name != "script");
    if !explicit {
        names.retain(|name| !matches!(*name, "host" | "run" | "script"));
    }
    if explicit {
        names.extend_from_slice(&[
            "scratch",
            "buffers",
            "buffer",
            "edit",
            "line",
            "save-as",
            "reload",
            "reload-discard",
            "close",
            "discard",
        ]);
        names.push("script");
        names.push(SESSION_COMMAND);
        names.extend_from_slice(INSPECTION_COMMANDS);
        if active {
            names.push(SESSION_RESET_COMMAND);
        }
    }
    Some(
        names
            .into_iter()
            .filter(|name| {
                name.starts_with(prefix)
                    && (cfg!(feature = "agent")
                        || !matches!(*name, "agent" | "ask" | "proposal-agent"))
            })
            .map(|name| CompletionItem {
                label: name.into(),
                replacement: start..end,
                insert_text: name.into(),
                detail: None,
            })
            .collect(),
    )
}

#[cfg(test)]
#[test]
fn completion_uses_parser_names_and_namespace_context() {
    let complete = |s: &str, active| complete_commands(s, s.len(), active).unwrap();
    assert_eq!(complete("hel", false)[0].insert_text, "help");
    assert_eq!(complete("ins", false)[0].insert_text, "inspect");
    assert_eq!(
        complete(":scope inspect op", true)[0].insert_text,
        "operation"
    );
    assert_eq!(complete("inspect con", false)[0].insert_text, "confirm");
    assert_eq!(
        complete(":scope inspect result --f", true)[0].insert_text,
        "--full"
    );
    assert!(complete(":scope inspect operation foo", true).is_empty());
    assert!(complete_commands("inspect con", 11, true).is_none());
    for active in [false, true] {
        let items = complete(":scope hos", active);
        assert_eq!(items[0].insert_text, "host");
        assert_eq!(items[0].replacement, 7..10);
        assert!(complete(":scope host inspect", active).is_empty());
    }
    assert!(complete("hos", false).is_empty());
    assert!(complete_commands("hos", 3, true).is_none());
    assert_eq!(complete("sym", false)[0].insert_text, "symbols");
    assert!(complete_commands("help", 4, true).is_none());
    assert!(complete(":scope unknown", true).is_empty());
    assert!(complete("open arg", false).is_empty());
    assert!(complete("session", false).is_empty());
    assert_eq!(complete(":s", true)[0].insert_text, ":scope ");
    assert_eq!(complete(":scope", false)[0].insert_text, ":scope ");
    assert_eq!(
        complete(":scope session-r", true)[0].insert_text,
        "session-reset"
    );
    assert!(complete(":scope session-r", false).is_empty());
    let item = &complete_commands("  helZZ arg", 5, false).unwrap()[0];
    assert_eq!(item.replacement, 2..7);
    let all = complete("", false);
    for name in ShellCommand::names() {
        if matches!(*name, "host" | "run" | "script")
            || (!cfg!(feature = "agent") && matches!(*name, "agent" | "ask" | "proposal-agent"))
        {
            continue;
        }
        assert!(all.iter().any(|item| item.label == *name));
        assert!(!matches!(
            ShellCommand::parse(name),
            Err(ShellError::UnknownCommand(_))
        ));
    }
}

const INSPECTION_COMMANDS: &[&str] = &["describe", "effect", "where"];

// Bound each provider-controlled field and escape controls before terminal output.
fn inspection_text(text: &str) -> String {
    inspection_text_limit(text, 2048)
}

fn inspection_text_limit(text: &str, limit: usize) -> String {
    let mut rendered = String::new();
    for character in text.chars() {
        let escaped = if character.is_control()
            || matches!(character, '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            character.escape_default().to_string()
        } else {
            character.to_string()
        };
        if rendered.len() + escaped.len() > limit {
            rendered.push('…');
            break;
        }
        rendered.push_str(&escaped);
    }
    rendered
}

fn print_inspection(
    mode: &str,
    name: &str,
    result: Result<Option<scope_language::SymbolDescription>, scope_language::InspectionError>,
    output: &mut impl Write,
) -> std::io::Result<()> {
    let description = match result {
        Ok(Some(description)) => description,
        Ok(None) => return writeln!(output, "unknown symbol: {}", inspection_text(name)),
        Err(error) => return writeln!(output, "error: {}", inspection_text(&error.to_string())),
    };
    writeln!(output, "symbol: {}", inspection_text(&description.name))?;
    if mode == "describe" {
        writeln!(
            output,
            "kind: {}",
            inspection_text(&description.kind.to_string())
        )?;
    }
    if mode != "where" {
        writeln!(
            output,
            "signature: {}",
            inspection_text(description.signature.as_deref().unwrap_or("[unavailable]"))
        )?;
    }
    if mode == "describe" {
        writeln!(
            output,
            "documentation: {}",
            inspection_text(
                description
                    .documentation
                    .as_deref()
                    .unwrap_or("[unavailable]")
            )
        )?;
    }
    if mode != "effect" {
        let origin = match description.origin {
            Some(scope_language::SymbolOrigin::BuiltIn) => "built-in",
            Some(scope_language::SymbolOrigin::Workspace) => "workspace",
            Some(scope_language::SymbolOrigin::Interactive) => "interactive",
            None => "[unavailable]",
        };
        writeln!(output, "origin: {origin}")?;
        match description.location {
            Some(location) => writeln!(
                output,
                "location: {}:{}:{}-{}:{}",
                inspection_text(&location.path.to_string()),
                location.range.start.line.saturating_add(1),
                location.range.start.column.saturating_add(1),
                location.range.end.line.saturating_add(1),
                location.range.end.column.saturating_add(1)
            )?,
            None => writeln!(output, "location: [unavailable]")?,
        }
        let definition = description.definition.as_deref().unwrap_or("[unavailable]");
        let rendered =
            inspection_text_limit(definition, if mode == "describe" { 160 } else { 2048 });
        writeln!(output, "definition: {rendered}")?;
        if mode == "describe" && rendered != inspection_text(definition) {
            writeln!(
                output,
                "Definition preview; use :scope where <symbol> for more (also bounded)."
            )?;
        }
    }
    Ok(())
}

const SESSION_COMMAND: &str = "session";
const SESSION_RESET_COMMAND: &str = "session-reset";

#[derive(Debug, Clone, PartialEq, Eq)]
enum ShellError {
    EmptyCommand,
    InvalidPath(cell_guest::InvalidPath),
    InvalidLineNumber,
    MissingReplaceCoordinate,
    InvalidReplaceCoordinate,
    InvalidReplacementEscape(Option<char>),
    MissingLineEnd,
    MissingAgentRequest,
    MissingLineStart,
    UnexpectedArgument(String),
    UnknownCommand(String),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyCommand => write!(formatter, "empty command"),
            Self::InvalidPath(source) => write!(formatter, "invalid path: {source}"),
            Self::InvalidLineNumber => write!(formatter, "line numbers must be positive integers"),
            Self::MissingReplaceCoordinate => write!(
                formatter,
                "replace requires start-line start-column end-line end-column"
            ),
            Self::InvalidReplaceCoordinate => {
                write!(formatter, "replace coordinates must be positive integers")
            }
            Self::InvalidReplacementEscape(Some(character)) => write!(
                formatter,
                "unknown replacement escape: \\{character}; supported escapes are \\n, \\t, \\\\"
            ),
            Self::InvalidReplacementEscape(None) => write!(
                formatter,
                "incomplete replacement escape: trailing backslash"
            ),
            Self::MissingLineEnd => write!(formatter, "missing ending line number"),
            Self::MissingAgentRequest => write!(formatter, "missing request after `agent`"),
            Self::MissingLineStart => write!(formatter, "missing starting line number"),
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "unexpected argument: {argument}")
            }
            Self::UnknownCommand(command) => write!(formatter, "unknown command: {command}"),
        }
    }
}

impl std::error::Error for ShellError {}

fn parse_shell_replace(mut rest: &str) -> Result<ShellCommand, ShellError> {
    let mut coordinates = [0; 4];
    for coordinate in &mut coordinates {
        rest = rest.trim_start();
        if rest.is_empty() {
            return Err(ShellError::MissingReplaceCoordinate);
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        *coordinate = rest[..end]
            .parse::<usize>()
            .ok()
            .and_then(|value| value.checked_sub(1))
            .ok_or(ShellError::InvalidReplaceCoordinate)?;
        rest = &rest[end..];
    }
    // Consume exactly one separator so replacement whitespace remains literal.
    if let Some(separator) = rest.chars().next() {
        rest = &rest[separator.len_utf8()..];
    }
    let mut replacement = String::new();
    let mut characters = rest.chars();
    while let Some(character) = characters.next() {
        replacement.push(if character == '\\' {
            match characters.next() {
                Some('n') => '\n',
                Some('t') => '\t',
                Some('\\') => '\\',
                other => return Err(ShellError::InvalidReplacementEscape(other)),
            }
        } else {
            character
        });
    }
    Ok(ShellCommand::Replace {
        edit: TextEdit::replace(
            TextRange::new(
                Position::new(coordinates[0], coordinates[1]),
                Position::new(coordinates[2], coordinates[3]),
            ),
            replacement,
        ),
    })
}

fn reject_shell_extra<'a>(mut parts: impl Iterator<Item = &'a str>) -> Result<(), ShellError> {
    if let Some(argument) = parts.next() {
        return Err(ShellError::UnexpectedArgument(argument.to_owned()));
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Args {
    workspace_root: String,
    writable: bool,
    command: Command,
    agent_config: AgentConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    List,
    Meta {
        options: SnapshotOptions,
    },
    Capabilities,
    Authority,
    Show {
        path: ScopePath,
    },
    Shell,
    #[cfg(feature = "agent")]
    Agent {
        request: String,
        open_path: Option<ScopePath>,
    },
    #[cfg(feature = "tui")]
    Tui,
    #[cfg(feature = "tui")]
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParseResult {
    Help,
    Run(Args),
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<ParseResult, CliError> {
        let mut args = args.into_iter().peekable();
        let no_args = args.peek().is_none();
        let mut workspace_root = None;
        let mut writable = false;
        let mut command = None;
        #[cfg(feature = "agent")]
        let mut agent_model = DEFAULT_AGENT_MODEL.to_string();
        #[cfg(feature = "agent")]
        let mut agent_base_url = Some(DEFAULT_AGENT_BASE_URL.to_string());
        #[cfg(feature = "agent")]
        let mut agent_api_key = None;
        #[cfg(feature = "agent")]
        let (mut launcher_binding, mut explicit_model, mut explicit_endpoint) =
            (false, false, false);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--writable" => writable = true,
                "-h" | "--help" => return Ok(ParseResult::Help),
                "-w" | "--workspace" => {
                    let root = args.next().ok_or(CliError::MissingWorkspacePath)?;
                    workspace_root = Some(root);
                }
                #[cfg(feature = "agent")]
                "--agent-local-binding" => launcher_binding = true,
                #[cfg(feature = "agent")]
                "--agent-model" => {
                    explicit_model = true;
                    agent_model = args.next().ok_or(CliError::MissingAgentModel)?;
                }
                #[cfg(feature = "agent")]
                "--agent-base-url" => {
                    explicit_endpoint = true;
                    agent_base_url = Some(args.next().ok_or(CliError::MissingAgentBaseUrl)?);
                }
                #[cfg(feature = "agent")]
                "--agent-openai" => {
                    agent_base_url = None;
                }
                #[cfg(feature = "agent")]
                "--agent-api-key-env" => {
                    let name = args.next().ok_or(CliError::MissingAgentApiKeyEnv)?;
                    agent_api_key = Some(
                        std::env::var(&name)
                            .map_err(|source| CliError::EnvironmentVariable { name, source })?,
                    );
                }
                #[cfg(not(feature = "agent"))]
                "agent"
                | "ask"
                | "--agent-model"
                | "--agent-local-binding"
                | "--agent-base-url"
                | "--agent-openai"
                | "--agent-api-key-env" => return Err(CliError::AgentFeatureRequired),
                "capabilities" => {
                    command = Some(Command::Capabilities);
                    reject_extra_args(args)?;
                    break;
                }
                "authority" => {
                    command = Some(Command::Authority);
                    reject_extra_args(args)?;
                    break;
                }
                "meta" => {
                    let mut options = SnapshotOptions::default();
                    while let Some(flag) = args.next() {
                        let allow_zero = match flag.as_str() {
                            "--max-entries" => false,
                            "--max-depth" => true,
                            "-h" | "--help" => return Ok(ParseResult::Help),
                            _ => return Err(CliError::UnexpectedArgument(flag)),
                        };
                        let value = args.next().ok_or_else(|| {
                            CliError::UnexpectedArgument(format!("missing number after {flag}"))
                        })?;
                        let number = value
                            .parse::<usize>()
                            .ok()
                            .filter(|number| {
                                !value.is_empty()
                                    && value.bytes().all(|byte| byte.is_ascii_digit())
                                    && (allow_zero || *number != 0)
                            })
                            .ok_or_else(|| {
                                CliError::UnexpectedArgument(format!(
                                    "invalid value {value:?} for {flag}"
                                ))
                            })?;
                        if allow_zero {
                            options.max_depth = number;
                        } else {
                            options.max_entries = number;
                        }
                    }
                    command = Some(Command::Meta { options });
                    break;
                }
                "list" => {
                    command = Some(Command::List);
                    reject_extra_args(args)?;
                    break;
                }
                "show" => {
                    let path = args.next().ok_or(CliError::MissingFilePath)?;
                    command = Some(Command::Show {
                        path: ScopePath::new(path).map_err(CliError::InvalidPath)?,
                    });
                    reject_extra_args(args)?;
                    break;
                }
                #[cfg(feature = "agent")]
                "agent" | "ask" => {
                    let mut open_path = None;
                    let mut request_parts = Vec::new();

                    while let Some(part) = args.next() {
                        match part.as_str() {
                            "--open" => {
                                let path = args.next().ok_or(CliError::MissingAgentOpenPath)?;
                                open_path =
                                    Some(ScopePath::new(path).map_err(CliError::InvalidPath)?);
                            }
                            _ => request_parts.push(part),
                        }
                    }

                    if request_parts.is_empty() {
                        return Err(CliError::MissingAgentRequest);
                    }

                    command = Some(Command::Agent {
                        request: request_parts.join(" "),
                        open_path,
                    });
                    break;
                }
                "open" | "shell" => {
                    command = Some(Command::Shell);
                    if let Some(root) = args.next() {
                        if workspace_root.is_some() {
                            return Err(CliError::UnexpectedArgument(root));
                        }
                        workspace_root = Some(root);
                    }
                    reject_extra_args(args)?;
                    break;
                }
                #[cfg(feature = "tui")]
                "tui" | "workspace" => {
                    command = Some(if arg == "workspace" {
                        Command::Workspace
                    } else {
                        Command::Tui
                    });
                    if let Some(root) = args.next() {
                        if workspace_root.is_some() {
                            return Err(CliError::UnexpectedArgument(root));
                        }
                        workspace_root = Some(root);
                    }
                    reject_extra_args(args)?;
                    break;
                }
                _ if workspace_root.is_none() && command.is_none() => {
                    workspace_root = Some(arg);
                }
                _ => return Err(CliError::UnexpectedArgument(arg)),
            }
        }

        let workspace_root = match workspace_root {
            Some(root) => root,
            None => std::env::current_dir()
                .map_err(CliError::CurrentDirectory)?
                .to_string_lossy()
                .into_owned(),
        };

        #[cfg(feature = "agent")]
        let agent_config = match agent_base_url {
            Some(base_url) => AgentConfig::local(agent_model, base_url),
            None => AgentConfig::new(agent_model),
        };
        #[cfg(feature = "agent")]
        let agent_config = match agent_api_key {
            Some(api_key) => agent_config.with_api_key(api_key),
            None => agent_config,
        };

        #[cfg(feature = "agent")]
        let agent_config = if launcher_binding {
            if !explicit_model || !explicit_endpoint {
                return Err(CliError::UnexpectedArgument(
                    "--agent-local-binding requires explicit --agent-model and --agent-base-url"
                        .into(),
                ));
            }
            agent_config
                .enable_launcher_binding()
                .map_err(CliError::UnexpectedArgument)?
        } else {
            agent_config
        };

        #[cfg(not(feature = "agent"))]
        let agent_config = AgentConfig;

        Ok(ParseResult::Run(Args {
            workspace_root,
            writable,
            command: command.unwrap_or(if no_args {
                Command::Shell
            } else {
                Command::List
            }),
            agent_config,
        }))
    }
}

#[derive(Debug)]
#[allow(dead_code)]
enum CliError {
    AgentFeatureRequired,
    CurrentDirectory(std::io::Error),
    EnvironmentVariable {
        name: String,
        source: std::env::VarError,
    },
    InvalidPath(cell_guest::InvalidPath),
    MissingAgentApiKeyEnv,
    MissingAgentBaseUrl,
    MissingAgentModel,
    MissingAgentOpenPath,
    MissingAgentRequest,
    MissingFilePath,
    MissingWorkspacePath,
    UnexpectedArgument(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AgentFeatureRequired => formatter.write_str(AGENT_FEATURE_REQUIRED),
            Self::CurrentDirectory(source) => {
                write!(formatter, "failed to get current directory: {source}")
            }
            Self::EnvironmentVariable { name, source } => {
                write!(
                    formatter,
                    "failed to read environment variable {name}: {source}"
                )
            }
            Self::InvalidPath(source) => write!(formatter, "invalid path: {source}"),
            Self::MissingAgentApiKeyEnv => write!(
                formatter,
                "missing variable name after `--agent-api-key-env`"
            ),
            Self::MissingAgentBaseUrl => write!(formatter, "missing URL after `--agent-base-url`"),
            Self::MissingAgentModel => write!(formatter, "missing model after `--agent-model`"),
            Self::MissingAgentOpenPath => write!(formatter, "missing path after `agent --open`"),
            Self::MissingAgentRequest => write!(formatter, "missing request for `agent` command"),
            Self::MissingFilePath => write!(formatter, "missing file path for `show` command"),
            Self::MissingWorkspacePath => write!(formatter, "missing path after `--workspace`"),
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "unexpected argument: {argument}")
            }
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(source) => Some(source),
            Self::EnvironmentVariable { source, .. } => Some(source),
            Self::AgentFeatureRequired
            | Self::InvalidPath(_)
            | Self::MissingAgentApiKeyEnv
            | Self::MissingAgentBaseUrl
            | Self::MissingAgentModel
            | Self::MissingAgentOpenPath
            | Self::MissingAgentRequest
            | Self::MissingFilePath
            | Self::MissingWorkspacePath
            | Self::UnexpectedArgument(_) => None,
        }
    }
}

#[cfg(all(test, feature = "agent"))]
mod launcher_binding_tests {
    use super::*;

    fn parse(flags: &[&str]) -> Result<ParseResult, CliError> {
        Args::parse(flags.iter().map(|s| s.to_string()))
    }

    #[test]
    fn binding_is_off_by_default_and_requires_explicit_local_configuration() {
        let ParseResult::Run(args) = parse(&["shell"]).unwrap() else {
            panic!()
        };
        assert!(!args.agent_config.launcher_binding());
        for flags in [
            vec!["--agent-local-binding", "shell"],
            vec!["--agent-local-binding", "--agent-model", "mock", "shell"],
            vec![
                "--agent-local-binding",
                "--agent-base-url",
                "http://localhost:1234/v1",
                "shell",
            ],
            vec![
                "--agent-local-binding",
                "--agent-model",
                "mock",
                "--agent-base-url",
                "https://example.com/v1",
                "shell",
            ],
            vec![
                "--agent-local-binding",
                "--agent-model",
                "mock",
                "--agent-base-url",
                "http://localhost.evil/v1",
                "shell",
            ],
            vec![
                "--agent-local-binding",
                "--agent-model",
                "mock",
                "--agent-base-url",
                "http://localhost:1234/v1",
                "--agent-openai",
                "shell",
            ],
        ] {
            assert!(parse(&flags).is_err(), "{flags:?}");
        }
    }

    #[test]
    fn explicit_binding_does_not_grant_workspace_writes_or_contact_backend() {
        for endpoint in [
            "http://localhost:1234/v1",
            "http://127.0.0.1:1/v1",
            "http://[::1]:1234/v1",
        ] {
            let ParseResult::Run(args) = parse(&[
                "--agent-local-binding",
                "--agent-model",
                "mock",
                "--agent-base-url",
                endpoint,
                "shell",
            ])
            .unwrap() else {
                panic!()
            };
            assert!(args.agent_config.launcher_binding());
            assert!(!args.writable);
            assert_eq!(args.agent_config.model(), "mock");
            assert_eq!(args.agent_config.base_url(), Some(endpoint));
        }
    }
}

fn reject_extra_args(mut args: impl Iterator<Item = String>) -> Result<(), CliError> {
    if let Some(argument) = args.next() {
        return Err(CliError::UnexpectedArgument(argument));
    }

    Ok(())
}

fn print_usage() {
    println!(
        "Workspace access defaults to read-only. Pass --writable before the command to opt in to workspace writes. Metadata snapshots remain read-only.\nDefault features include tools; tool availability does not grant write authority."
    );
    let defaults = SnapshotOptions::default();
    println!(
        "Scope runtime: cargo run -- [OPTIONS] [COMMAND]\nUsage: scope-ide [--workspace <path>] [COMMAND]\n       scope-ide <workspace> [COMMAND]\n\nNo arguments: start the shell in the current directory (piped input supported).\nA workspace argument without a command retains the default file listing.\nWorkspaces must already exist; startup does not create them.\n\nCommands:\n  open, shell  Start the Scope shell (also: open <workspace>)\n  list         List workspace files\n  show <path>  Print a workspace-relative file\n  capabilities List installed interfaces (host metadata)\n  authority    Show advisory authority metadata, not authorization\n  meta         Bounded read-only metadata graph\n  agent, ask   Ask the agent (requires feature `agent`)\n\nOptions:\n  -w, --workspace <path>      Workspace root (default: current directory)\n  -h, --help                  Show this help\n  --max-entries N             Metadata entry budget (default: {}; positive)\n  --max-depth N               Metadata depth (default: {}; zero allowed)\n\nShell discovery: help, files, authority, quit.\nDownstream evaluators require host registration. The default workbench installs pure Script via :scope script run; Scope Simple analysis is separate.\nScript CLI: cargo run -p scope-script -- --help (separate no-host runtime).",
        defaults.max_entries, defaults.max_depth
    );
    #[cfg(feature = "agent")]
    println!(
        "Agent: agent [--open <path>] <request>\n  --agent-model <name>        Model (default: {})\n  --agent-base-url <url>      API URL (default: {})\n  --agent-local-binding       Opt in to selected-document host model binding; requires explicit model and loopback base URL; grants no read/edit/filesystem authority\n  --agent-openai              Use OpenAI default API URL\n  --agent-api-key-env <name>  Read API key from environment",
        DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL
    );
    #[cfg(not(feature = "agent"))]
    println!("{AGENT_FEATURE_REQUIRED}");
    #[cfg(feature = "tui")]
    println!(
        "  tui [workspace]            Start the experimental ratatui interface\n  workspace [path]           Scratch editor, F4 commands/output, F2 shell"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempWorkspace;

    #[cfg(not(feature = "agent"))]
    fn default_agent_config() -> AgentConfig {
        AgentConfig
    }

    #[cfg(not(feature = "agent"))]
    #[test]
    fn disabled_agent_commands_and_options_explain_how_to_enable_support() {
        for args in [
            vec!["agent", "explain"],
            vec!["ask", "explain"],
            vec!["--agent-api-key-env", "SCOPE_MISSING_KEY"],
            vec!["--agent-model", "test-model"],
            vec!["--agent-base-url", "http://localhost:1234/v1"],
            vec!["--agent-openai"],
        ] {
            let error = Args::parse(args.into_iter().map(String::from)).unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains("cargo run --locked -p scope-workbench --features agent -- shell")
            );
            assert!(message.contains("before --"));
            assert!(message.contains("scope-ide/agent"));
        }
        let root = TempWorkspace::new("disabled_agent");
        root.write_file("main.scope", "fn main() {}\n");
        let app = App::new(root.workspace()).unwrap();
        let mut output = Vec::new();
        run_shell(
            app,
            AgentConfig,
            &b"agent explain\nask explain\nfiles\nquit\n"[..],
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches(AGENT_FEATURE_REQUIRED).count(), 2);
        assert!(output.contains("main.scope"));
    }

    #[cfg(feature = "agent")]
    fn default_agent_config() -> AgentConfig {
        AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL)
    }

    #[test]
    fn parses_meta_defaults_and_overrides() {
        for (input, options) in [
            (vec!["meta"], SnapshotOptions::default()),
            (
                vec![
                    "--workspace",
                    ".",
                    "meta",
                    "--max-entries",
                    "12",
                    "--max-depth",
                    "0",
                ],
                SnapshotOptions {
                    max_entries: 12,
                    max_depth: 0,
                },
            ),
            (
                vec!["meta", "--max-depth", "3", "--max-entries", "8"],
                SnapshotOptions {
                    max_entries: 8,
                    max_depth: 3,
                },
            ),
        ] {
            let ParseResult::Run(args) = Args::parse(input.into_iter().map(String::from)).unwrap()
            else {
                panic!("expected runnable args");
            };
            assert_eq!(args.command, Command::Meta { options });
        }
    }

    #[test]
    fn rejects_invalid_meta_options() {
        for input in [
            vec!["meta", "--unknown", "1"],
            vec!["meta", "unexpected"],
            vec!["meta", "--max-entries"],
            vec!["meta", "--max-depth"],
            vec!["meta", "--max-entries", "0"],
            vec!["meta", "--max-depth", "-1"],
            vec!["meta", "--max-entries", "bad"],
            vec!["meta", "--max-depth", "1.5"],
            vec!["meta", "--max-depth", "+1"],
            vec!["meta", "--max-entries", "--max-depth", "1"],
            vec![
                "meta",
                "--max-entries",
                "999999999999999999999999999999999999",
            ],
        ] {
            assert!(Args::parse(input.into_iter().map(String::from)).is_err());
        }
    }

    #[test]
    fn meta_output_includes_empty_directories_hierarchy_and_sources() {
        let root = TempWorkspace::new("cli_meta");
        root.create_dir("empty");
        root.write_file("src/main.scope", "fn main() {}\n");
        let workspace = Workspace::read_only(root.root().to_string_lossy().into_owned()).unwrap();
        let mut output = Vec::new();
        print_meta(&workspace, SnapshotOptions::default(), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("graph: scope.workspace:"));
        assert!(output.contains("revision: "));
        assert!(output.contains("nodes: 4 edges: 3"));
        assert!(output.contains("node: \"empty\" kind: scope.directory@1 label: \"empty\""));
        assert!(output.contains("locator: \"project/src/main.scope\""));
        assert!(output.contains("edge: \".\" --scope.contains@1--> \"empty\""));
        assert!(output.contains("edge: \"src\" --scope.contains@1--> \"src/main.scope\""));
        assert!(output.contains("provenance: Observed { provider: \"scope.workspace@1\" }"));
        assert!(!output.contains("not complete"));
        let nodes: Vec<_> = output
            .lines()
            .filter(|line| line.starts_with("node:"))
            .collect();
        assert!(nodes.windows(2).all(|pair| pair[0] < pair[1]));
        let edges: Vec<_> = output
            .lines()
            .filter(|line| line.starts_with("edge:"))
            .collect();
        assert!(edges.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn meta_output_reports_each_depth_truncation() {
        let root = TempWorkspace::new("cli_meta_depth");
        root.create_dir("empty");
        root.write_file("src/main.scope", "hidden\n");
        for (depth, paths) in [(0, vec!["."]), (1, vec!["empty", "src"])] {
            let mut output = Vec::new();
            print_meta(
                &root.workspace(),
                SnapshotOptions {
                    max_depth: depth,
                    ..SnapshotOptions::default()
                },
                &mut output,
            )
            .unwrap();
            let output = String::from_utf8(output).unwrap();
            assert_eq!(output.matches("depth truncation:").count(), paths.len());
            for path in paths {
                assert!(output.contains(&format!(
                    "depth truncation: {path:?} (directory not scanned; max_depth={depth})"
                )));
            }
            assert!(output.contains("snapshot is not complete"));
            assert!(!output.contains("src/main.scope"));
        }
    }

    #[test]
    fn meta_entry_budget_failure_emits_no_partial_output() {
        let root = TempWorkspace::new("cli_meta_budget");
        root.write_file("src/main.scope", "fn main() {}\n");
        let mut output = Vec::new();
        let error = print_meta(
            &root.workspace(),
            SnapshotOptions {
                max_entries: 2,
                ..SnapshotOptions::default()
            },
            &mut output,
        )
        .unwrap_err();
        assert!(error.to_string().contains("entry limit"));
        assert!(output.is_empty());
    }

    #[test]
    fn defaults_to_shell_in_current_directory() {
        let parsed = Args::parse([]).unwrap();

        match parsed {
            ParseResult::Run(args) => {
                assert_eq!(args.command, Command::Shell);
                assert!(std::path::Path::new(&args.workspace_root).is_absolute());
            }
            ParseResult::Help => panic!("expected runnable args"),
        }
    }

    #[test]
    fn parses_capabilities_command() {
        for arguments in [
            vec!["--workspace", "/tmp/project", "capabilities"],
            vec!["/tmp/project", "capabilities"],
        ] {
            assert_eq!(
                Args::parse(arguments.into_iter().map(String::from)).unwrap(),
                ParseResult::Run(Args {
                    writable: false,
                    workspace_root: "/tmp/project".to_string(),
                    command: Command::Capabilities,
                    agent_config: default_agent_config(),
                })
            );
        }

        match Args::parse(["capabilities"].map(String::from)).unwrap() {
            ParseResult::Run(args) => {
                assert_eq!(args.command, Command::Capabilities);
                assert!(std::path::Path::new(&args.workspace_root).is_absolute());
            }
            ParseResult::Help => panic!("expected runnable args"),
        }
        assert!(matches!(
            Args::parse(["capabilities", "extra"].map(String::from)),
            Err(CliError::UnexpectedArgument(argument)) if argument == "extra"
        ));
    }

    #[test]
    fn parses_authority_command() {
        for arguments in [
            vec!["--workspace", "/tmp/project", "authority"],
            vec!["/tmp/project", "authority"],
        ] {
            assert_eq!(
                Args::parse(arguments.into_iter().map(String::from)).unwrap(),
                ParseResult::Run(Args {
                    writable: false,
                    workspace_root: "/tmp/project".to_string(),
                    command: Command::Authority,
                    agent_config: default_agent_config(),
                })
            );
        }
        match Args::parse(["authority"].map(String::from)).unwrap() {
            ParseResult::Run(args) => {
                assert_eq!(args.command, Command::Authority);
                assert!(std::path::Path::new(&args.workspace_root).is_absolute());
            }
            ParseResult::Help => panic!("expected runnable args"),
        }
        for arguments in [
            vec!["authority", "extra"],
            vec!["--workspace", "/tmp/project", "authority", "extra"],
            vec!["/tmp/project", "authority", "extra"],
        ] {
            assert!(matches!(
                Args::parse(arguments.into_iter().map(String::from)),
                Err(CliError::UnexpectedArgument(argument)) if argument == "extra"
            ));
        }
    }

    #[test]
    fn parses_shell_authority_command() {
        assert_eq!(
            ShellCommand::parse("authority"),
            Ok(ShellCommand::Authority)
        );
        assert_eq!(
            ShellCommand::parse("authority extra"),
            Err(ShellError::UnexpectedArgument("extra".to_string()))
        );
    }

    #[test]
    fn authority_reports_policies_without_physical_paths() {
        let root = TempWorkspace::new("authority_policies");
        for writable in [false, true] {
            let workspace = if writable {
                root.workspace()
            } else {
                Workspace::read_only(root.root().to_string_lossy()).unwrap()
            };
            let mut output = Vec::new();
            print_authority(&workspace, &mut output).unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains("Advisory"));
            assert!(output.contains("not an authorization check"));
            assert!(output.contains("read policy: WithinRoot"));
            assert!(output.contains(if writable {
                "write policy: WithinRoot"
            } else {
                "write policy: Denied"
            }));
            assert!(output.contains("permitted operations:"));
            assert!(output.contains("ReadText"));
            assert_eq!(output.contains("WriteText"), writable);
            assert_eq!(output.contains("SaveText"), writable);
            assert!(!output.contains(root.root().to_str().unwrap()));
            assert!(!output.contains(root.root().file_name().unwrap().to_str().unwrap()));
            for descriptor in workspace.capability_descriptors() {
                assert!(output.contains(&format!("interface: {}", descriptor.interface)));
                if let cell::AuthorityDescription::Filesystem { roots } = &descriptor.authority {
                    assert!(!roots.is_empty());
                    for root in roots {
                        assert!(output.contains(&format!("logical root: {:?}", root.name)));
                        assert!(
                            output
                                .contains(&format!("permitted operations: {:?}", root.operations))
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn shell_renders_authority_and_help() {
        let root = TempWorkspace::new("shell_authority");
        let workspace = root.workspace();
        let mut expected = Vec::new();
        print_authority(&workspace, &mut expected).unwrap();
        let app = App::new(workspace).unwrap();
        let mut output = Vec::new();
        run_shell(
            app,
            default_agent_config(),
            &b"authority\nhelp\nquit\n"[..],
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains(&String::from_utf8(expected).unwrap()));
        assert!(output.contains("authority          Show advisory workspace authority metadata"));
    }

    #[test]
    fn parses_shell_capabilities_command() {
        assert_eq!(
            ShellCommand::parse("capabilities").unwrap(),
            ShellCommand::Capabilities
        );
        assert_eq!(
            ShellCommand::parse("capabilities extra"),
            Err(ShellError::UnexpectedArgument("extra".to_string()))
        );
    }

    #[test]
    fn shell_lists_only_workspace_capabilities() {
        let root = TempWorkspace::new("shell_lists_only_workspace_capabilities");
        let app = App::new(root.workspace()).unwrap();
        let interfaces = app.workspace().capabilities();
        assert!(!interfaces.is_empty());
        assert!(
            interfaces
                .iter()
                .all(|interface| !interface.contains("chat"))
        );
        let expected = format!(
            "Scope — {}\nType `help` for commands, `quit` to exit. Workspace files change only on explicit commands.\nDiscover: :scope languages | :scope session | :scope context | :scope inspect\nScratch edits: :scope scratch new <language-id>, then :scope edit (in memory; not evaluation).\nScript: :scope script enter / leave selects continuous context; :scope script run evaluates selected scratch. Standalone: cargo run -p scope-script -- --help\nscope> {}\nscope> ",
            app.workspace().root(),
            interfaces.join("\n")
        );
        let input = b"capabilities\nquit\n";
        let mut output = Vec::new();

        run_shell(app, default_agent_config(), &input[..], &mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }

    #[test]
    fn parses_explicit_workspace_and_show_command() {
        let parsed =
            Args::parse(["--workspace", "/tmp/project", "show", "README.md"].map(String::from))
                .unwrap();

        assert_eq!(
            parsed,
            ParseResult::Run(Args {
                writable: false,
                workspace_root: "/tmp/project".to_string(),
                command: Command::Show {
                    path: ScopePath::new("README.md").unwrap()
                },
                agent_config: default_agent_config(),
            })
        );
    }

    #[test]
    fn parses_open_command_with_explicit_workspace_option() {
        let parsed =
            Args::parse(["--workspace", "/tmp/project", "open"].map(String::from)).unwrap();

        assert_eq!(
            parsed,
            ParseResult::Run(Args {
                writable: false,
                workspace_root: "/tmp/project".to_string(),
                command: Command::Shell,
                agent_config: default_agent_config(),
            })
        );
    }

    #[test]
    fn parses_open_command_with_workspace_argument() {
        let parsed = Args::parse(["open", "/tmp/project"].map(String::from)).unwrap();

        assert_eq!(
            parsed,
            ParseResult::Run(Args {
                writable: false,
                workspace_root: "/tmp/project".to_string(),
                command: Command::Shell,
                agent_config: default_agent_config(),
            })
        );
    }

    #[cfg(feature = "tui")]
    #[test]
    fn parses_tui_command_with_workspace_argument() {
        let parsed = Args::parse(["tui", "/tmp/project"].map(String::from)).unwrap();

        assert_eq!(
            parsed,
            ParseResult::Run(Args {
                writable: false,
                workspace_root: "/tmp/project".to_string(),
                command: Command::Tui,
                agent_config: default_agent_config(),
            })
        );
    }

    #[test]
    fn keeps_positional_workspace_compatible_with_previous_usage() {
        let parsed = Args::parse(["/tmp/project"].map(String::from)).unwrap();

        assert_eq!(
            parsed,
            ParseResult::Run(Args {
                writable: false,
                workspace_root: "/tmp/project".to_string(),
                command: Command::List,
                agent_config: default_agent_config(),
            })
        );
    }

    #[test]
    fn parses_help_without_requiring_other_arguments() {
        let parsed = Args::parse(["--help"].map(String::from)).unwrap();

        assert_eq!(parsed, ParseResult::Help);
    }

    #[test]
    fn parses_shell_lines_command() {
        assert_eq!(
            ShellCommand::parse("lines 2 5").unwrap(),
            ShellCommand::Lines { start: 2, end: 5 }
        );
    }

    #[test]
    fn parses_shell_replace_command() {
        assert_eq!(
            ShellCommand::parse(r"replace 1 2 3 4 a\n\t\\  ").unwrap(),
            ShellCommand::Replace {
                edit: TextEdit::replace(
                    TextRange::new(Position::new(0, 1), Position::new(2, 3)),
                    "a\n\t\\  ",
                ),
            }
        );
        assert_eq!(
            ShellCommand::parse("replace 1 1 1 1").unwrap(),
            ShellCommand::Replace {
                edit: TextEdit::insert(Position::new(0, 0), ""),
            }
        );
        for command in [
            "replace",
            "replace 1 1 1",
            "replace 0 1 1 1 x",
            "replace 1 -1 1 1 x",
            "replace 1 x 1 1 x",
            "replace 999999999999999999999999999 1 1 1 x",
            r"replace 1 1 1 1 \q",
            "replace 1 1 1 1 \\",
        ] {
            assert!(ShellCommand::parse(command).is_err(), "{command}");
        }
        assert!(
            ShellCommand::parse(r"replace 1 1 1 1 \q")
                .unwrap_err()
                .to_string()
                .contains("unknown replacement escape")
        );
    }

    #[test]
    fn shell_line_routes_only_explicitly_to_selected_buffer() {
        let root = TempWorkspace::new("shell_line_routes_only_explicitly_to_selected_buffer");
        root.write_file("README.md", "App original\n");
        let input = b"open README.md\n:scope line 1 \"no selection\"\n:scope scratch new test\n:scope line 1 \"first\"\n:scope scratch new test\n:scope line 1 \"second\"\n:scope scratch show\n:scope scratch select 0\nline 1 \"bare must fail\"\n:scope scratch show\nshow\nquit\n";
        let mut output = Vec::new();
        run_shell(
            App::new(root.workspace()).unwrap(),
            default_agent_config(),
            &input[..],
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output
                .matches("scratch line replaced (no save or evaluation)")
                .count(),
            2
        );
        assert_eq!(output.matches("error:").count(), 2);
        assert!(output.contains("first"));
        assert!(output.contains("second"));
        assert!(output.contains("App original\n"));
        assert_eq!(root.read_file("README.md"), "App original\n");
    }

    #[test]
    fn shell_line_same_path_keeps_app_and_disk_unchanged() {
        let root = TempWorkspace::new("shell_line_same_path_keeps_app_and_disk_unchanged");
        root.write_file("README.md", "original README\n");
        let input = b"open README.md\n:scope open README.md\n:scope show\n:scope line 1 \"edited buffer\"\n:scope show\nshow\ndirty\nquit\n";
        let mut output = Vec::new();
        run_shell(
            App::new(root.workspace()).unwrap(),
            default_agent_config(),
            &input[..],
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("error:"), "{output}");
        let (before, after) = output
            .split_once("scratch line replaced (no save or evaluation)")
            .unwrap();
        assert!(
            before.contains("Shell buffer target: Scratch(0) @0"),
            "{output}"
        );
        assert!(
            before.contains("buffer 0 README.md clean\n    1 | original README\n"),
            "{output}"
        );
        assert!(
            after.contains("Shell buffer target: Scratch(0) @1"),
            "{output}"
        );
        assert!(
            after.contains("buffer 0 README.md dirty\n    1 | edited buffer\n    2 | \n"),
            "{output}"
        );
        let (_, app_view) = after.split_once("App document target:").unwrap();
        assert!(
            app_view.contains(" @0 path=README.md\noriginal README\n"),
            "{output}"
        );
        assert!(app_view.contains("README.md clean\n"), "{output}");
        assert!(!app_view.contains("edited buffer"), "{output}");
        assert_eq!(root.read_file("README.md"), "original README\n");
    }

    #[test]
    fn shell_replace_multiline_save_and_revert() {
        let root = TempWorkspace::new("shell_replace_multiline_save_and_revert");
        root.write_file("README.md", "héllo\nworld\n");
        let input = b"replace 1 1 1 1 x\nopen README.md\nreplace 1 2 2 3 X\\nY\\t\\\\  \ndirty\nshow\nsave\ndirty\nreplace 1 1 1 2 z\nrevert\nshow\nquit\n";
        let mut output = Vec::new();
        run_shell(
            App::new(root.workspace()).unwrap(),
            default_agent_config(),
            &input[..],
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("error: no document is open"));
        assert!(output.contains("README.md dirty"));
        assert!(output.contains("saved"));
        assert!(output.contains("README.md clean"));
        assert!(output.contains("reverted"));
        assert_eq!(output.matches("hX\nY\t\\  rld\n").count(), 2);
        assert_eq!(root.read_file("README.md"), "hX\nY\t\\  rld\n");
    }

    #[test]
    fn shell_replace_invalid_ranges_leave_unicode_unchanged() {
        let root = TempWorkspace::new("shell_replace_invalid_ranges_leave_unicode_unchanged");
        root.write_file("README.md", "é🙂x\n");
        let input = b"open README.md\nreplace 1 2 1 3 bad\nreplace 1 3 1 4 bad\nreplace 1 8 1 1 bad\nreplace 9 1 9 1 bad\nreplace 1 99 1 99 bad\ndirty\nshow\nreplace 1 3 1 7 Z\ndirty\nshow\nquit\n";
        let mut output = Vec::new();
        run_shell(
            App::new(root.workspace()).unwrap(),
            default_agent_config(),
            &input[..],
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("error:").count(), 5);
        assert!(output.contains("README.md clean"));
        assert!(output.contains("é🙂x\n"));
        assert!(output.contains("README.md dirty"));
        assert!(output.contains("éZx\n"));
        assert_eq!(root.read_file("README.md"), "é🙂x\n");
    }

    #[test]
    fn parses_shell_save_command() {
        assert_eq!(ShellCommand::parse("save").unwrap(), ShellCommand::Save);
        assert_eq!(ShellCommand::parse("w").unwrap(), ShellCommand::Save);
    }

    #[test]
    fn parses_shell_agent_command() {
        assert_eq!(
            ShellCommand::parse("agent explain the open file").unwrap(),
            ShellCommand::Agent {
                request: "explain the open file".to_string(),
            }
        );
    }

    #[cfg(feature = "agent")]
    #[test]
    fn parses_agent_command_with_open_file() {
        let parsed = Args::parse(
            [
                "--workspace",
                "/tmp/project",
                "agent",
                "--open",
                "README.md",
                "summarize",
                "this",
            ]
            .map(String::from),
        )
        .unwrap();

        assert_eq!(
            parsed,
            ParseResult::Run(Args {
                writable: false,
                workspace_root: "/tmp/project".to_string(),
                command: Command::Agent {
                    request: "summarize this".to_string(),
                    open_path: Some(ScopePath::new("README.md").unwrap()),
                },
                agent_config: default_agent_config(),
            })
        );
    }

    #[test]
    fn piped_shell_keeps_plain_input_and_does_not_write_history() {
        let root = TempWorkspace::new("piped_history");
        root.write_file("history.json", "[\"prior\"]");
        let workspace = root.workspace();
        let mut output = Vec::new();
        run_shell_inner(
            App::new(workspace.clone()).unwrap(),
            default_agent_config(),
            &b"\r\nnot-a-command\r\nhelp\nquit\n"[..],
            &mut output,
            false,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("error:"));
        assert!(output.contains("Commands:"));
        assert!(!output.contains('\u{1b}'));
        assert_eq!(
            workspace.load_file("history.json").unwrap().contents,
            "[\"prior\"]"
        );
        assert_eq!(workspace.list_files().unwrap().len(), 1);
    }

    #[test]
    fn runs_shell_commands_against_app_state() {
        let root = TempWorkspace::new("runs_shell_commands_against_app_state");
        root.write_file("README.md", "one\ntwo\nthree\n");

        let app = App::new(root.workspace()).unwrap();
        let input = b"files\nopen README.md\nlines 2 3\ndirty\nquit\n";
        let mut output = Vec::new();

        run_shell(app, default_agent_config(), &input[..], &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("> README.md"));
        assert!(output.contains("opened README.md"));
        assert!(output.contains("   2  two"));
        assert!(output.contains("   3  three"));
        assert!(output.contains("README.md clean"));
    }

    #[test]
    fn shell_save_reports_saved_for_open_document() {
        let root = TempWorkspace::new("shell_save_reports_saved_for_open_document");
        root.write_file("README.md", "hello");

        let app = App::new(root.workspace()).unwrap();
        let input = b"open README.md\nsave\nquit\n";
        let mut output = Vec::new();

        run_shell(app, default_agent_config(), &input[..], &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("opened README.md"));
        assert!(output.contains("saved"));
        assert_eq!(root.read_file("README.md"), "hello");
    }
}

#[cfg(test)]
mod inspection_tests {
    use super::*;
    use scope_language::{
        EvaluationResult, InspectionError, InteractiveLanguageSession, InteractiveSessionProvider,
        SessionError, SymbolDescription, SymbolKind, SymbolOrigin,
    };
    use std::sync::Mutex;

    struct Provider(Mutex<Vec<String>>);
    struct Session(&'static Provider);

    impl InteractiveSessionProvider for &'static Provider {
        fn language_id(&self) -> &'static str {
            "inspection-mock"
        }
        fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
            Ok(Box::new(Session(self)))
        }
    }

    impl InteractiveLanguageSession for Session {
        fn language_id(&self) -> &str {
            "inspection-mock"
        }
        fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
            self.0.0.lock().unwrap().push(format!("eval:{source}"));
            Ok(EvaluationResult {
                output: vec![],
                diagnostics: vec![],
                status: scope_language::EvaluationStatus::Success,
            })
        }
        fn inspect(&self, name: &str) -> Result<Option<SymbolDescription>, InspectionError> {
            self.0.0.lock().unwrap().push(format!("inspect:{name}"));
            match name {
                "unknown" => Ok(None),
                "unsupported" => Err(InspectionError::Unsupported),
                "auth" => Err(InspectionError::Authorization {
                    code: "denied".into(),
                    message: "no\u{1b}[31m".into(),
                }),
                "infra" => Err(InspectionError::Infrastructure {
                    code: "offline".into(),
                    message: "retry\rnow".into(),
                }),
                _ => Ok(Some(description())),
            }
        }
        fn reset(&mut self) -> Result<(), SessionError> {
            Ok(())
        }
        fn close(&mut self) -> Result<(), SessionError> {
            Ok(())
        }
    }

    fn description() -> SymbolDescription {
        SymbolDescription {
            name: "sample\u{1b}[31m".into(),
            kind: SymbolKind::Function,
            signature: Some("( -- value ) ! io".into()),
            documentation: Some("first\nsecond\tline".into()),
            origin: Some(SymbolOrigin::Interactive),
            definition: Some("x".repeat(10000)),
            location: None,
        }
    }

    #[test]
    fn borrowed_shell_preserves_owners_on_quit_eof_and_io_error() {
        struct Runtime(std::sync::Arc<Mutex<Vec<String>>>);
        impl InteractiveLanguageSession for Runtime {
            fn language_id(&self) -> &str {
                "borrowed-mock"
            }
            fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
                self.0.lock().unwrap().push(format!("eval:{source}"));
                Ok(EvaluationResult {
                    output: vec![],
                    diagnostics: vec![],
                    status: scope_language::EvaluationStatus::Success,
                })
            }
            fn reset(&mut self) -> Result<(), SessionError> {
                Ok(())
            }
            fn close(&mut self) -> Result<(), SessionError> {
                self.0.lock().unwrap().push("close".into());
                Ok(())
            }
        }
        struct BrokenInput;
        impl std::io::Read for BrokenInput {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("input failed"))
            }
        }
        impl BufRead for BrokenInput {
            fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
                Err(std::io::Error::other("input failed"))
            }
            fn consume(&mut self, _: usize) {}
        }
        let root = crate::test_support::TempWorkspace::new("borrowed_shell");
        root.write_file("target", "original");
        let mut app = App::builder(root.workspace()).build().unwrap();
        app.open_path("target").unwrap();
        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let mut session = InteractiveSession::new();
        session
            .install("borrowed-mock", Box::new(Runtime(events.clone())))
            .unwrap();
        for input in [
            &b"first\n:scope quit\nnot evaluated\n"[..],
            &b"second\n"[..],
        ] {
            run_shell_borrowed(
                &mut app,
                &mut session,
                #[cfg(feature = "agent")]
                AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL),
                #[cfg(not(feature = "agent"))]
                AgentConfig,
                input,
                Vec::new(),
                false,
                #[cfg(feature = "agent")]
                None,
                None,
            )
            .unwrap();
            assert_eq!(session.language_id(), Some("borrowed-mock"));
            assert_eq!(app.open_document().unwrap().text(), "original");
        }
        let error = run_shell_borrowed(
            &mut app,
            &mut session,
            #[cfg(feature = "agent")]
            AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL),
            #[cfg(not(feature = "agent"))]
            AgentConfig,
            BrokenInput,
            Vec::new(),
            false,
            #[cfg(feature = "agent")]
            None,
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("input failed"));
        session.evaluate("after error").unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            ["eval:first", "eval:second", "eval:after error"]
        );
        run_with_session(app, session, &b":scope quit\n"[..], Vec::new()).unwrap();
        assert_eq!(events.lock().unwrap().last().unwrap(), "close");
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| *event == "close")
                .count(),
            1
        );
    }

    #[test]
    fn inspection_routing_errors_and_recovery() {
        let root = crate::test_support::TempWorkspace::new("cli_inspection");
        root.write_file("sentinel", "unchanged");
        let workspace = root.workspace();
        let capabilities = workspace.capabilities().to_vec();
        let provider: &'static Provider = Box::leak(Box::new(Provider(Mutex::new(vec![]))));
        let registry =
            LanguageRegistry::empty().with_interactive_provider(Box::leak(Box::new(provider)));
        let app = App::builder(workspace.clone())
            .language_registry(registry)
            .build()
            .unwrap();
        #[cfg(feature = "agent")]
        let config = AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL);
        #[cfg(not(feature = "agent"))]
        let config = AgentConfig;
        let mut output = Vec::new();
        run_shell(app, config, &b"describe sample\n:scope describe sample\n:scope session inspection-mock\ndescribe sample\neffect sample\nwhere sample\n:scope describe\n:scope effect sample extra\n:scope describe unknown\n:scope effect unsupported\n:scope where auth\n:scope describe infra\n:scope describe sample\n:scope effect sample\n:scope where sample\n:scope quit\n"[..], &mut output).unwrap();
        assert_eq!(
            *provider.0.lock().unwrap(),
            [
                "eval:describe sample",
                "eval:effect sample",
                "eval:where sample",
                "inspect:unknown",
                "inspect:unsupported",
                "inspect:auth",
                "inspect:infra",
                "inspect:sample",
                "inspect:sample",
                "inspect:sample",
            ]
        );
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("unknown command: describe"));
        assert!(output.contains(&InspectionError::NoSession.to_string()));
        assert!(output.contains("describe requires a symbol"));
        assert!(output.contains("unexpected argument: extra"));
        assert!(output.contains("unknown symbol: unknown"));
        assert!(output.contains(&InspectionError::Unsupported.to_string()));
        assert!(output.contains("denied"));
        assert!(output.contains("offline"));
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\r'));
        assert!(output.contains("signature: ( -- value ) ! io"));
        assert!(output.contains("origin: interactive"));
        assert!(output.contains("location: [unavailable]"));
        assert_eq!(workspace.capabilities(), capabilities);
        assert_eq!(root.read_file("sentinel"), "unchanged");
        assert_eq!(workspace.list_files().unwrap().len(), 1);
    }

    #[test]
    fn inspection_modes_missing_fields_and_bounded_controls() {
        for mode in INSPECTION_COMMANDS {
            let mut output = Vec::new();
            print_inspection(mode, "sample", Ok(Some(description())), &mut output).unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(output.len() < 9000);
            assert!(!output.contains('\u{1b}'));
            assert_eq!(output.contains("signature:"), *mode != "where");
            assert_eq!(output.contains("documentation:"), *mode == "describe");
            assert_eq!(output.contains("definition:"), *mode != "effect");
            let mut missing = description();
            missing.signature = None;
            missing.documentation = None;
            missing.origin = None;
            missing.definition = None;
            let mut output = Vec::new();
            print_inspection(mode, "sample", Ok(Some(missing)), &mut output).unwrap();
            assert!(String::from_utf8(output).unwrap().contains("[unavailable]"));
        }
        let sanitized = inspection_text("\0\u{1b}\u{7f}\u{85}\u{202e}\n\t");
        assert!(!sanitized.chars().any(char::is_control));
        assert!(!sanitized.contains('\u{202e}'));
        assert!(inspection_text(&"é".repeat(10000)).len() <= 2051);
    }

    #[test]
    fn describe_previews_definitions_and_points_to_existing_where() {
        for definition in ["x".repeat(10000), "é".repeat(100), "\n\u{1b}".repeat(100)] {
            let mut symbol = description();
            symbol.definition = Some(definition);
            let mut overview = Vec::new();
            print_inspection(
                "describe",
                "sample",
                Ok(Some(symbol.clone())),
                &mut overview,
            )
            .unwrap();
            let overview = String::from_utf8(overview).unwrap();
            let preview = overview
                .lines()
                .find_map(|line| line.strip_prefix("definition: "))
                .unwrap();
            assert!(preview.len() <= 163);
            assert!(preview.ends_with('…'));
            assert!(overview.contains("use :scope where <symbol>"));
            assert!(!overview.contains('\u{1b}'));
            let mut detail = Vec::new();
            print_inspection("where", "sample", Ok(Some(symbol)), &mut detail).unwrap();
            let detail = String::from_utf8(detail).unwrap();
            let full_definition = detail
                .lines()
                .find_map(|line| line.strip_prefix("definition: "))
                .unwrap();
            assert!(full_definition.len() > preview.len());
            assert!(!detail.contains("Definition preview"));
        }
        for definition in [
            None,
            Some(String::new()),
            Some("é".repeat(80)),
            Some("short source".into()),
        ] {
            let mut symbol = description();
            symbol.definition = definition.clone();
            let mut output = Vec::new();
            print_inspection("describe", "sample", Ok(Some(symbol)), &mut output).unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains(&format!(
                "definition: {}",
                definition.as_deref().unwrap_or("[unavailable]")
            )));
            assert!(!output.contains("Definition preview"));
        }
    }

    #[test]
    fn inspection_location_and_origins_are_metadata_only() {
        for (origin, label) in [
            (SymbolOrigin::BuiltIn, "built-in"),
            (SymbolOrigin::Workspace, "workspace"),
            (SymbolOrigin::Interactive, "interactive"),
        ] {
            let mut symbol = description();
            symbol.origin = Some(origin);
            symbol.location = Some(scope_language::SymbolLocation {
                path: scope_language::LanguagePath::new("missing\u{1b}.scope"),
                range: scope_language::TextRange::new(
                    scope_language::Position::new(1, 2),
                    scope_language::Position::new(3, 4),
                ),
            });
            let mut output = Vec::new();
            print_inspection("where", "sample", Ok(Some(symbol)), &mut output).unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains(&format!("origin: {label}")));
            assert!(output.contains(".scope:2:3-4:5"));
            assert!(!output.contains('\u{1b}'));
        }
    }

    #[test]
    fn inspection_completion_is_explicit_even_without_session() {
        for active in [false, true] {
            for name in INSPECTION_COMMANDS {
                let source = format!(":scope {name}");
                let items = complete_commands(&source, source.len(), active).unwrap();
                assert!(items.iter().any(|item| item.label == *name));
                assert!(matches!(
                    ShellCommand::parse(name),
                    Err(ShellError::UnknownCommand(_))
                ));
            }
        }
        let mut help = Vec::new();
        print_shell_help(&mut help).unwrap();
        let help = String::from_utf8(help).unwrap();
        for name in INSPECTION_COMMANDS {
            assert!(help.contains(&format!(":scope {name} <symbol>")));
        }
    }
}

#[cfg(test)]
#[path = "cli/interactive_tests.rs"]
mod interactive_tests;

#[cfg(test)]
mod disposable_source_tests {
    use super::*;

    #[test]
    fn borrowed_shell_without_opt_in_discards_sources() {
        let mut app =
            App::new(crate::workspace::Workspace::read_only(env!("CARGO_MANIFEST_DIR")).unwrap())
                .unwrap();
        let mut session = InteractiveSession::new();
        for input in [
            ":scope scratch new text\n:scope semantic-target 0\n:scope quit\n",
            ":scope buffers\n:scope semantic-target\n",
        ] {
            let mut output = Vec::new();
            run_shell_borrowed(
                &mut app,
                &mut session,
                #[cfg(feature = "agent")]
                AgentConfig::local(DEFAULT_AGENT_MODEL, DEFAULT_AGENT_BASE_URL),
                #[cfg(not(feature = "agent"))]
                AgentConfig,
                input.as_bytes(),
                &mut output,
                false,
                #[cfg(feature = "agent")]
                None,
                None,
            )
            .unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(!output.contains("error:"), "{output}");
            if input.starts_with(":scope buffers") {
                assert!(output.contains("none"), "{output}");
                assert!(!output.contains("target: 0"), "{output}");
            }
        }
    }
}
