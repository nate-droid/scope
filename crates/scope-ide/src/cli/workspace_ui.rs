//! Normal executable composition; terminal/input ownership stays in WorkspaceTui.
use super::ReadOnlyCommand;
use crate::{
    app::App,
    language::InteractiveSession,
    scratch::{BoundSource, ScratchBuffers},
    tui::{EditableScratchPane, Pane, TuiContext, View, WorkspaceTui},
};

/// Optional downstream startup values. Scope retains source, input, and proposal ownership.
/// Pane IDs in `layout` and `focus` refer to `editor` and `inspector`.
pub struct WorkspaceConfig {
    pub language: String,
    pub content: String,
    /// Existing workspace-relative file; opened through normal workspace authority.
    /// Takes precedence over language/content and never writes the file.
    pub open_path: Option<String>,
    pub bind_selected: bool,
    pub focus: String,
    pub layout: Option<View>,
    /// Inspector with focused read-only view input; source-edit and legacy mutable
    /// callbacks are not forwarded. Global shortcuts remain owned by Scope.
    pub inspector: Option<Box<dyn Pane>>,
}
impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            language: "text".into(),
            content: String::new(),
            open_path: None,
            bind_selected: false,
            focus: "editor".into(),
            layout: None,
            inspector: None,
        }
    }
}
/// Forwards presentation and read-only view input, never mutable pane callbacks.
struct SuppliedInspector(Box<dyn Pane>);
impl Pane for SuppliedInspector {
    fn id(&self) -> &'static str {
        self.0.id()
    }
    fn render(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: ratatui::layout::Rect,
        ctx: &TuiContext<'_>,
    ) {
        self.0.render(frame, area, ctx)
    }

    fn workspace_document_request(
        &mut self,
        event: &crossterm::event::Event,
        ctx: &TuiContext<'_>,
    ) -> Option<String> {
        self.0.workspace_document_request(event, ctx)
    }

    fn handle_view_event(&mut self, event: crossterm::event::Event, ctx: &TuiContext<'_>) -> bool {
        self.0.handle_view_event(event, ctx)
    }
}

/// Presentation only: no input callback, editing capability or execution access.
struct Inspector;
impl Pane for Inspector {
    fn id(&self) -> &'static str {
        "workspace-inspector"
    }

    fn render(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: ratatui::layout::Rect,
        ctx: &TuiContext<'_>,
    ) {
        let mut text = String::from(
            "Read-only inspector — committed source, not editor drafts.\n\n\
             Editor follows scratch selection; semantic binding is independent.\n\
             Ctrl-S commits in memory; Ctrl-C cancels the local draft.\n\n\
             F1 actions: New Script scratch, focus panes, help.\n\
             F5 runs committed selected Script (all drafts must be clean).\n\
             F3 previous pane; Alt-Left/Right result history.\n\
             F4 commands; F6 focus; F7 layout; F2 shell; Ctrl-Q exit.\n\n\
             Pure Script has no native authority. Semantic operations require\n\
             explicit host installation, target binding and authorization.\n\
             Nothing is evaluated or saved on startup.\n\n",
        );
        if let Some(selected) = ctx.selected_source() {
            text.push_str(&format!(
                "Selected scratch {}\nSource: {:?}\nRevision: {:?}\nCommitted text:\n{}\n\n",
                selected.scratch_id,
                selected.source.source_id,
                selected.source.revision_id,
                selected.source.text,
            ));
        } else {
            text.push_str("No selected scratch.\n\n");
        }
        match ctx.bound_semantic_source() {
            Some(BoundSource::Available(source)) => text.push_str(&format!(
                "Semantic target: {:?} revision {:?}\n",
                source.source_id, source.revision_id,
            )),
            Some(BoundSource::Unavailable { scratch_id }) => {
                text.push_str(&format!("Semantic target {scratch_id} is closed.\n"));
            }
            _ => text.push_str("Semantic target: unbound (no implicit selection fallback).\n"),
        }
        frame.render_widget(
            ratatui::widgets::Paragraph::new(text)
                .scroll((ctx.scroll(), 0))
                .wrap(ratatui::widgets::Wrap { trim: false }),
            area,
        );
    }
}

fn configured_sources(
    app: &App,
    config: &WorkspaceConfig,
) -> Result<ScratchBuffers, Box<dyn std::error::Error>> {
    let mut sources = ScratchBuffers::default();
    let id = if let Some(path) = &config.open_path {
        sources.open(app.workspace(), app.languages(), path)?
    } else {
        sources.create(&config.language, &config.content)
    };
    if config.bind_selected {
        sources.bind_semantic_target(Some(id))?;
    }
    Ok(sources)
}

#[cfg(test)]
pub(super) fn build() -> Result<WorkspaceTui, Box<dyn std::error::Error>> {
    build_configured(WorkspaceConfig::default())
}

fn build_configured(
    mut config: WorkspaceConfig,
) -> Result<WorkspaceTui, Box<dyn std::error::Error>> {
    let mut ui = WorkspaceTui::new();
    ui.register_editable("editor", EditableScratchPane::new())?;
    ui.register(
        "inspector",
        SuppliedInspector(
            config
                .inspector
                .take()
                .unwrap_or_else(|| Box::new(Inspector)),
        ),
    )?;
    ui.layout(
        "workspace",
        config
            .layout
            .take()
            .unwrap_or_else(|| View::horizontal([View::leaf("editor"), View::leaf("inspector")])),
    )?;
    ui.layout("editor", View::leaf("editor"))?;
    ui.select_layout("workspace")?;
    ui.focus_initial(&config.focus)?;
    Ok(ui)
}

#[cfg(test)]
fn build_with_actions(
    actions: impl IntoIterator<Item = crate::tui::DownstreamAction>,
) -> Result<WorkspaceTui, Box<dyn std::error::Error>> {
    let mut ui = build()?;
    for action in actions {
        ui.register_downstream_action(action)?;
    }
    Ok(ui)
}

pub(super) fn run(
    app: &mut App,
    host: Option<&mut dyn ReadOnlyCommand>,
    #[cfg(feature = "agent")] editing: Option<super::InteractiveEditConfig>,
    #[cfg(feature = "agent")] agent_config: super::AgentConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = host
        .as_deref()
        .and_then(|host| host.workspace_config())
        .unwrap_or_default();
    let mut session = InteractiveSession::new();
    let mut sources = configured_sources(app, &config)?;
    let actions = host
        .as_deref()
        .map(|host| host.downstream_actions())
        .unwrap_or_default();
    let mut ui = build_configured(config)?;
    for action in actions {
        ui.register_downstream_action(action)?;
    }
    #[cfg(feature = "agent")]
    ui.configure_commands(editing, agent_config)?;
    ui.run_with_sources(app, &mut session, host, &mut sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct InputInspector(std::rc::Rc<std::cell::RefCell<Vec<crossterm::event::Event>>>);

    impl Pane for InputInspector {
        fn id(&self) -> &'static str {
            "supplied-input-inspector"
        }

        fn render(
            &self,
            _frame: &mut ratatui::Frame<'_>,
            _area: ratatui::layout::Rect,
            _ctx: &TuiContext<'_>,
        ) {
        }

        fn handle_view_event(
            &mut self,
            event: crossterm::event::Event,
            ctx: &TuiContext<'_>,
        ) -> bool {
            assert!(ctx.is_focused());
            assert_eq!(
                ctx.selected_source().unwrap().source.text.as_ref(),
                "retained source"
            );
            assert!(matches!(
                ctx.bound_semantic_source(),
                Some(BoundSource::Available(_))
            ));
            let consumed = matches!(event, crossterm::event::Event::Paste(_));
            self.0.borrow_mut().push(event);
            consumed
        }

        fn handle_source_event(
            &mut self,
            _event: crossterm::event::Event,
            _access: &mut crate::tui::ScratchEditAccess<'_>,
        ) -> bool {
            panic!("supplied inspector must not receive source-edit access");
        }

        fn handle_key(
            &mut self,
            _key: crossterm::event::KeyEvent,
            _ctx: &mut crate::tui::TuiContextMut<'_>,
        ) -> bool {
            panic!("supplied inspector must not receive legacy mutable access");
        }
    }

    #[test]
    fn configured_supplied_inspector_receives_only_focused_read_only_input() {
        use crate::tui::WorkspaceInput;
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

        let root = crate::test_support::TempWorkspace::new("supplied-inspector-input");
        let mut app = App::new(root.workspace()).unwrap();
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let config = WorkspaceConfig {
            content: "retained source".into(),
            bind_selected: true,
            focus: "inspector".into(),
            inspector: Some(Box::new(InputInspector(events.clone()))),
            ..WorkspaceConfig::default()
        };
        let mut sources = configured_sources(&app, &config).unwrap();
        let before_text = sources.selected().unwrap().text().to_owned();
        let before_version = sources.selected().unwrap().version();
        let mut ui = build_configured(config).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
        terminal
            .draw(|frame| ui.render_with_sources(frame, frame.area(), &app, &sources))
            .unwrap();
        let paste = Event::Paste(":scope save\n雪".into());
        let key = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            ui.handle_event_with_sources(paste.clone(), &mut app, &mut sources),
            WorkspaceInput::Handled
        );
        // A declined event must not fall through to the supplied mutable callback.
        assert_eq!(
            ui.handle_event_with_sources(key.clone(), &mut app, &mut sources),
            WorkspaceInput::Ignored
        );
        assert_eq!(*events.borrow(), vec![paste.clone(), key.clone()]);
        assert_eq!(sources.selected().unwrap().text(), before_text);
        assert_eq!(sources.selected().unwrap().version(), before_version);
        ui.focus_named("editor").unwrap();
        ui.handle_event_with_sources(key, &mut app, &mut sources);
        assert_eq!(events.borrow().len(), 2);
        ui.focus_named("inspector").unwrap();
        for (code, modifiers, expected) in [
            (KeyCode::F(1), KeyModifiers::NONE, WorkspaceInput::Handled),
            (KeyCode::F(4), KeyModifiers::NONE, WorkspaceInput::Handled),
            (KeyCode::F(2), KeyModifiers::NONE, WorkspaceInput::Shell),
            (
                KeyCode::Char('q'),
                KeyModifiers::CONTROL,
                WorkspaceInput::Quit,
            ),
        ] {
            assert_eq!(
                ui.handle_event_with_sources(
                    Event::Key(KeyEvent::new(code, modifiers)),
                    &mut app,
                    &mut sources
                ),
                expected
            );
        }
        assert_eq!(events.borrow().len(), 2);
        assert_eq!(sources.selected().unwrap().text(), before_text);
        assert_eq!(sources.selected().unwrap().version(), before_version);
    }

    #[test]
    fn downstream_startup_uses_owned_sources_and_existing_layout_validation() {
        let root = crate::test_support::TempWorkspace::new("configured-workspace");
        root.write_file("existing.txt", "retained file");
        let app = App::new(root.workspace()).unwrap();
        let config = WorkspaceConfig {
            language: "demo".into(),
            content: "starter".into(),
            bind_selected: true,
            focus: "inspector".into(),
            layout: Some(View::vertical([
                View::leaf("editor"),
                View::leaf("inspector"),
            ])),
            ..WorkspaceConfig::default()
        };
        let sources = configured_sources(&app, &config).unwrap();
        assert_eq!(sources.selected().unwrap().language_id(), "demo");
        assert_eq!(sources.selected().unwrap().text(), "starter");
        assert!(sources.semantic_target().unwrap().is_some());
        assert_eq!(
            build_configured(config).unwrap().focused(),
            Some("inspector")
        );
        let mut config = WorkspaceConfig {
            open_path: Some("existing.txt".into()),
            content: "never replace".into(),
            bind_selected: true,
            ..WorkspaceConfig::default()
        };
        let sources = configured_sources(&app, &config).unwrap();
        assert_eq!(sources.selected().unwrap().text(), "retained file");
        assert_eq!(sources.selected().unwrap().language_id(), "text");
        assert_eq!(root.read_file("existing.txt"), "retained file");
        config.open_path = Some("missing.txt".into());
        assert!(configured_sources(&app, &config).is_err());
        assert!(!root.host_path_exists("missing.txt"));
        config.focus = "absent".into();
        assert!(build_configured(config).is_err());
        assert!(
            build_configured(WorkspaceConfig {
                layout: Some(View::leaf("unknown")),
                ..WorkspaceConfig::default()
            })
            .is_err()
        );
    }

    #[test]
    fn launcher_registration_uses_existing_bounds_and_duplicate_validation() {
        use crate::tui::DownstreamAction;
        let action = DownstreamAction {
            id: "demo-inspect".into(),
            label: "Inspect demo".into(),
            language: "demo".into(),
            namespace: "demo".into(),
            command: "inspect".into(),
            args: vec![],
        };
        assert!(build_with_actions([]).is_ok());
        assert!(build_with_actions([action.clone()]).is_ok());
        assert!(build_with_actions([action.clone(), action.clone()]).is_err());
        for invalid in [
            DownstreamAction {
                id: "INVALID".into(),
                ..action.clone()
            },
            DownstreamAction {
                label: "x".repeat(257),
                ..action.clone()
            },
            DownstreamAction {
                language: "x".repeat(65),
                ..action.clone()
            },
            DownstreamAction {
                args: vec!["x".into(); 33],
                ..action.clone()
            },
            DownstreamAction {
                args: vec!["x".repeat(1025)],
                ..action.clone()
            },
            DownstreamAction {
                args: vec!["two words".into()],
                ..action.clone()
            },
        ] {
            assert!(build_with_actions([invalid]).is_err());
        }
        let actions = (0..65).map(|i| DownstreamAction {
            id: format!("demo-{i}"),
            ..action.clone()
        });
        assert!(build_with_actions(actions).is_err());
    }

    #[test]
    fn workspace_command_parses_with_explicit_root_and_rejects_extra_arguments() {
        use super::super::{Args, Command, ParseResult};
        let ParseResult::Run(args) =
            Args::parse(["workspace", "some-root"].map(str::to_owned)).unwrap()
        else {
            panic!("expected workspace command");
        };
        assert!(matches!(args.command, Command::Workspace));
        assert_eq!(args.workspace_root, "some-root");
        assert!(!args.writable);
        assert!(Args::parse(["workspace", "some-root", "extra"].map(str::to_owned)).is_err());
    }

    #[test]
    fn executable_workspace_starts_with_selected_editable_text_and_no_semantic_binding() {
        let root = crate::test_support::TempWorkspace::new("executable-workspace");
        let mut app = App::new(root.workspace()).unwrap();
        let mut sources = configured_sources(&app, &WorkspaceConfig::default()).unwrap();
        let id = sources.selected().unwrap().id();
        assert_eq!(sources.selected().unwrap().language_id(), "text");
        assert_eq!(sources.selected().unwrap().text(), "");
        assert!(sources.semantic_target().unwrap().is_none());
        let mut ui = build().unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
        terminal
            .draw(|frame| ui.render_with_sources(frame, frame.area(), &app, &sources))
            .unwrap();
        assert_eq!(ui.focused(), Some("editor"));
        ui.handle_event_with_sources(
            crossterm::event::Event::Paste("雪😀".into()),
            &mut app,
            &mut sources,
        );
        assert_eq!(sources.get(id).unwrap().text(), "");
        ui.handle_event_with_sources(
            crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('s'),
                crossterm::event::KeyModifiers::CONTROL,
            )),
            &mut app,
            &mut sources,
        );
        assert_eq!(sources.get(id).unwrap().text(), "雪😀");
        ui.focus_named("inspector").unwrap();
        ui.handle_event_with_sources(
            crossterm::event::Event::Paste("must not edit".into()),
            &mut app,
            &mut sources,
        );
        assert_eq!(sources.get(id).unwrap().text(), "雪😀");
        assert!(sources.semantic_target().unwrap().is_none());
        ui.select_layout("editor").unwrap();
        terminal
            .draw(|frame| ui.render_with_sources(frame, frame.area(), &app, &sources))
            .unwrap();
    }
}
