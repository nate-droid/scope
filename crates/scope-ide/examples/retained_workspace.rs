//! cargo run -p scope-ide --features tui --example retained_workspace
use ratatui::{layout::Rect, widgets::Paragraph};
use scope_ide::{
    app::App,
    language::InteractiveSession,
    scratch::{BoundSource, ScratchBuffers},
    tui::{Pane, TuiContext, View, WorkspaceTui},
    workspace::Workspace,
};
use std::error::Error;

struct SnapshotPane;
impl Pane for SnapshotPane {
    fn id(&self) -> &'static str {
        "bound-snapshot"
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        let mut text = String::from(
            "Retained semantic source (read-only snapshot)\n\n\
             F2: shell; :scope quit: return here; Ctrl-Q: exit workspace\n\
             In the shell, try:\n\
             :scope scratch new text\n\
             :scope line 1 \"hello from an unsaved source\"\n\
             :scope semantic-target 0\n\
             :scope quit\n\n\
             Repeat F2 / line edits / :scope quit to see new revisions.\n\
             Selecting another scratch does not change this binding.\n\
             :scope discard closes the selected scratch without writing.\n\
             :scope semantic-target none explicitly unbinds.\n\n",
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
        frame.render_widget(Paragraph::new(text), area);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::read_only(env!("CARGO_MANIFEST_DIR"))?;
    let mut app = App::builder(workspace).build()?;
    let mut session = InteractiveSession::new();
    // One caller-owned collection survives every workspace/shell handoff.
    let mut sources = ScratchBuffers::default();
    let mut tui = WorkspaceTui::new();
    tui.register("snapshot", SnapshotPane)?;
    tui.layout("snapshot", View::leaf("snapshot"))?;
    tui.select_layout("snapshot")?;
    tui.run_with_sources(&mut app, &mut session, None, &mut sources)?;
    println!(
        "Retained binding after workspace exit: {:?}",
        sources.bound_source()
    );
    Ok(())
}
