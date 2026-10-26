//! cargo run -p scope-ide --features tui --example editable_workspace
use crossterm::event::{Event, KeyCode};
use ratatui::{layout::Rect, widgets::Paragraph};
use scope_ide::{
    app::App,
    language::InteractiveSession,
    scratch::{BoundSource, ScratchBuffers},
    tui::{EditableScratchPane, Pane, TuiContext, View, WorkspaceTui},
    workspace::Workspace,
};
use std::error::Error;

#[derive(Default)]
struct ObserverPane {
    // Immutable reports retain their source identity/revision after later commits.
    reports: Vec<String>,
}
impl Pane for ObserverPane {
    fn id(&self) -> &'static str {
        "scratch-observer"
    }

    fn handle_view_event(&mut self, event: Event, ctx: &TuiContext<'_>) -> bool {
        if !matches!(event, Event::Key(key) if key.code == KeyCode::F(5)) {
            return false;
        }
        // This host authorizes only a local, read-only metadata operation.
        // No interpreter, shell, file access, or semantic execution is involved.
        let report = match ctx.bound_semantic_source() {
            Some(BoundSource::Available(source)) => format!(
                "Metadata result: {:?} {:?}: {} chars, {} lines",
                source.source_id,
                source.revision_id,
                source.text.chars().count(),
                source.text.lines().count(),
            ),
            _ => "Metadata unavailable: bind a live scratch first".into(),
        };
        self.reports.push(report);
        true
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        let mut text = String::from(
            "Committed semantic source (read-only observer)\n\n\
             Ctrl-S: commit draft; Ctrl-C: cancel draft\n\
             Esc/F2: shell; :scope quit: return here\n\
             F6: focus; F7: layout; Ctrl-Q: exit workspace\n\
             F5 here: authorized read-only source metadata\n\n\
             Drafts survive layout changes and shell handoffs.\n\
             This observer changes only after a commit.\n\
             Shell edits/rebinding can conflict with a retained draft.\n\
             Ctrl-C in the editor cancels; edit again to restart.\n\n\
             In the shell, try:\n\
             :scope show\n\
             :scope line 1 \"changed in the shell\"\n\
             :scope quit\n\n",
        );
        match ctx.bound_semantic_source() {
            None => text.push_str("No source owner supplied."),
            Some(BoundSource::Unbound) => text.push_str("No semantic source bound."),
            Some(BoundSource::Unavailable { scratch_id }) => {
                text.push_str(&format!(
                    "Bound scratch {scratch_id} has closed; rebind or unbind."
                ));
            }
            Some(BoundSource::Available(snapshot)) => {
                text.push_str(&format!(
                    "Source: {:?}\nRevision: {:?}\n\n{}",
                    snapshot.source_id, snapshot.revision_id, snapshot.text,
                ));
            }
        }
        if let Some(report) = self.reports.last() {
            text.insert_str(
                0,
                &format!("{report}\nReports retained: {}\n\n", self.reports.len()),
            );
        }
        frame.render_widget(Paragraph::new(text), area);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::read_only(env!("CARGO_MANIFEST_DIR"))?;
    let mut app = App::builder(workspace).build()?;
    let mut session = InteractiveSession::new();
    // The editor and normal shell borrow this same caller-owned source collection.
    let mut sources = ScratchBuffers::default();
    let id = sources.create("text", "Edit this unsaved text.\nUnicode: 猫 é\n");
    sources.bind_semantic_target(Some(id))?;

    let mut tui = WorkspaceTui::new();
    tui.register_editable("editor", EditableScratchPane::new())?;
    tui.register("observer", ObserverPane::default())?;
    tui.layout(
        "side-by-side",
        View::horizontal([View::leaf("editor"), View::leaf("observer")]),
    )?;
    tui.layout(
        "stacked",
        View::vertical([View::leaf("editor"), View::leaf("observer")]),
    )?;
    tui.select_layout("side-by-side")?;
    tui.run_with_sources(&mut app, &mut session, None, &mut sources)?;
    println!(
        "Retained binding after workspace exit: {:?}",
        sources.bound_source()
    );
    Ok(())
}
