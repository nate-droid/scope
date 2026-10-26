//! cargo run -p scope-ide --features tui --example composable_workspace
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, widgets::Paragraph};
use scope_ide::{
    app::App,
    cli::{DownstreamError, DownstreamOutcome, ReadOnlyCommand},
    document::Document,
    language::InteractiveSession,
    scratch::ScratchBuffer,
    text::TextRange,
    tui::{Pane, TuiContext, TuiContextMut, View, WorkspaceTui},
    workspace::Workspace,
};
use std::{cell::RefCell, error::Error, io::Write, rc::Rc};

struct Domain {
    rows: Vec<&'static str>,
}

// All four instances share domain data but retain independent local selection.
struct SyntheticView {
    domain: Rc<RefCell<Domain>>,
    label: &'static str,
    selected: usize,
}
impl Pane for SyntheticView {
    fn id(&self) -> &'static str {
        "synthetic-list"
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        let domain = self.domain.borrow();
        let mut text = format!(
            "{} • {}\nUp/Down or j/k: local selection\n",
            self.label,
            if ctx.is_focused() {
                "focused"
            } else {
                "unfocused"
            }
        );
        for (index, row) in domain.rows.iter().enumerate() {
            text.push_str(&format!(
                "{} {row}\n",
                if index == self.selected { ">" } else { " " }
            ));
        }
        frame.render_widget(Paragraph::new(text), area);
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut TuiContextMut<'_>) -> bool {
        let count = self.domain.borrow().rows.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            _ => return false,
        }
        true
    }
}

struct DemoHost(Rc<RefCell<Domain>>);
impl ReadOnlyCommand for DemoHost {
    fn downstream_namespaces(&self) -> &[&str] {
        &["demo"]
    }

    fn invoke_downstream(
        &mut self,
        namespace: &str,
        command: &str,
        args: &[&str],
        _semantic_target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<DownstreamOutcome, DownstreamError> {
        if namespace != "demo" || command != "status" {
            return Ok(DownstreamOutcome::Unhandled);
        }
        if !args.is_empty() {
            return Err(DownstreamError::new(
                "invalid_arguments",
                "usage: :scope downstream demo status",
            ));
        }
        writeln!(
            output,
            "demo: {} synthetic rows; no execution adapter installed",
            self.0.borrow().rows.len()
        )
        .map_err(|error| DownstreamError::new("output_error", error.to_string()))?;
        Ok(DownstreamOutcome::Handled)
    }

    fn invoke(
        &mut self,
        _document: &Document,
        _selection: TextRange,
        _operation: &str,
        _output: &mut dyn Write,
    ) -> Result<(), Box<dyn Error>> {
        Err("demo supports only :scope downstream demo status".into())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Public construction, no temporary files and no write authority.
    let workspace = Workspace::read_only(env!("CARGO_MANIFEST_DIR"))?;
    let mut app = App::builder(workspace).build()?;
    let mut session = InteractiveSession::new();
    let domain = Rc::new(RefCell::new(Domain {
        rows: vec!["alpha (synthetic)", "beta (synthetic)", "gamma (synthetic)"],
    }));
    let mut tui = WorkspaceTui::new();
    for (id, label) in [
        ("overview", "Overview"),
        ("tasks", "Tasks"),
        ("events", "Events"),
        ("details", "Details"),
    ] {
        tui.register(
            id,
            SyntheticView {
                domain: domain.clone(),
                label,
                selected: 0,
            },
        )?;
    }
    tui.layout(
        "2x2",
        View::vertical([
            View::horizontal([View::leaf("overview"), View::leaf("tasks")]),
            View::horizontal([View::leaf("events"), View::leaf("details")]),
        ]),
    )?;
    tui.layout("focused", View::leaf("details"))?;
    tui.select_layout("2x2")?;
    let mut host = DemoHost(domain.clone());
    tui.run(&mut app, &mut session, Some(&mut host))?;
    println!(
        "Host retained {} synthetic rows.",
        domain.borrow().rows.len()
    );
    Ok(())
}
