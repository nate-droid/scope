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
#[cfg(test)]
mod tests;
use super::{Pane, TuiContext, TuiContextMut, session::TerminalSession};
use crate::{app::App, cli::ReadOnlyCommand, language::InteractiveSession};
pub use actions::{ActionInfo, DownstreamAction, WorkspaceAction};
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
    fn validate(
        &self,
        instances: &BTreeMap<String, Instance>,
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

struct Instance {
    pane: Box<dyn Pane>,
    scroll: u16,
    editable: bool,
}
/// Host-owned instances survive layout replacement, including hidden instances.
/// Instance keys are independent of `Pane::id()` (the view type). No capability
/// or execution context is created by registration or rendering.
#[derive(Default)]
pub struct WorkspaceTui {
    instances: BTreeMap<String, Instance>,
    layouts: Vec<(String, View)>,
    active: usize,
    focused: Option<String>,
    last_focused: Option<String>,
    command_line: Option<String>,
    output: crate::cli::workspace_command::WorkspaceCommandOutput,
    output_errors: String,
    output_scroll: u16,
    output_area: Option<Rect>,
    visible: Vec<(String, Rect)>,
    picker: Option<actions::Picker>,
    downstream_actions: Vec<DownstreamAction>,
    results: Vec<crate::cli::ScriptPresentation>,
    result_index: Option<usize>,
    show_result: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum WorkspaceInput {
    Handled,
    Ignored,
    Shell,
    Quit,
}
impl WorkspaceTui {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(
        &mut self,
        id: impl Into<String>,
        pane: impl Pane + 'static,
    ) -> Result<(), String> {
        let id = id.into();
        if id.is_empty() || id == "output" || self.instances.contains_key(&id) {
            return Err(format!("empty or duplicate instance: {id}"));
        }
        self.instances.insert(
            id,
            Instance {
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
        pane: impl Pane + 'static,
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

    /// Focus a visible instance, output, the previous pane, or a one-based
    /// position in the current visible layout (not registration order).
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
    pub fn render(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
        self.render_optional_sources(frame, area, app, None);
    }

    /// Renders panes with read-only access to the caller's semantic source binding.
    pub fn render_with_sources(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        app: &App,
        sources: &crate::scratch::ScratchBuffers,
    ) {
        self.render_optional_sources(frame, area, app, Some(sources));
    }

    fn render_optional_sources(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        app: &App,
        sources: Option<&crate::scratch::ScratchBuffers>,
    ) {
        self.geometry(area);
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
                &TuiContext::with_view_sources(app, focused, instance.scroll, sources),
            );
        }
    }
    /// Host shortcuts: F4 command bar; Esc/F2 shell; F6/Shift-F6 focus; F7 next layout;
    /// Ctrl-Q exit. All other keys (including Tab and q) belong to the pane.
    pub fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> WorkspaceInput {
        self.handle_event_optional_sources(Event::Key(key), app, None)
    }

    /// Routes host shortcuts first, then grants only the focused opted-in pane
    /// a short-lived editing capability. Paste is never interpreted as commands.
    pub fn handle_event_with_sources(
        &mut self,
        event: Event,
        app: &mut App,
        sources: &mut crate::scratch::ScratchBuffers,
    ) -> WorkspaceInput {
        self.handle_event_optional_sources(event, app, Some(sources))
    }

    fn handle_event_optional_sources(
        &mut self,
        input: Event,
        app: &mut App,
        mut sources: Option<&mut crate::scratch::ScratchBuffers>,
    ) -> WorkspaceInput {
        let key = match &input {
            Event::Key(key) => *key,
            Event::Paste(_) => KeyEvent::new(KeyCode::Null, event::KeyModifiers::NONE),
            _ => return WorkspaceInput::Ignored,
        };
        if key.kind != KeyEventKind::Press {
            return WorkspaceInput::Ignored;
        }
        match key.code {
            KeyCode::F(1) => {
                self.picker = Some(actions::Picker::default());
                return WorkspaceInput::Handled;
            }
            KeyCode::F(3) => {
                let _ = self.focus_named("last");
                return WorkspaceInput::Handled;
            }
            KeyCode::F(4) => {
                self.command_line = Some(String::new());
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
                let _ = self.focus_named(&ids[(old + step) % ids.len()]);
                return WorkspaceInput::Handled;
            }
            _ => {}
        }
        if self.focused.as_deref() == Some("output") {
            let page = self
                .output_area
                .map_or(1, |area| area.height.saturating_sub(2).max(1));
            match key.code {
                KeyCode::Up => self.output_scroll = self.output_scroll.saturating_sub(1),
                KeyCode::Down => {
                    self.output_scroll = self.output_scroll.saturating_add(1).min(
                        self.result_text(None)
                            .unwrap_or_else(|| self.output_text())
                            .lines()
                            .count()
                            .saturating_sub(1)
                            .min(u16::MAX as usize) as u16,
                    )
                }
                KeyCode::PageUp => self.output_scroll = self.output_scroll.saturating_sub(page),
                KeyCode::PageDown => {
                    self.output_scroll = self.output_scroll.saturating_add(page).min(
                        self.result_text(None)
                            .unwrap_or_else(|| self.output_text())
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
            let instance = self.instances.get_mut(id).unwrap();
            let inner = Block::default().borders(Borders::ALL).inner(*area);
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
                &TuiContext::with_view_sources(app, true, instance.scroll, sources.as_deref()),
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

    // Returns a submission only on Enter. All command editing is isolated from panes.
    fn command_event(&mut self, input: &Event) -> Option<String> {
        let line = self.command_line.as_mut()?;
        match input {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc | KeyCode::F(4) => {
                    self.command_line = None;
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
        app: &App,
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

    fn submit_command(
        &mut self,
        line: &str,
        app: &mut App,
        session: &mut InteractiveSession,
        host: Option<&mut dyn ReadOnlyCommand>,
        sources: &mut crate::scratch::ScratchBuffers,
    ) {
        self.show_result = false;
        let result = match PaneAction::parse(line) {
            Some(PaneAction::Focus(name)) => self.focus_named(&name),
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
                    self.output
                        .submit(line, app, session, host, sources)
                        .map_err(|e| e.to_string())
                }
            }
        };
        if let Err(error) = result {
            self.output_errors.push_str(&format!("{error}\n"));
        }
    }

    /// Owns the terminal and input loop; the caller retains app, session and host.
    pub fn run(
        &mut self,
        app: &mut App,
        session: &mut InteractiveSession,
        command: Option<&mut dyn ReadOnlyCommand>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.run_optional_sources(app, session, command, None)
    }

    /// Runs with caller-owned scratch buffers retained across shell handoffs.
    pub fn run_with_sources(
        &mut self,
        app: &mut App,
        session: &mut InteractiveSession,
        command: Option<&mut dyn ReadOnlyCommand>,
        sources: &mut crate::scratch::ScratchBuffers,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.run_optional_sources(app, session, command, Some(sources))
    }

    fn run_optional_sources(
        &mut self,
        app: &mut App,
        session: &mut InteractiveSession,
        mut command: Option<&mut dyn ReadOnlyCommand>,
        mut sources: Option<&mut crate::scratch::ScratchBuffers>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.layouts.is_empty() {
            return Err("register at least one layout".into());
        }
        let mut fallback_sources = crate::scratch::ScratchBuffers::default();
        let mut terminal = Some(TerminalSession::enter()?);
        loop {
            terminal.as_mut().unwrap().draw(|frame| {
                let output_height = (frame.area().height / 3).clamp(1, 8);
                let areas = Layout::vertical([
                    Constraint::Min(0),
                    Constraint::Length(output_height),
                    Constraint::Length(1),
                ])
                .split(frame.area());
                self.output_area = (areas[1].width > 0 && areas[1].height > 0).then_some(areas[1]);
                self.render_optional_sources(frame, areas[0], app, sources.as_deref());
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title("output")
                    .border_style(if self.focused.as_deref() == Some("output") {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default()
                    });
                frame.render_widget(
                    Paragraph::new(self.result_text(sources.as_deref()).unwrap_or_else(|| self.output_text()))
                        .block(block)
                        .scroll((self.output_scroll, 0)),
                    areas[1],
                );
                frame.render_widget(
                    Paragraph::new(
                        self.command_line
                            .as_ref()
                            .map(|s| format!("> {s}"))
                            .unwrap_or_else(|| {
                                "F1 actions • F5 run • F3 previous pane • F4 command • F6 focus • F7 layout • F2/Esc shell • Ctrl-Q exit"
                                    .into()
                            }),
                    ),
                    areas[2],
                );
                self.render_picker(frame, sources.as_deref().unwrap_or(&fallback_sources), command.as_ref().is_some_and(|h| h.supports_direct_script()));
            })?;
            if event::poll(std::time::Duration::from_millis(250))? {
                let input = event::read()?;
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
                            KeyCode::F(5) => Some(WorkspaceAction::RunScript),
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
                    if let Some(line) = self.command_event(&input) {
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
                            })?;
                        }
                        self.submit_command(
                            &line,
                            app,
                            session,
                            command
                                .as_mut()
                                .map(|host| &mut **host as &mut dyn ReadOnlyCommand),
                            sources.as_deref_mut().unwrap_or(&mut fallback_sources),
                        );
                    }
                    continue;
                }
                match self.handle_event_optional_sources(input, app, sources.as_deref_mut()) {
                    WorkspaceInput::Quit => break,
                    WorkspaceInput::Shell => {
                        drop(terminal.take());
                        let command = command
                            .as_mut()
                            .map(|host| &mut **host as &mut dyn ReadOnlyCommand);
                        let result = match sources.as_deref_mut() {
                            Some(sources) => crate::cli::run_terminal_borrowed_with_sources(
                                app, session, command, sources,
                            ),
                            None => crate::cli::run_terminal_borrowed(app, session, command),
                        };
                        result?;
                        terminal = Some(TerminalSession::enter()?);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}
