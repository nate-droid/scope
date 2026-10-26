//! Normal executable composition; terminal/input ownership stays in WorkspaceTui.
use super::ReadOnlyCommand;
use crate::{
    app::App,
    language::InteractiveSession,
    scratch::{BoundSource, ScratchBuffers},
    tui::{EditableScratchPane, Pane, TuiContext, View, WorkspaceTui},
};

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

fn initial_sources() -> ScratchBuffers {
    let mut sources = ScratchBuffers::default();
    sources.create("text", "");
    sources
}

pub(super) fn build() -> Result<WorkspaceTui, Box<dyn std::error::Error>> {
    let mut ui = WorkspaceTui::new();
    ui.register_editable("editor", EditableScratchPane::new())?;
    ui.register("inspector", Inspector)?;
    ui.layout(
        "workspace",
        View::horizontal([View::leaf("editor"), View::leaf("inspector")]),
    )?;
    ui.layout("editor", View::leaf("editor"))?;
    ui.select_layout("workspace")?;
    Ok(ui)
}

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
    let mut session = InteractiveSession::new();
    let mut sources = initial_sources();
    let actions = host
        .as_deref()
        .map(|host| host.downstream_actions())
        .unwrap_or_default();
    let mut ui = build_with_actions(actions)?;
    #[cfg(feature = "agent")]
    ui.configure_commands(editing, agent_config)?;
    ui.run_with_sources(app, &mut session, host, &mut sources)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut sources = initial_sources();
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
