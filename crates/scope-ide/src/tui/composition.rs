//! Value-based workspaces using Scope's existing panes and Ratatui renderer.

#[cfg(test)]
#[test]
fn render_propagates_optional_source_owner() {
    use std::{cell::Cell, rc::Rc};

    struct Probe(Rc<Cell<u8>>);
    impl Pane for Probe {
        fn id(&self) -> &'static str {
            "source-probe"
        }

        fn render(&self, _: &mut ratatui::Frame<'_>, _: Rect, ctx: &TuiContext<'_>) {
            assert!(ctx.is_focused());
            assert_eq!(ctx.scroll(), 0);
            self.0.set(match ctx.bound_semantic_source() {
                None => 1,
                Some(crate::scratch::BoundSource::Unbound) => 2,
                _ => panic!("unexpected semantic binding"),
            });
        }
    }

    let root = crate::test_support::TempWorkspace::new("workspace-source-context");
    let app = App::new(root.workspace()).unwrap();
    assert!(TuiContext::new(&app).bound_semantic_source().is_none());
    let sources = crate::scratch::ScratchBuffers::default();
    let observed = Rc::new(Cell::new(0));
    let mut ui = WorkspaceTui::new();
    ui.register("probe", Probe(observed.clone())).unwrap();
    ui.layout("main", View::leaf("probe")).unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 5)).unwrap();
    terminal.draw(|f| ui.render(f, f.area(), &app)).unwrap();
    assert_eq!(observed.get(), 1);
    terminal
        .draw(|f| ui.render_with_sources(f, f.area(), &app, &sources))
        .unwrap();
    assert_eq!(observed.get(), 2);
    terminal.draw(|f| ui.render(f, f.area(), &app)).unwrap();
    assert_eq!(observed.get(), 1);
}
mod actions;
mod draft_apply;
#[cfg(test)]
mod format_tests;
#[cfg(feature = "agent")]
mod progress;
#[cfg(all(test, feature = "experimental-sprout-closure"))]
mod registered_tests;
#[cfg(test)]
mod tests;
use super::{
    DefaultProvider, Pane, TrustedClosureProvider, TuiContext, TuiContextMut,
    session::TerminalSession,
};
#[cfg(test)]
use crate::app::App;
use crate::{app::AppState, cli::ReadOnlyCommand, language::InteractiveSession};
pub use actions::{
    ActionInfo, DownstreamAction, DownstreamPrompt, DownstreamPromptChoice, WorkspaceAction,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use std::collections::{BTreeMap, BTreeSet};

/// A registered instance reference or a nested layout. Horizontal places children
/// side by side; vertical stacks them. Constraints use Ratatui's deterministic
/// solver; undersized terminals may reduce children to zero cells.
#[derive(Clone, Debug)]
pub enum View {
    Leaf(String),
    Split {
        direction: Direction,
        children: Vec<(Constraint, View)>,
    },
    /// Only the selected child is visible. Selection is part of the layout value.
    Tabs {
        selected: usize,
        children: Vec<View>,
    },
}
impl View {
    pub fn leaf(instance: impl Into<String>) -> Self {
        Self::Leaf(instance.into())
    }
    pub fn horizontal(children: impl IntoIterator<Item = View>) -> Self {
        Self::split(
            Direction::Horizontal,
            children.into_iter().map(|v| (Constraint::Fill(1), v)),
        )
    }
    pub fn vertical(children: impl IntoIterator<Item = View>) -> Self {
        Self::split(
            Direction::Vertical,
            children.into_iter().map(|v| (Constraint::Fill(1), v)),
        )
    }
    pub fn split(
        direction: Direction,
        children: impl IntoIterator<Item = (Constraint, View)>,
    ) -> Self {
        Self::Split {
            direction,
            children: children.into_iter().collect(),
        }
    }
    fn validate<P: TrustedClosureProvider>(
        &self,
        instances: &BTreeMap<String, Instance<P>>,
        seen: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 32 {
            return Err("layout nesting exceeds 32".into());
        }
        match self {
            Self::Leaf(id) => {
                if !instances.contains_key(id) {
                    return Err(format!("unknown view instance: {id}"));
                }
                if !seen.insert(id.clone()) {
                    return Err(format!("duplicate view instance in layout: {id}"));
                }
            }
            Self::Split { children, .. } => {
                if children.is_empty() {
                    return Err("empty split".into());
                }
                for (_, child) in children {
                    child.validate(instances, seen, depth + 1)?;
                }
            }
            Self::Tabs { selected, children } => {
                if *selected >= children.len() {
                    return Err("invalid selected tab".into());
                }
                for child in children {
                    child.validate(instances, seen, depth + 1)?;
                }
            }
        }
        Ok(())
    }
    fn place(&self, area: Rect, out: &mut Vec<(String, Rect)>) {
        match self {
            Self::Leaf(id) => {
                if area.width > 0 && area.height > 0 {
                    out.push((id.clone(), area));
                }
            }
            Self::Split {
                direction,
                children,
            } => {
                let areas = Layout::default()
                    .direction(*direction)
                    .constraints(children.iter().map(|(c, _)| *c))
                    .split(area);
                for ((_, child), area) in children.iter().zip(areas.iter()) {
                    child.place(*area, out);
                }
            }
            Self::Tabs { selected, children } => {
                if let Some(child) = children.get(*selected) {
                    child.place(area, out);
                }
            }
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
enum PaneAction {
    Focus(String),
    Help,
}
impl PaneAction {
    fn parse(line: &str) -> Option<Self> {
        let words: Vec<_> = line.split_whitespace().collect();
        match words.as_slice() {
            ["help"] | ["pane", "help"] => Some(Self::Help),
            ["pane", name] => Some(Self::Focus((*name).into())),
            _ => None,
        }
    }
}

// Legacy CLI dispatch still requires the default App. Keep that bridge explicit:
// a typed workspace must never substitute an inert owner or copy its documents.
#[cfg(feature = "agent")]
type LegacySubmit<P> = fn(
    &mut crate::cli::workspace_command::WorkspaceCommandOutput,
    &str,
    &mut AppState<P>,
    &mut InteractiveSession,
    Option<&mut dyn ReadOnlyCommand>,
    &mut crate::scratch::ScratchBuffers,
    &mut dyn FnMut(&crate::agent::AgentTurn),
) -> Result<(), Box<dyn std::error::Error>>;
#[cfg(not(feature = "agent"))]
type LegacySubmit<P> = fn(
    &mut crate::cli::workspace_command::WorkspaceCommandOutput,
    &str,
    &mut AppState<P>,
    &mut InteractiveSession,
    Option<&mut dyn ReadOnlyCommand>,
    &mut crate::scratch::ScratchBuffers,
) -> Result<(), Box<dyn std::error::Error>>;
type LegacyShell<P> = fn(
    &mut AppState<P>,
    &mut InteractiveSession,
    Option<&mut dyn ReadOnlyCommand>,
    Option<&mut crate::scratch::ScratchBuffers>,
) -> Result<(), Box<dyn std::error::Error>>;
struct LegacyRuntime<P: TrustedClosureProvider> {
    submit: LegacySubmit<P>,
    shell: LegacyShell<P>,
}
impl Default for LegacyRuntime<DefaultProvider> {
    fn default() -> Self {
        Self {
            submit: |output,
                     line,
                     app,
                     session,
                     host,
                     sources,
                     #[cfg(feature = "agent")] observer| {
                #[cfg(feature = "agent")]
                {
                    output.submit_observed(line, app, session, host, sources, observer)
                }
                #[cfg(not(feature = "agent"))]
                {
                    output.submit(line, app, session, host, sources)
                }
            },
            shell: |app, session, host, sources| match sources {
                Some(sources) => {
                    crate::cli::run_terminal_borrowed_with_sources(app, session, host, sources)
                }
                None => crate::cli::run_terminal_borrowed(app, session, host),
            },
        }
    }
}

struct Instance<P: TrustedClosureProvider> {
    generation: u64,
    pane: Box<dyn Pane<P>>,
    scroll: u16,
    editable: bool,
}
/// Host-owned instances survive layout replacement, including hidden instances.
/// Instance keys are independent of `Pane::id()` (the view type). No capability
/// or execution context is created by registration or rendering.
#[derive(Default)]
struct EditorSelection {
    identity: u64,
    retained: Vec<EditorAuthority>,
}

struct EditorAuthority {
    source: scope_language::SourceId,
    editor: String,
    generation: u64,
    identity: u64,
    invalidated: bool,
}

pub struct WorkspaceTui<P: TrustedClosureProvider = DefaultProvider> {
    instances: BTreeMap<String, Instance<P>>,
    layouts: Vec<(String, View)>,
    active: usize,
    focused: Option<String>,
    last_focused: Option<String>,
    editor_selection: std::cell::RefCell<EditorSelection>,
    next_generation: u64,
    command_line: Option<String>,
    open_app_prompt: bool,
    open_copy_prompt: bool,
    open_targets: Vec<crate::cli::WorkspaceOpenTarget>,
    open_targets_label: String,
    opened_targets: BTreeMap<(String, String), u64>,
    open_suggestions: super::quick_open::QuickOpen,
    app_editor: bool,
    #[cfg(feature = "desktop")]
    desktop_tabs: Option<Vec<String>>,
    owner_binding_generation: u64,
    // Display hints only: invocation always rediscovers and captures from AppState<P>.
    action_document_present: bool,
    format_registry: crate::language::LanguageRegistry,
    action_format_available: bool,
    action_candidate_available: bool,
    action_registered_available: bool,
    pending_prompt: Option<actions::PendingPrompt>,
    output: crate::cli::workspace_command::WorkspaceCommandOutput,
    output_errors: String,
    // Match the displayed text before attributing freshness to App diagnostics.
    app_diagnostics: Option<AppDiagnostics>,
    output_scroll: u16,
    output_area: Option<Rect>,
    visible: Vec<(String, Rect)>,
    picker: Option<actions::Picker>,
    draft_review: Option<super::draft_review::DraftReview>,
    semantic_inspection: Option<super::semantic::SemanticInspection>,
    retained_inspection: Option<super::semantic::SemanticInspection>,
    downstream_actions: Vec<DownstreamAction>,
    downstream_check_action: Option<String>,
    downstream_run_test_action: Option<String>,

    latest_run_test: Option<(crate::analysis::CheckEvidence, bool, String)>,
    navigation_is_test: bool,
    latest_check: Option<(crate::analysis::CheckEvidence, bool)>,
    scratch_run_available: bool,
    results: Vec<crate::cli::ScriptPresentation>,
    result_index: Option<usize>,
    show_result: bool,
    legacy: Option<LegacyRuntime<P>>,
    #[cfg(feature = "experimental-sprout-closure")]
    registered_execution: Option<super::registered::RegisteredInspection<P::Output>>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum WorkspaceInput {
    Handled,
    Ignored,
    Shell,
    Quit,
}
impl<P: TrustedClosureProvider> WorkspaceTui<P> {
    fn empty() -> Self {
        Self {
            instances: Default::default(),
            layouts: Default::default(),
            active: Default::default(),
            focused: Default::default(),
            last_focused: Default::default(),
            editor_selection: Default::default(),
            next_generation: Default::default(),
            command_line: Default::default(),
            open_app_prompt: Default::default(),
            open_copy_prompt: Default::default(),
            open_targets: Default::default(),
            open_targets_label: Default::default(),
            opened_targets: Default::default(),
            open_suggestions: Default::default(),
            app_editor: Default::default(),
            #[cfg(feature = "desktop")]
            desktop_tabs: None,
            owner_binding_generation: Default::default(),
            action_document_present: Default::default(),
            format_registry: crate::language::LanguageRegistry::empty(),
            action_format_available: false,
            action_candidate_available: Default::default(),
            action_registered_available: false,
            pending_prompt: Default::default(),
            output: Default::default(),
            output_errors: Default::default(),
            app_diagnostics: None,
            output_scroll: Default::default(),
            output_area: Default::default(),
            visible: Default::default(),
            picker: Default::default(),
            draft_review: None,
            semantic_inspection: Default::default(),
            retained_inspection: Default::default(),
            downstream_actions: Default::default(),
            downstream_check_action: Default::default(),
            downstream_run_test_action: Default::default(),

            latest_run_test: Default::default(),
            latest_check: Default::default(),
            navigation_is_test: false,
            scratch_run_available: Default::default(),
            results: Default::default(),
            result_index: Default::default(),
            show_result: Default::default(),
            legacy: None,
            #[cfg(feature = "experimental-sprout-closure")]
            registered_execution: None,
        }
    }
}
struct AppDiagnostics {
    text: String,
    pinned: Option<super::EditorDraftSnapshot>,
    first_offset: Option<usize>,
}

impl Default for WorkspaceTui {
    fn default() -> Self {
        let mut ui = Self::empty();
        ui.legacy = Some(LegacyRuntime::default());
        ui
    }
}
impl WorkspaceTui {
    fn document_shortcut(key: KeyEvent) -> Option<WorkspaceAction> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('t'), event::KeyModifiers::CONTROL)
            | (KeyCode::Enter, event::KeyModifiers::ALT) => {
                Some(WorkspaceAction::CheckCurrentDocument)
            }
            (KeyCode::Char('r'), event::KeyModifiers::CONTROL) => {
                Some(WorkspaceAction::RunCurrentDocument)
            }
            (KeyCode::Char('o'), event::KeyModifiers::CONTROL) => {
                Some(WorkspaceAction::OpenAppDocument)
            }
            _ => None,
        }
    }

    pub fn new() -> Self {
        Self::default()
    }
}
impl<P: TrustedClosureProvider> WorkspaceTui<P> {
    /// Construct a workspace for an explicitly typed provider.
    /// Legacy shell/command dispatch is unavailable; App documents remain in their owner.
    pub fn for_provider() -> Self {
        Self::empty()
    }

    /// Install an operation behind the existing Run/Check actions; never grants authority.
    /// Replacing the configuration drops pending confirmation and retained UI evidence.
    #[cfg(feature = "experimental-sprout-closure")]
    pub fn set_registered_execution(
        &mut self,
        config: Option<super::RegisteredExecution<P::Output>>,
    ) -> Result<(), String> {
        if let Some(config) = &config {
            config.validate()?;
        }
        self.registered_execution = config.map(super::registered::RegisteredInspection::new);
        Ok(())
    }

    /// Passive original result, including snapshots and compiler source-index mapping.
    /// A failed or cancelled later attempt does not discard this historical evidence.
    #[cfg(feature = "experimental-sprout-closure")]
    pub fn registered_result(
        &self,
    ) -> Option<&crate::registered_document_closure::RegisteredClosureResult<P::Output>> {
        self.registered_execution.as_ref()?.result()
    }

    fn registered_execution_available(&self, app: &AppState<P>) -> bool {
        #[cfg(feature = "experimental-sprout-closure")]
        {
            self.app_editor
                && self.registered_execution.is_some()
                && app.selected_registered_module().is_some()
        }
        #[cfg(not(feature = "experimental-sprout-closure"))]
        {
            let _ = app;
            false
        }
    }

    #[cfg(feature = "experimental-sprout-closure")]
    fn finish_registered_run(&mut self) {
        if let Some(observation) = self
            .registered_execution
            .as_mut()
            .and_then(|inspection| inspection.take_completed_observation())
        {
            self.retain_completed_observation(observation);
        }
        let _ = self.focus_named("output");
    }

    fn registered_modal_open(&self) -> bool {
        #[cfg(feature = "experimental-sprout-closure")]
        {
            self.registered_execution
                .as_ref()
                .is_some_and(|inspection| inspection.is_open())
        }
        #[cfg(not(feature = "experimental-sprout-closure"))]
        {
            false
        }
    }

    /// Register a bounded host-curated copy catalog. Targets are opaque, not paths.
    pub fn set_open_targets(
        &mut self,
        label: String,
        targets: Vec<crate::cli::WorkspaceOpenTarget>,
    ) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        if targets.len() > 64
            || label.trim().is_empty()
            || targets.iter().any(|entry| {
                entry.target.is_empty()
                    || entry.target.len() > 4096
                    || entry.label.trim().is_empty()
                    || entry.target.contains(['\n', '\r'])
                    || !seen.insert(entry.target.clone())
                    || crate::path::ScopePath::new(&entry.path).is_err()
            })
        {
            return Err("invalid supplied open catalog".into());
        }
        self.open_targets_label = label;
        self.open_targets = targets;
        self.opened_targets.clear();
        Ok(())
    }

    fn open_workspace_target(
        &mut self,
        app: &AppState<P>,
        sources: &mut crate::scratch::ScratchBuffers,
        target: &str,
    ) -> Result<u64, String> {
        let id = if let Some(entry) = self
            .open_targets
            .iter()
            .find(|entry| entry.target == target)
        {
            if let Some(id) = self
                .opened_targets
                .get(&(app.workspace().root().to_owned(), target.to_owned()))
                .copied()
                .filter(|id| sources.get(*id).is_ok())
            {
                sources.select(id).map_err(|e| e.to_string())?;
                id
            } else {
                let path = crate::path::ScopePath::new(&entry.path).map_err(|e| e.to_string())?;
                let file = app
                    .workspace()
                    .read_document_source(&path)
                    .map_err(|e| e.to_string())?;
                let language = app
                    .languages()
                    .language_for_path(&path)
                    .map(|l| l.language_id())
                    .unwrap_or("text");
                let id = sources.create(language, file.contents);
                self.opened_targets
                    .insert((app.workspace().root().to_owned(), target.into()), id);
                id
            }
        } else {
            sources
                .open(app.workspace(), app.languages(), target)
                .map_err(|e| e.to_string())?
        };
        self.set_app_editor(false);
        self.focus_source_editor(app, Some(sources))?;
        Ok(id)
    }

    #[cfg(feature = "desktop")]
    pub(super) fn enable_desktop_tabs(&mut self, app: &AppState<P>) {
        self.desktop_tabs = Some(
            app.open_document()
                .filter(|_| self.app_editor)
                .map(|doc| doc.path().to_string())
                .into_iter()
                .collect(),
        );
    }

    #[cfg(feature = "desktop")]
    fn tab_dirty(&self, doc: &crate::document::Document) -> bool {
        doc.is_dirty()
            || self
                .instances
                .values()
                .any(|instance| instance.pane.has_dirty_source(doc.evaluation_source_id()))
    }

    #[cfg(feature = "desktop")]
    fn close_desktop_tab(&mut self, app: &mut AppState<P>, path: &str) -> Result<(), String> {
        let Some(tabs) = &self.desktop_tabs else {
            return Ok(());
        };
        let Some(index) = tabs.iter().position(|tab| tab == path) else {
            return Ok(());
        };
        let doc = app
            .desktop_document(path)
            .ok_or("tab document unavailable")?;
        if self.tab_dirty(doc) {
            return Err("Save or cancel changes before closing this tab".into());
        }
        let source = doc.evaluation_source_id().clone();
        let was_active = app
            .open_document()
            .is_some_and(|doc| doc.path().as_str() == path);
        let tabs = self.desktop_tabs.as_mut().unwrap();
        tabs.remove(index);
        let next = tabs.get(index.min(tabs.len().saturating_sub(1))).cloned();
        app.close_desktop_document(path);
        for instance in self.instances.values_mut() {
            instance.pane.forget_closed_source(&source);
        }
        self.editor_selection
            .borrow_mut()
            .retained
            .retain(|entry| entry.source != source);
        if was_active
            && self.app_editor
            && let Some(next) = next
        {
            self.open_app_document(app, &next)?;
        }
        self.refresh_action_context(app);
        Ok(())
    }

    /// Open a confined workspace file directly into App and edit that same document.
    /// Refuses unpublished drafts and replacement of unsaved unenrolled documents.
    /// Enrolled documents stay in their owner, including unsaved changes.
    pub fn open_app_document(&mut self, app: &mut AppState<P>, path: &str) -> Result<(), String> {
        #[cfg(feature = "desktop")]
        let tabs_enabled = self.desktop_tabs.is_some();
        #[cfg(not(feature = "desktop"))]
        let tabs_enabled = false;
        if !tabs_enabled {
            if let Some(reason) = self.draft_blocker() {
                return Err(reason);
            }
            #[cfg(feature = "experimental-sprout-closure")]
            let retained = app.selected_registered_module().is_some();
            #[cfg(not(feature = "experimental-sprout-closure"))]
            let retained = false;
            if !retained
                && app
                    .open_document()
                    .is_some_and(|document| document.is_dirty())
            {
                return Err("cannot replace App document with unsaved changes".into());
            }
        }
        if path.is_empty() || path.contains(['\n', '\r']) {
            return Err("enter one workspace-relative App file path".into());
        }
        let editor = self
            .visible_focus_ids()
            .into_iter()
            .find(|id| {
                self.instances
                    .get(id)
                    .is_some_and(|instance| instance.editable)
            })
            .ok_or("no visible editable pane")?;
        if tabs_enabled {
            #[cfg(feature = "desktop")]
            {
                app.open_desktop_document(path)
                    .map_err(|error| error.to_string())?;
            }
        } else {
            #[cfg(feature = "experimental-sprout-closure")]
            let enrolled = app.registered_documents().and_then(|documents| {
                documents
                    .documents()
                    .find(|(_, document)| document.path().display().to_string() == path)
                    .map(|(module, _)| module.to_owned())
            });
            #[cfg(feature = "experimental-sprout-closure")]
            if let Some(module) = enrolled {
                app.select_registered_document(&module)
                    .map_err(|error| error.to_string())?;
            } else {
                app.open_path(path).map_err(|error| error.to_string())?;
            }
            #[cfg(not(feature = "experimental-sprout-closure"))]
            app.open_path(path).map_err(|error| error.to_string())?;
        }
        // Only the launcher's conventional default split opts into AppState<P>'s minimal view.
        // Keep custom layouts and the workspace layout itself available unchanged.
        if self.layouts.get(self.active).is_some_and(|(name, view)| {
            name == "workspace"
                && matches!(view, View::Split { direction: Direction::Horizontal, children }
                    if children.len() == 2 && children.iter().zip(["editor", "inspector"]).all(
                        |((constraint, view), id)| *constraint == Constraint::Fill(1)
                            && matches!(view, View::Leaf(leaf) if leaf == id)))
        }) && self
            .layouts
            .iter()
            .any(|(name, view)| name == "editor" && matches!(view, View::Leaf(id) if id == &editor))
        {
            let area = self
                .visible
                .iter()
                .find(|(id, _)| id == &editor)
                .map(|(_, area)| *area)
                .unwrap_or_default();
            self.select_layout("editor")?;
            self.geometry(area);
        }
        self.set_app_editor(true);
        self.focus_source_editor(app, None)?;
        self.refresh_action_context(app);
        Ok(())
    }

    /// Select an enrolled App document without routing through scratch. Pane-local
    /// drafts must first be committed to the authoritative Document or cancelled.
    #[cfg(feature = "experimental-sprout-closure")]
    pub fn select_registered_app_document(
        &mut self,
        app: &mut AppState<P>,
        module: &str,
    ) -> Result<(), String> {
        if let Some(reason) = self.draft_blocker() {
            return Err(reason);
        }
        if !self.visible_focus_ids().iter().any(|id| {
            self.instances
                .get(id)
                .is_some_and(|instance| instance.editable)
        }) {
            return Err("no visible editable pane".into());
        }
        app.select_registered_document(module)
            .map_err(|error| error.to_string())?;
        self.set_app_editor(true);
        self.focus_source_editor(app, None)?;
        self.refresh_action_context(app);
        Ok(())
    }

    /// Latest complete semantic observation, retained across modal close/reopen.
    /// Consumers must check `InspectionResult::is_fresh_app` before presenting it as current.
    pub fn inspection_result(&self) -> Option<&crate::semantic_inspection::InspectionResult> {
        self.semantic_inspection
            .as_ref()
            .or(self.retained_inspection.as_ref())?
            .result()
    }
    /// Publish completed feedback into the ordinary two-result lifecycle.
    /// This is passive retention, not evidence validation or execution authority.
    pub fn retain_completed_observation(
        &mut self,
        result: crate::semantic_inspection::InspectionResult,
    ) {
        let mut inspection = self
            .semantic_inspection
            .take()
            .or_else(|| self.retained_inspection.take())
            .unwrap_or_default();
        inspection.retain_completed(result);
        self.retained_inspection = Some(inspection);
    }

    pub fn register(
        &mut self,
        id: impl Into<String>,
        pane: impl Pane<P> + 'static,
    ) -> Result<(), String> {
        let id = id.into();
        if id.is_empty() || id == "output" || self.instances.contains_key(&id) {
            return Err(format!("empty or duplicate instance: {id}"));
        }
        self.next_generation += 1;
        self.instances.insert(
            id,
            Instance {
                generation: self.next_generation,
                pane: Box::new(pane),
                scroll: 0,
                editable: false,
            },
        );
        Ok(())
    }
    /// Explicitly grants focused input access to the bound scratch editing capability.
    /// Ordinary registration and source visibility never grant this authority.
    pub fn register_editable(
        &mut self,
        id: impl Into<String>,
        pane: impl Pane<P> + 'static,
    ) -> Result<(), String> {
        let id = id.into();
        self.register(id.clone(), pane)?;
        self.instances.get_mut(&id).unwrap().editable = true;
        Ok(())
    }

    pub fn layout(&mut self, name: impl Into<String>, view: View) -> Result<(), String> {
        let name = name.into();
        if name.is_empty() || self.layouts.iter().any(|(n, _)| n == &name) {
            return Err("empty or duplicate layout name".into());
        }
        view.validate(&self.instances, &mut BTreeSet::new(), 0)?;
        self.layouts.push((name, view));
        Ok(())
    }
    pub fn select_layout(&mut self, name: &str) -> Result<(), String> {
        self.active = self
            .layouts
            .iter()
            .position(|(n, _)| n == name)
            .ok_or_else(|| format!("unknown layout: {name}"))?;
        self.visible.clear();
        Ok(())
    }
    /// Launcher-only focus selection before terminal geometry is available.
    pub(crate) fn focus_initial(&mut self, name: &str) -> Result<(), String> {
        fn contains(view: &View, name: &str) -> bool {
            match view {
                View::Leaf(id) => id == name,
                View::Split { children, .. } => {
                    children.iter().any(|(_, view)| contains(view, name))
                }
                View::Tabs { selected, children } => children
                    .get(*selected)
                    .is_some_and(|view| contains(view, name)),
            }
        }
        if !self
            .layouts
            .get(self.active)
            .is_some_and(|(_, view)| contains(view, name))
        {
            return Err(format!("pane is not in the initial layout: {name}"));
        }
        self.focused = Some(name.into());
        Ok(())
    }

    /// Owner changes invalidate inspection pins without replacing source-local
    /// editor authority. Record transitions here, not when a snapshot is read.
    fn set_app_editor(&mut self, enabled: bool) {
        if self.app_editor != enabled {
            self.owner_binding_generation = self
                .owner_binding_generation
                .checked_add(1)
                .expect("editor owner transitions exhausted");
            self.app_editor = enabled;
        }
    }

    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }
    fn visible_focus_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.visible.iter().map(|(id, _)| id.clone()).collect();
        if self.output_area.is_some() {
            ids.push("output".into());
        }
        ids
    }

    /// Move input focus without selecting editor authority. Use `select_editor`
    /// for an explicit user selection with its exact current source.
    /// Positions follow the current visible layout, not registration order.
    pub fn focus_named(&mut self, name: &str) -> Result<(), String> {
        let target = if name == "last" {
            self.last_focused.clone().ok_or("no previous pane")?
        } else if let Ok(position) = name.parse::<usize>() {
            self.visible_focus_ids()
                .get(position.wrapping_sub(1))
                .cloned()
                .ok_or("no visible pane at that position")?
        } else {
            name.to_owned()
        };
        if !(target == "output" && self.output_area.is_some())
            && !self.visible.iter().any(|(id, _)| id == &target)
        {
            return Err(format!("pane is not visible: {target}"));
        }

        if self.focused.as_ref() != Some(&target) {
            self.last_focused = self.focused.replace(target);
        }
        Ok(())
    }
    /// Explicitly select the focused editable instance for the source that is
    /// selected now, never for a source discovered at a later snapshot query.
    pub fn select_editor(
        &mut self,
        name: &str,
        app: &AppState<P>,
        sources: Option<&crate::scratch::ScratchBuffers>,
    ) -> Result<(), String> {
        self.focus_named(name)?;
        let editor = self.focused.as_ref().unwrap();
        let Some(instance) = self
            .instances
            .get(editor)
            .filter(|instance| instance.editable)
        else {
            return Ok(());
        };
        let baseline = if self.app_editor {
            app.open_document()
                .map(super::ScratchEditAccess::document_snapshot)
        } else {
            TuiContext::with_view_sources(app, false, 0, sources).selected_source()
        }
        .ok_or("no source selected for editor authority")?;
        let mut selection = self.editor_selection.borrow_mut();
        let previous = selection
            .retained
            .iter()
            .find(|authority| authority.source == baseline.source.source_id);
        if previous.is_some_and(|authority| {
            !authority.invalidated
                && authority.editor == *editor
                && authority.generation == instance.generation
        }) {
            return Ok(());
        }
        selection.identity += 1;
        let identity = selection.identity;
        selection
            .retained
            .retain(|authority| authority.source != baseline.source.source_id);
        selection.retained.push(EditorAuthority {
            source: baseline.source.source_id,
            editor: editor.clone(),
            generation: instance.generation,
            identity,
            invalidated: false,
        });
        Ok(())
    }

    fn focus_source_editor(
        &mut self,
        app: &AppState<P>,
        sources: Option<&crate::scratch::ScratchBuffers>,
    ) -> Result<(), String> {
        let source = if self.app_editor {
            app.open_document().map(|document| {
                super::ScratchEditAccess::document_snapshot(document)
                    .source
                    .source_id
            })
        } else {
            sources
                .and_then(|sources| sources.selected().ok())
                .map(|source| source.evaluation_source_id().clone())
        };
        let retained = self
            .editor_selection
            .borrow()
            .retained
            .iter()
            .find(|authority| Some(&authority.source) == source.as_ref() && !authority.invalidated)
            .map(|authority| authority.editor.clone());
        let editor = retained
            .filter(|editor| self.visible.iter().any(|(id, _)| id == editor))
            .or_else(|| {
                self.visible_focus_ids().into_iter().find(|id| {
                    self.instances
                        .get(id)
                        .is_some_and(|instance| instance.editable)
                })
            });
        if let Some(editor) = editor {
            self.focus_named(&editor)?;
        }
        Ok(())
    }

    fn geometry(&mut self, area: Rect) {
        self.visible.clear();
        if let Some((_, view)) = self.layouts.get(self.active) {
            view.place(area, &mut self.visible);
        }
        if !(self.focused.as_deref() == Some("output") && self.output_area.is_some())
            && !self
                .visible
                .iter()
                .any(|(id, _)| Some(id) == self.focused.as_ref())
        {
            self.focused = self.visible.first().map(|(id, _)| id.clone());
        }
    }
    pub fn render(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect, app: &AppState<P>) {
        self.render_optional_sources(frame, area, app, None);
    }

    /// Renders panes with read-only access to the caller's semantic source binding.
    pub fn render_with_sources(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        app: &AppState<P>,
        sources: &crate::scratch::ScratchBuffers,
    ) {
        self.render_optional_sources(frame, area, app, Some(sources));
    }

    fn render_optional_sources(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        app: &AppState<P>,
        sources: Option<&crate::scratch::ScratchBuffers>,
    ) {
        self.refresh_action_context(app);
        self.geometry(area);
        let editor_drafts = self
            .instances
            .values()
            .any(|instance| instance.pane.has_dirty_draft());
        let current_draft = self.editor_draft_outcome(app, sources);
        let check = self.check_presentation(app, sources);
        for (id, area) in &self.visible {
            let instance = &self.instances[id];
            let focused = self.focused.as_ref() == Some(id);
            let block = Block::default()
                .borders(Borders::ALL)
                .title(id.as_str())
                .border_style(if focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                });
            let inner = block.inner(*area);
            frame.render_widget(block, *area);
            instance.pane.render(
                frame,
                inner,
                &TuiContext::with_view_sources(app, focused, instance.scroll, sources)
                    .with_editor_drafts(editor_drafts)
                    .with_check_presentation(check.clone())
                    .with_draft_outcome(current_draft.clone())
                    .with_app_editor(self.app_editor),
            );
        }
        if let Some(inspection) = &self.semantic_inspection {
            inspection.render(
                frame,
                frame.area(),
                app,
                self.instances
                    .values()
                    .any(|instance| instance.pane.has_dirty_app_draft()),
            );
        }
        if let Some(review) = &self.draft_review {
            review.render(frame, frame.area());
        }
        #[cfg(feature = "experimental-sprout-closure")]
        if let Some(inspection) = &self.registered_execution {
            inspection.render(frame, frame.area(), app, self.draft_blocker().is_some());
        }
    }
    /// Host shortcuts: F4 command bar; Esc/F2 shell; F6/Shift-F6 focus; F7 next layout;
    /// Ctrl-Q exit. All other keys (including Tab and q) belong to the pane.
    pub fn handle_key(&mut self, key: KeyEvent, app: &mut AppState<P>) -> WorkspaceInput {
        self.handle_event_optional_sources(Event::Key(key), app, None)
    }

    /// Routes host shortcuts first, then grants only the focused opted-in pane
    /// a short-lived editing capability. Paste is never interpreted as commands.
    pub fn handle_event_with_sources(
        &mut self,
        event: Event,
        app: &mut AppState<P>,
        sources: &mut crate::scratch::ScratchBuffers,
    ) -> WorkspaceInput {
        self.handle_event_optional_sources(event, app, Some(sources))
    }

    fn handle_event_optional_sources(
        &mut self,
        input: Event,
        app: &mut AppState<P>,
        mut sources: Option<&mut crate::scratch::ScratchBuffers>,
    ) -> WorkspaceInput {
        self.refresh_action_context(app);
        let key = match &input {
            Event::Key(key) => *key,
            Event::Paste(_) => KeyEvent::new(KeyCode::Null, event::KeyModifiers::NONE),
            _ => return WorkspaceInput::Ignored,
        };
        if key.kind != KeyEventKind::Press {
            return WorkspaceInput::Ignored;
        }
        if let Some(mut review) = self.draft_review.take() {
            if let Some(decision) = review.decision(key) {
                let replacement = review.replacement.as_deref().unwrap_or(&review.pinned.text);
                if let Err(error) = self.apply_reviewed_editor_draft(
                    app,
                    sources.as_deref(),
                    &review.pinned,
                    replacement,
                    decision,
                ) {
                    review.fail(error);
                    self.draft_review = Some(review);
                }
            } else {
                self.draft_review = Some(review);
            }
            return WorkspaceInput::Handled;
        }
        #[cfg(feature = "experimental-sprout-closure")]
        if self.registered_modal_open() {
            let blocker = self.draft_blocker();
            let completed = self
                .registered_execution
                .as_mut()
                .unwrap()
                .handle_key(key, app, blocker);
            if completed {
                self.finish_registered_run();
            }
            return WorkspaceInput::Handled;
        }
        if let Some(inspection) = &mut self.semantic_inspection {
            if inspection.handle_key(key, app) {
                self.retained_inspection = self.semantic_inspection.take();
            }
            return WorkspaceInput::Handled;
        }
        match key.code {
            KeyCode::Char('g') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                self.picker = Some(actions::Picker::default());
                return WorkspaceInput::Handled;
            }
            KeyCode::F(1) => {
                self.picker = Some(actions::Picker::default());
                return WorkspaceInput::Handled;
            }
            KeyCode::F(3) => {
                let _ = self.select_editor("last", app, sources.as_deref());
                return WorkspaceInput::Handled;
            }
            KeyCode::F(4) => {
                self.open_app_prompt = false;
                self.command_line = Some(String::new());
                return WorkspaceInput::Handled;
            }
            KeyCode::Esc if self.app_editor => {
                if self.focused.as_deref() == Some("output") {
                    let _ = self.select_editor("last", app, sources.as_deref());
                }
                return WorkspaceInput::Handled;
            }
            KeyCode::Esc | KeyCode::F(2) => return WorkspaceInput::Shell,
            KeyCode::Char('q') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                return WorkspaceInput::Quit;
            }
            KeyCode::F(7) if !self.layouts.is_empty() => {
                self.active = (self.active + 1) % self.layouts.len();
                self.visible.clear();
                return WorkspaceInput::Handled;
            }
            KeyCode::F(6) if !self.visible.is_empty() || self.output_area.is_some() => {
                let ids = self.visible_focus_ids();
                let old = ids
                    .iter()
                    .position(|id| Some(id) == self.focused.as_ref())
                    .unwrap_or(0);
                let step = if key.modifiers.contains(event::KeyModifiers::SHIFT) {
                    ids.len() - 1
                } else {
                    1
                };
                let _ = self.select_editor(&ids[(old + step) % ids.len()], app, sources.as_deref());
                return WorkspaceInput::Handled;
            }
            _ => {}
        }
        if self.focused.as_deref() == Some("output") {
            let page = self
                .output_area
                .map_or(1, |area| area.height.saturating_sub(2).max(1));
            match key.code {
                #[cfg(feature = "experimental-sprout-closure")]
                KeyCode::Char('d') if self.registered_execution_available(app) => {
                    self.registered_execution.as_mut().unwrap().inspect();
                }
                KeyCode::Up => self.output_scroll = self.output_scroll.saturating_sub(1),
                KeyCode::Down => {
                    self.output_scroll = self.output_scroll.saturating_add(1).min(
                        self.rendered_output(app, sources.as_deref())
                            .lines()
                            .count()
                            .saturating_sub(1)
                            .min(u16::MAX as usize) as u16,
                    )
                }
                KeyCode::PageUp => self.output_scroll = self.output_scroll.saturating_sub(page),
                KeyCode::PageDown => {
                    self.output_scroll = self.output_scroll.saturating_add(page).min(
                        self.rendered_output(app, sources.as_deref())
                            .lines()
                            .count()
                            .saturating_sub(1)
                            .min(u16::MAX as usize) as u16,
                    )
                }
                _ => return WorkspaceInput::Ignored,
            }
            return WorkspaceInput::Handled;
        }
        if let Some((id, area)) = self
            .visible
            .iter()
            .find(|(id, _)| Some(id) == self.focused.as_ref())
        {
            let current_draft = self.editor_draft_outcome(app, sources.as_deref());
            let check = self.check_presentation(app, sources.as_deref());
            let editor_drafts = self
                .instances
                .values()
                .any(|instance| instance.pane.has_dirty_draft());
            let instance = self.instances.get_mut(id).unwrap();
            let inner = Block::default().borders(Borders::ALL).inner(*area);
            if let Some(path) = instance.pane.workspace_document_request(
                &input,
                &TuiContext::with_view_sources(app, true, instance.scroll, sources.as_deref())
                    .with_check_presentation(check.clone())
                    .with_draft_outcome(current_draft.clone())
                    .with_editor_drafts(editor_drafts)
                    .with_app_editor(self.app_editor),
            ) {
                self.show_result = false;
                let result = sources
                    .as_deref_mut()
                    .ok_or_else(|| "no workspace source owner available".to_owned())
                    .and_then(|sources| self.open_workspace_target(app, sources, &path));
                match result {
                    Ok(_) => {
                        self.set_app_editor(false);
                    }
                    Err(error) => self.output_errors.push_str(&format!("{error}\n")),
                }
                return WorkspaceInput::Handled;
            }
            if instance.editable && self.app_editor {
                if let Some(document) = app.open_document_mut() {
                    instance
                        .pane
                        .handle_source_event(input, &mut super::ScratchEditAccess::app(document));
                } else {
                    self.output_errors
                        .push_str("App document is no longer open; draft retained.\n");
                }
                return WorkspaceInput::Handled;
            }
            if instance.editable
                && let Some(sources) = sources.as_deref_mut()
                && instance.pane.handle_source_event(
                    input.clone(),
                    &mut super::ScratchEditAccess::selected(sources),
                )
            {
                return WorkspaceInput::Handled;
            }
            if instance.pane.handle_view_event(
                input.clone(),
                &TuiContext::with_view_sources(app, true, instance.scroll, sources.as_deref())
                    .with_check_presentation(check.clone())
                    .with_draft_outcome(current_draft.clone())
                    .with_editor_drafts(editor_drafts)
                    .with_app_editor(self.app_editor),
            ) {
                return WorkspaceInput::Handled;
            }
            if matches!(input, Event::Key(_))
                && instance.pane.handle_key(
                    key,
                    &mut TuiContextMut::with_scroll(app, &mut instance.scroll, inner),
                )
            {
                return WorkspaceInput::Handled;
            }
        }
        WorkspaceInput::Ignored
    }
    fn output_text(&self) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(self.output.bytes()),
            self.output_errors
        )
    }

    fn open_document_event(&mut self, input: &Event, app: &AppState<P>) -> Option<String> {
        self.open_suggestions
            .set_query(self.command_line.as_deref().unwrap_or_default());
        if let Event::Key(key) = input
            && key.kind == KeyEventKind::Press
        {
            if self.open_suggestions.navigate(key.code, app) {
                return None;
            }
            if matches!(key.code, KeyCode::Tab | KeyCode::Enter) {
                self.command_line = Some(self.open_suggestions.selected_path(app)?);
                if key.code == KeyCode::Tab {
                    return None;
                }
            }
        }
        let submitted = self.command_event(input);
        self.open_suggestions
            .set_query(self.command_line.as_deref().unwrap_or_default());
        submitted
    }

    // Returns a submission only on Enter. All command editing is isolated from panes.
    fn command_event(&mut self, input: &Event) -> Option<String> {
        if let Some(pending) = self.pending_prompt.as_mut()
            && let Event::Key(key) = input
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Up => {
                    pending.selected = pending.selected.saturating_sub(1);
                    return None;
                }
                KeyCode::Down => {
                    pending.selected = (pending.selected + 1).min(pending.prompt.choices.len() - 1);
                    return None;
                }
                KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                    self.pending_prompt = None;
                    self.command_line = None;
                    return None;
                }
                _ => {}
            }
        }
        let line = self.command_line.as_mut()?;
        match input {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc | KeyCode::F(4) => {
                    self.command_line = None;
                    self.open_app_prompt = false;
                    self.open_copy_prompt = false;
                    self.open_suggestions = Default::default();
                    self.pending_prompt = None;
                }
                KeyCode::Enter => return self.command_line.take(),
                KeyCode::Backspace => {
                    line.pop();
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(event::KeyModifiers::CONTROL | event::KeyModifiers::ALT) =>
                {
                    line.push(ch)
                }
                _ => {}
            },
            Event::Paste(text) => line.push_str(text),
            _ => {}
        }
        None
    }

    #[cfg(feature = "agent")]
    pub(crate) fn configure_commands(
        &mut self,
        editing: Option<crate::cli::InteractiveEditConfig>,
        agent_config: crate::agent::AgentConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(editing) = editing {
            self.output =
                crate::cli::workspace_command::WorkspaceCommandOutput::with_editing(editing)?;
        }
        self.output.set_agent_config(agent_config);
        Ok(())
    }

    /// The same retained controller used by F4 source-proposal commands.
    /// Proposal-producing hosts must independently authorize their operation;
    /// this grants no source mutation, evaluation, or filesystem authority.
    pub fn source_edit_controller(&mut self) -> &mut crate::source_edit::SourceEditController {
        self.output.source_edit_controller()
    }

    fn document_pending(
        &self,
        line: &str,
        sources: &crate::scratch::ScratchBuffers,
    ) -> Option<String> {
        let command = line.strip_prefix(":scope ")?.trim();
        let name = command.split_whitespace().next()?;
        if !matches!(name, "document-ask" | "document-propose") {
            return None;
        }
        if name == "document-propose" && self.draft_blocker().is_some() {
            return None;
        }
        let source = sources.selected().ok()?;
        Some(format!(
            "Pending {name} — source {:?}, revision {} (committed)\nUI waits synchronously until dispatch returns; input and cancellation unavailable.\nNo progress updates; request may still fail validation.",
            source.id(),
            source.version(),
        ))
    }

    fn render_document_pending(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        app: &AppState<P>,
        sources: Option<&crate::scratch::ScratchBuffers>,
        status: &str,
    ) {
        let areas =
            Layout::vertical([Constraint::Min(0), Constraint::Length(6)]).split(frame.area());
        self.render_optional_sources(frame, areas[0], app, sources);
        frame.render_widget(
            Paragraph::new(status)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("document agent"),
                ),
            areas[1],
        );
    }

    #[cfg(test)]
    fn submit_command(
        &mut self,
        line: &str,
        app: &mut AppState<P>,
        session: &mut InteractiveSession,
        host: Option<&mut dyn ReadOnlyCommand>,
        sources: &mut crate::scratch::ScratchBuffers,
    ) {
        self.submit_command_inner(
            line,
            app,
            session,
            host,
            sources,
            #[cfg(feature = "agent")]
            &mut |_| {},
        );
    }

    fn submit_command_inner(
        &mut self,
        line: &str,
        app: &mut AppState<P>,
        session: &mut InteractiveSession,
        host: Option<&mut dyn ReadOnlyCommand>,
        sources: &mut crate::scratch::ScratchBuffers,
        #[cfg(feature = "agent")] observer: &mut dyn FnMut(&crate::agent::AgentTurn),
    ) {
        self.show_result = false;
        if self.open_app_prompt || self.open_copy_prompt {
            let copy = self.open_copy_prompt;
            self.open_app_prompt = false;
            self.open_copy_prompt = false;
            self.open_suggestions = Default::default();
            let result = if copy {
                if self.open_targets.iter().any(|entry| entry.target == line) {
                    self.open_workspace_target(app, sources, line).map(|_| ())
                } else {
                    Err("unknown supplied open target".into())
                }
            } else {
                self.open_app_document(app, line)
            };
            if let Err(error) = result {
                self.output_errors.push_str(&format!("{error}\n"));
            }
            return;
        }
        let selected_before = sources
            .selected()
            .ok()
            .map(|source| source.evaluation_source_id().clone());
        let result = match PaneAction::parse(line) {
            Some(PaneAction::Focus(name)) => self.select_editor(&name, app, Some(sources)),
            Some(PaneAction::Help) => {
                self.output_errors.push_str("F4 command • Esc cancel • F6/Shift-F6 focus • PageUp/PageDown scroll output\npane <registered visible name|output|last|1|2|…> • help\nNumbers follow visible layout order, then output. Names are exact instance names; no editor alias.\n:scope help lists existing shell actions; :scope context shows source owners.\nSubmit existing :scope commands unchanged; commit editor drafts before :scope script run.\n");
                Ok(())
            }
            None => {
                let execution = line.strip_prefix(":scope ").is_some_and(|command| {
                    let words: Vec<_> = command.split_whitespace().collect();
                    matches!(
                        words.first(),
                        Some(
                            &"downstream"
                                | &"source-accept"
                                | &"document-propose"
                                | &"save"
                                | &"save-as"
                        )
                    ) || words.starts_with(&["agent", "edit"])
                        || words.as_slice() == ["script", "run"]
                        || host.as_deref().is_some_and(|host| {
                            host.compositions().iter().any(|entry| {
                                words.as_slice()
                                    == [entry.namespace.as_str(), entry.command.as_str()]
                            })
                        })
                });
                // Stop compositions before even their source selection/binding steps.
                let blocker = execution.then(|| self.draft_blocker()).flatten();
                if let Some(reason) = blocker {
                    Err(reason)
                } else {
                    self.legacy
                        .as_ref()
                        .ok_or_else(|| {
                            "legacy :scope dispatch is unavailable for this typed workspace"
                                .to_string()
                        })
                        .and_then(|legacy| {
                            (legacy.submit)(
                                &mut self.output,
                                line,
                                app,
                                session,
                                host,
                                sources,
                                #[cfg(feature = "agent")]
                                observer,
                            )
                            .map_err(|error| error.to_string())
                        })
                }
            }
        };
        if selected_before
            != sources
                .selected()
                .ok()
                .map(|source| source.evaluation_source_id().clone())
        {
            self.set_app_editor(false);
        }
        if let Err(error) = result {
            self.output_errors.push_str(&format!("{error}\n"));
        }
    }

    fn footer_height(&self) -> u16 {
        if self.pending_prompt.is_some() { 4 } else { 1 }
    }

    fn render_footer(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        use ratatui::widgets::Wrap;

        if let Some(pending) = &self.pending_prompt {
            // Reserve input space independently of contextual metadata, even on tiny terminals.
            let input_height = area.height.min(2);
            let metadata_height = area.height.saturating_sub(input_height);
            let areas = Layout::vertical([
                Constraint::Length(metadata_height.min(1)),
                Constraint::Length(metadata_height.saturating_sub(1)),
                Constraint::Length(input_height),
            ])
            .split(area);
            frame.render_widget(Paragraph::new(pending.prompt.label.as_str()), areas[0]);
            frame.render_widget(
                Paragraph::new(format!(
                    "↑/↓ select • Esc/Ctrl-C cancel • {}",
                    pending.prompt.choices[pending.selected].label,
                )),
                areas[1],
            );
            let candidate = format!("> {}", self.command_line.as_deref().unwrap_or_default());
            let paragraph = Paragraph::new(candidate).wrap(Wrap { trim: false });
            let scroll = paragraph
                .line_count(areas[2].width)
                .saturating_sub(areas[2].height as usize)
                .min(u16::MAX as usize) as u16;
            frame.render_widget(paragraph.scroll((scroll, 0)), areas[2]);
        } else {
            frame.render_widget(Paragraph::new(self.command_line.as_ref()
                .map(|s| format!("{}> {s}", if self.open_app_prompt || self.open_copy_prompt { "Open document: " } else { "" }))
                .unwrap_or_else(|| if self.app_editor {
                    if self.action_candidate_available {
                        "Ctrl-O Open · Ctrl-R Run · Ctrl-T Check · Ctrl-Q Quit · Ctrl-G More".into()
                    } else {
                        "Ctrl-O Open · Ctrl-T Check · Ctrl-Q Quit · Ctrl-G More".into()
                    }
                } else {
                    format!("Ctrl-O open • {}Ctrl-T check • Ctrl-S publish (memory) • Ctrl-G actions/Save • Ctrl-Q quit",
                        if self.scratch_run_available { "F5 Run Script • " } else { "" })
                })), area);
        }
    }

    fn rendered_output(
        &self,
        app: &AppState<P>,
        sources: Option<&crate::scratch::ScratchBuffers>,
    ) -> String {
        // A rejected latest action must not present an older Run as its outcome.
        if self.app_editor && !self.output_errors.is_empty() {
            if let Some(diagnostics) = &self.app_diagnostics
                && diagnostics.text == self.output_errors
                && diagnostics
                    .pinned
                    .as_ref()
                    .is_none_or(|pinned| self.validate_editor_draft(app, sources, pinned).is_err())
            {
                return format!(
                    "STALE (historical App Check; editor draft changed or unavailable)\n{}",
                    diagnostics.text
                );
            }
            return self.output_errors.clone();
        }
        #[cfg(feature = "experimental-sprout-closure")]
        if self.registered_execution_available(app) {
            return self
                .registered_execution
                .as_ref()
                .unwrap()
                .concise(app, self.draft_blocker().is_some())
                .unwrap_or_else(|| "Registered closure: use Check or Run".into());
        }
        if self.app_editor
            && let Some(inspection) = &self.retained_inspection
            && let Some(summary) = inspection.concise_output(app)
        {
            let draft_stale = self
                .instances
                .values()
                .any(|instance| instance.pane.has_dirty_draft())
                && !inspection.is_stale(app);
            return format!(
                "{}{}",
                if draft_stale {
                    "STALE (unpublished editor draft)\n"
                } else {
                    ""
                },
                summary,
            );
        }
        self.result_text(sources).unwrap_or_else(|| {
            let mut output = self.output_text();
            // Retained test output never enters the unvalidated shared transcript.
            // Revalidate it whenever that transcript is presented, including after focus actions.
            let test_output = if let Some(sources) = sources {
                self.run_test_output(app, sources)
            } else {
                self.latest_run_test.as_ref().map(|(evidence, _, text)| {
                    format!(
                        "Run Test [stale / historical; invocation {:?}]\n{text}",
                        evidence.invocation_id()
                    )
                })
            };
            if let Some(test_output) = test_output {
                if !output.is_empty() && !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str(&test_output);
            }
            output
        })
    }

    // Shared by the terminal loop and rendered composition regression tests.
    fn render_run_frame(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        app: &AppState<P>,
        sources: Option<&crate::scratch::ScratchBuffers>,
        quit_pending: bool,
    ) {
        let output_height = if self.app_editor {
            5.min(frame.area().height / 3).max(1)
        } else {
            (frame.area().height / 3).clamp(1, 8)
        };
        let areas = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(output_height),
            Constraint::Length(self.footer_height()),
        ])
        .split(frame.area());
        self.output_area = (areas[1].width > 0 && areas[1].height > 0).then_some(areas[1]);
        self.render_optional_sources(frame, areas[0], app, sources);
        let block = Block::default()
            .borders(Borders::ALL)
            .title("output")
            .border_style(if self.focused.as_deref() == Some("output") {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            });
        frame.render_widget(
            Paragraph::new(self.rendered_output(app, sources))
                .block(block)
                .scroll((self.output_scroll, 0)),
            areas[1],
        );
        if quit_pending {
            frame.render_widget(
                Paragraph::new(
                    "Unsaved work: Ctrl-Q again to discard and quit • Esc/Ctrl-C cancel",
                ),
                areas[2],
            );
        } else {
            self.render_footer(frame, areas[2]);
        }
        if self.open_app_prompt || self.open_copy_prompt {
            let title = if self.open_copy_prompt {
                format!(
                    "{} — ↑/↓ select · Tab complete · Enter open · Esc cancel",
                    self.open_targets_label
                )
            } else {
                "Open document — ↑/↓ select · Tab complete · Enter open · Esc cancel".into()
            };
            self.open_suggestions
                .render_titled(frame, areas[1], app, &title);
        }
        #[cfg(feature = "experimental-sprout-closure")]
        if let Some(inspection) = &self.registered_execution {
            inspection.render(frame, frame.area(), app, self.draft_blocker().is_some());
        }
    }

    /// Owns the terminal and input loop; the caller retains app, session and host.
    pub fn run(
        &mut self,
        app: &mut AppState<P>,
        session: &mut InteractiveSession,
        command: Option<&mut dyn ReadOnlyCommand>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.run_optional_sources(app, session, command, None, None)
    }

    /// Runs with caller-owned scratch buffers retained across shell handoffs.
    pub fn run_with_sources(
        &mut self,
        app: &mut AppState<P>,
        session: &mut InteractiveSession,
        command: Option<&mut dyn ReadOnlyCommand>,
        sources: &mut crate::scratch::ScratchBuffers,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.run_optional_sources(app, session, command, Some(sources), None)
    }

    #[cfg(feature = "desktop")]
    pub(super) fn run_desktop(
        &mut self,
        app: &mut AppState<P>,
        command: Option<&mut dyn ReadOnlyCommand>,
        sources: &mut crate::scratch::ScratchBuffers,
        desktop: super::desktop::DesktopIo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.run_optional_sources(
            app,
            &mut InteractiveSession::new(),
            command,
            Some(sources),
            Some(WorkspaceSurface::Desktop(Box::new(desktop))),
        )
    }

    fn run_optional_sources(
        &mut self,
        app: &mut AppState<P>,
        session: &mut InteractiveSession,
        mut command: Option<&mut dyn ReadOnlyCommand>,
        mut sources: Option<&mut crate::scratch::ScratchBuffers>,
        surface: Option<WorkspaceSurface>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.layouts.is_empty() {
            return Err("register at least one layout".into());
        }
        let mut fallback_sources = crate::scratch::ScratchBuffers::default();
        let mut terminal = Some(match surface {
            Some(surface) => surface,
            None => WorkspaceSurface::terminal()?,
        });
        let mut quit_pending = false;
        loop {
            self.scratch_run_available =
                command.as_ref().is_some_and(|h| h.supports_direct_script())
                    && sources
                        .as_deref()
                        .unwrap_or(&fallback_sources)
                        .selected()
                        .is_ok_and(|source| source.language_id() == "scope-script");
            #[cfg(feature = "desktop")]
            if let Some(WorkspaceSurface::Desktop(io)) = terminal.as_mut() {
                if let Some(tabs) = &mut self.desktop_tabs
                    && let Some(doc) = app.open_document().filter(|_| self.app_editor)
                    && !tabs.iter().any(|path| path == doc.path().as_str())
                {
                    tabs.push(doc.path().to_string());
                }
                io.set_tabs(
                    self.desktop_tabs
                        .iter()
                        .flatten()
                        .filter_map(|path| {
                            let doc = app.desktop_document(path)?;
                            Some(super::desktop::DesktopTab {
                                path: path.clone(),
                                dirty: self.tab_dirty(doc),
                            })
                        })
                        .collect(),
                );
                io.set_status(
                    app.open_document()
                        .filter(|_| self.app_editor)
                        .map(|doc| doc.path().to_string()),
                    self.has_unsaved_work(app, sources.as_deref()),
                    self.draft_blocker().is_some(),
                    self.app_editor && app.open_document().is_some_and(|doc| doc.is_dirty()),
                    quit_pending,
                );
            }
            terminal.as_mut().unwrap().draw(|frame| {
                self.render_run_frame(frame, app, sources.as_deref(), quit_pending);
                self.render_picker(
                    frame,
                    sources.as_deref().unwrap_or(&fallback_sources),
                    command.as_ref().is_some_and(|h| h.supports_direct_script()),
                );
                if let Some(inspection) = &self.semantic_inspection {
                    inspection.render(
                        frame,
                        frame.area(),
                        app,
                        self.instances
                            .values()
                            .any(|instance| instance.pane.has_dirty_app_draft()),
                    );
                }
                if let Some(review) = &self.draft_review {
                    review.render(frame, frame.area());
                }
            })?;
            if let Some(input) = match terminal.as_mut().unwrap().read()? {
                SurfaceInput::Idle => None,
                #[cfg(feature = "desktop")]
                SurfaceInput::Closed => break,
                #[cfg(feature = "desktop")]
                SurfaceInput::Desktop(input) => {
                    use super::desktop::DesktopInput;
                    if matches!(input, DesktopInput::Close) {
                        if !self.has_unsaved_work(app, sources.as_deref()) {
                            break;
                        }
                        quit_pending = true;
                        continue;
                    }
                    if matches!(input, DesktopInput::ConfirmClose) && quit_pending {
                        break;
                    }
                    if quit_pending
                        || self.command_line.is_some()
                        || self.picker.is_some()
                        || self.draft_review.is_some()
                        || self.semantic_inspection.is_some()
                        || self.registered_modal_open()
                    {
                        continue;
                    }
                    match input {
                        DesktopInput::Open(path) => {
                            if let Err(error) = self.open_app_document(app, &path) {
                                self.show_result = false;
                                self.output_errors.push_str(&format!("{error}\n"));
                            }
                        }
                        DesktopInput::CloseDocument(path) => {
                            if let Err(error) = self.close_desktop_tab(app, &path) {
                                self.show_result = false;
                                self.output_errors.push_str(&format!("{error}\n"));
                            }
                        }
                        DesktopInput::CycleDocument { backwards } => {
                            if let Some(tabs) = &self.desktop_tabs
                                && !tabs.is_empty()
                            {
                                let active = app
                                    .open_document()
                                    .filter(|_| self.app_editor)
                                    .and_then(|doc| {
                                        tabs.iter().position(|path| path == doc.path().as_str())
                                    });
                                let next = match active {
                                    Some(index) if backwards => {
                                        (index + tabs.len() - 1) % tabs.len()
                                    }
                                    Some(index) => (index + 1) % tabs.len(),
                                    None if backwards => tabs.len() - 1,
                                    None => 0,
                                };
                                let path = tabs[next].clone();
                                if let Err(error) = self.open_app_document(app, &path) {
                                    self.output_errors.push_str(&format!("{error}\n"));
                                }
                            }
                        }
                        DesktopInput::Copy { cut } => {
                            if let Some(draft) = self.current_editor_draft(app, sources.as_deref())
                                && self.focused() == Some(draft.editor.as_str())
                                && let Some(anchor) =
                                    draft.anchor.filter(|anchor| *anchor != draft.cursor)
                                && let Some(text) = draft
                                    .text
                                    .get(anchor.min(draft.cursor)..anchor.max(draft.cursor))
                            {
                                if let Some(WorkspaceSurface::Desktop(io)) = terminal.as_mut() {
                                    io.copy(text.to_owned());
                                }
                                if cut {
                                    self.handle_event_optional_sources(
                                        Event::Key(KeyEvent::new(
                                            KeyCode::Delete,
                                            event::KeyModifiers::NONE,
                                        )),
                                        app,
                                        sources.as_deref_mut(),
                                    );
                                }
                            }
                        }
                        DesktopInput::ConfirmClose => {}
                        _ => unreachable!("surface handles transport input"),
                    }
                    continue;
                }
                SurfaceInput::Event(input) => Some(input),
            } {
                if quit_pending {
                    if let Event::Key(key) = &input
                        && key.kind == KeyEventKind::Press
                    {
                        if key.code == KeyCode::Char('q')
                            && key.modifiers.contains(event::KeyModifiers::CONTROL)
                        {
                            break;
                        }
                        if key.code == KeyCode::Esc
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers.contains(event::KeyModifiers::CONTROL))
                        {
                            quit_pending = false;
                        }
                    }
                    continue;
                }
                if self.draft_review.is_some()
                    || self.semantic_inspection.is_some()
                    || self.registered_modal_open()
                {
                    self.handle_event_optional_sources(input, app, sources.as_deref_mut());
                    continue;
                }
                #[cfg(feature = "desktop")]
                if matches!(terminal.as_ref(), Some(WorkspaceSurface::Desktop(_)))
                    && self.picker.is_none()
                    && self.command_line.is_none()
                    && self.app_editor
                    && app.workspace().allows_saves()
                    && matches!(&input, Event::Key(key) if key.kind == KeyEventKind::Press
                        && key.code == KeyCode::Char('s') && key.modifiers == event::KeyModifiers::CONTROL)
                {
                    let result = self.publish_focused_app_document(app).and_then(|()| {
                        if app.open_document().is_some_and(|doc| doc.is_dirty()) {
                            app.save_open_document()
                                .map_err(|error| error.to_string())?;
                        }
                        Ok(())
                    });
                    if let Err(error) = result {
                        self.show_result = false;
                        self.output_errors.push_str(&format!("Save: {error}\n"));
                    }
                    continue;
                }
                let supported = command.as_ref().is_some_and(|h| h.supports_direct_script());
                let action = if self.picker.is_some() {
                    let action = self.picker_event(
                        &input,
                        sources.as_deref().unwrap_or(&fallback_sources),
                        supported,
                    );
                    if action.is_none() {
                        continue;
                    }
                    action
                } else if self.command_line.is_none() {
                    match &input {
                        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                            _ if WorkspaceTui::document_shortcut(*key).is_some() => {
                                WorkspaceTui::document_shortcut(*key)
                            }
                            KeyCode::F(5) => self.primary_f5_action(
                                key.modifiers,
                                sources.as_deref().unwrap_or(&fallback_sources),
                            ),

                            KeyCode::Left if key.modifiers.contains(event::KeyModifiers::ALT) => {
                                Some(WorkspaceAction::PreviousResult)
                            }
                            KeyCode::Right if key.modifiers.contains(event::KeyModifiers::ALT) => {
                                Some(WorkspaceAction::NextResult)
                            }
                            _ => None,
                        },
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(action) = action {
                    if let Err(error) = self.invoke_action(
                        action,
                        app,
                        sources.as_deref_mut().unwrap_or(&mut fallback_sources),
                        command
                            .as_mut()
                            .map(|h| &mut **h as &mut dyn ReadOnlyCommand),
                    ) {
                        self.show_result = false;
                        self.output_errors.push_str(&format!("{error}\n"));
                    }
                    continue;
                }
                if self.command_line.is_some() {
                    let submitted = if self.open_app_prompt || self.open_copy_prompt {
                        self.open_document_event(&input, app)
                    } else {
                        self.command_event(&input)
                    };
                    if let Some(line) = submitted {
                        if self.pending_prompt.is_some() {
                            if let Err(error) = self.submit_prompt(
                                line,
                                app,
                                sources.as_deref_mut().unwrap_or(&mut fallback_sources),
                                command
                                    .as_mut()
                                    .map(|h| &mut **h as &mut dyn ReadOnlyCommand),
                            ) {
                                self.show_result = false;
                                self.output_errors.push_str(&format!("{error}\n"));
                            }
                            continue;
                        }
                        #[cfg(feature = "agent")]
                        let mut background = None;
                        if let Some(status) = self.document_pending(
                            &line,
                            sources.as_deref().unwrap_or(&fallback_sources),
                        ) {
                            // Flush an honest waiting frame before synchronous dispatch blocks input.
                            terminal.as_mut().unwrap().draw(|frame| {
                                self.render_document_pending(
                                    frame,
                                    app,
                                    sources.as_deref(),
                                    &status,
                                );
                                #[cfg(feature = "agent")]
                                {
                                    background = Some(frame.buffer_mut().clone());
                                }
                            })?;
                        }
                        #[cfg(feature = "agent")]
                        let mut draw_error = None;
                        self.submit_command_inner(
                            &line,
                            app,
                            session,
                            command
                                .as_mut()
                                .map(|host| &mut **host as &mut dyn ReadOnlyCommand),
                            sources.as_deref_mut().unwrap_or(&mut fallback_sources),
                            #[cfg(feature = "agent")]
                            &mut |turn| {
                                if draw_error.is_some() {
                                    return;
                                }
                                if let Err(error) = terminal.as_mut().unwrap().draw(|frame| {
                                    if let Some(buffer) = &background {
                                        frame.buffer_mut().merge(buffer);
                                    }
                                    progress::render(frame, turn);
                                }) {
                                    draw_error = Some(error);
                                }
                            },
                        );
                        #[cfg(feature = "agent")]
                        if let Some(error) = draw_error {
                            return Err(error.into());
                        }
                    }
                    continue;
                }
                match self.handle_event_optional_sources(input, app, sources.as_deref_mut()) {
                    WorkspaceInput::Quit => {
                        if self.has_unsaved_work(app, sources.as_deref()) {
                            quit_pending = true;
                        } else {
                            break;
                        }
                    }
                    WorkspaceInput::Shell => {
                        if !matches!(terminal.as_ref(), Some(WorkspaceSurface::Terminal(_))) {
                            continue;
                        }
                        let Some(legacy) = &self.legacy else {
                            self.output_errors.push_str(
                                "Shell handoff is unavailable for this typed workspace.\n",
                            );
                            self.show_result = false;
                            continue;
                        };
                        drop(terminal.take());
                        let command = command
                            .as_mut()
                            .map(|host| &mut **host as &mut dyn ReadOnlyCommand);
                        (legacy.shell)(app, session, command, sources.as_deref_mut())?;
                        terminal = Some(WorkspaceSurface::terminal()?);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn has_unsaved_work(
        &self,
        app: &AppState<P>,
        sources: Option<&crate::scratch::ScratchBuffers>,
    ) -> bool {
        self.draft_blocker().is_some()
            || app.has_unsaved_documents()
            || sources.is_some_and(|sources| sources.list().any(|source| source.is_dirty()))
    }
}

enum WorkspaceSurface {
    Terminal(TerminalSession),
    #[cfg(feature = "desktop")]
    Desktop(Box<super::desktop::DesktopIo>),
}

enum SurfaceInput {
    Idle,
    #[cfg(feature = "desktop")]
    Closed,
    Event(Event),
    #[cfg(feature = "desktop")]
    Desktop(super::desktop::DesktopInput),
}

impl WorkspaceSurface {
    fn terminal() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::Terminal(TerminalSession::enter()?))
    }

    fn draw(&mut self, render: impl FnOnce(&mut ratatui::Frame<'_>)) -> std::io::Result<()> {
        match self {
            Self::Terminal(terminal) => terminal.draw(render),
            #[cfg(feature = "desktop")]
            Self::Desktop(desktop) => desktop.draw(render),
        }
    }

    fn read(&mut self) -> std::io::Result<SurfaceInput> {
        match self {
            Self::Terminal(_) if event::poll(std::time::Duration::from_millis(250))? => {
                Ok(SurfaceInput::Event(event::read()?))
            }
            Self::Terminal(_) => Ok(SurfaceInput::Idle),
            #[cfg(feature = "desktop")]
            Self::Desktop(desktop) => {
                use super::desktop::DesktopInput;
                match desktop.recv() {
                    Err(_) => Ok(SurfaceInput::Closed),
                    Ok(DesktopInput::Resize(width, height)) => {
                        desktop.resize(width, height);
                        Ok(SurfaceInput::Idle)
                    }
                    Ok(DesktopInput::Event(event)) => Ok(SurfaceInput::Event(event)),
                    Ok(input) => Ok(SurfaceInput::Desktop(input)),
                }
            }
        }
    }
}

#[cfg(test)]
mod provider_runtime_tests {
    use super::*;

    #[test]
    fn default_workspace_keeps_legacy_dispatch_but_typed_constructor_never_substitutes_an_owner() {
        let root = crate::test_support::TempWorkspace::new("typed-workspace-legacy-boundary");
        root.write_file("main.txt", "original");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("main.txt").unwrap();
        let identity = app.open_document().unwrap().evaluation_source_id().clone();
        assert!(WorkspaceTui::new().legacy.is_some());
        let mut ui = WorkspaceTui::<DefaultProvider>::for_provider();
        assert!(ui.legacy.is_none());
        let mut session = InteractiveSession::new();
        let mut sources = crate::scratch::ScratchBuffers::default();
        ui.submit_command(":scope files", &mut app, &mut session, None, &mut sources);
        assert!(
            ui.output_errors
                .contains("legacy :scope dispatch is unavailable")
        );
        assert_eq!(
            app.open_document().unwrap().evaluation_source_id(),
            &identity
        );
        assert_eq!(app.open_document().unwrap().text(), "original");
        assert!(ui.output.bytes().is_empty());
    }

    #[cfg(feature = "experimental-sprout-closure")]
    #[test]
    fn prepared_provider_needs_no_default_or_output_formatting_bounds() {
        use crate::registered_document_closure::PreparedProvider;
        struct Host;
        struct Evidence;
        let ui = WorkspaceTui::<PreparedProvider<Host, Evidence>>::for_provider();
        assert!(ui.legacy.is_none());
    }
}
