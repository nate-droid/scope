//! In-process graphical transport for the ordinary workspace loop.
use std::{io, sync::mpsc};

pub use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal,
    backend::{Backend, TestBackend},
};
pub use ratatui::{
    buffer::Buffer,
    layout::Position,
    style::{Color, Modifier},
};

use super::{TrustedClosureProvider, WorkspaceTui};
use crate::{
    app::{App, AppState},
    cli::{ReadOnlyCommand, workspace_ui},
    scratch::ScratchBuffers,
    workspace::Workspace,
};

pub enum DesktopInput {
    Resize(u16, u16),
    Event(Event),
    Open(String),
    CloseDocument(String),
    CycleDocument { backwards: bool },
    Copy { cut: bool },
    Close,
    ConfirmClose,
}

#[derive(Clone)]
pub struct DesktopTab {
    pub path: String,
    pub dirty: bool,
}

pub struct DesktopFrame {
    pub tabs: Vec<DesktopTab>,
    pub cells: Buffer,
    pub cursor: Option<Position>,
    pub document: Option<String>,
    pub unsaved: bool,
    pub draft: bool,
    pub published_unsaved: bool,
    pub confirm_close: bool,
}

pub enum DesktopUpdate {
    Frame(DesktopFrame),
    Files(Result<Vec<String>, String>),
    Clipboard(String),
}

pub(super) struct DesktopIo {
    input: mpsc::Receiver<DesktopInput>,
    publish: Box<dyn FnMut(DesktopUpdate)>,
    terminal: Terminal<TestBackend>,
    tabs: Vec<DesktopTab>,
    document: Option<String>,
    unsaved: bool,
    draft: bool,
    published_unsaved: bool,
    confirm_close: bool,
}

impl DesktopIo {
    pub(super) fn draw(&mut self, render: impl FnOnce(&mut ratatui::Frame<'_>)) -> io::Result<()> {
        // The in-memory backend doesn't move its cursor when it draws cells.
        // A frame that leaves the cursor hidden therefore keeps this sentinel.
        let hidden = Position::new(u16::MAX, u16::MAX);
        self.terminal.backend_mut().set_cursor_position(hidden)?;
        let cells = self.terminal.draw(render)?.buffer.clone();
        let cursor = self.terminal.get_cursor_position()?;
        (self.publish)(DesktopUpdate::Frame(DesktopFrame {
            cells,
            tabs: self.tabs.clone(),
            cursor: (cursor != hidden).then_some(cursor),
            document: self.document.clone(),
            unsaved: self.unsaved,
            draft: self.draft,
            published_unsaved: self.published_unsaved,
            confirm_close: self.confirm_close,
        }));
        Ok(())
    }

    pub(super) fn recv(&self) -> Result<DesktopInput, mpsc::RecvError> {
        self.input.recv()
    }

    pub(super) fn resize(&mut self, width: u16, height: u16) {
        self.terminal
            .backend_mut()
            .resize(width.clamp(32, 240), height.clamp(12, 100));
    }

    pub(super) fn set_tabs(&mut self, tabs: Vec<DesktopTab>) {
        self.tabs = tabs;
    }

    pub(super) fn set_status(
        &mut self,
        path: Option<String>,
        unsaved: bool,
        draft: bool,
        published_unsaved: bool,
        confirm_close: bool,
    ) {
        self.document = path;
        self.unsaved = unsaved;
        self.draft = draft;
        self.published_unsaved = published_unsaved;
        self.confirm_close = confirm_close;
    }

    pub(super) fn copy(&mut self, text: String) {
        (self.publish)(DesktopUpdate::Clipboard(text));
    }
}

/// Run the maintained launcher workspace without acquiring a terminal. Construct
/// and call on the workspace worker; the UI communicates solely via input and frames.
/// `writable` is an explicit project mount choice, not an execution grant.
pub fn run(
    root: String,
    writable: bool,
    initial_path: Option<&str>,
    host: &mut dyn ReadOnlyCommand,
    input: mpsc::Receiver<DesktopInput>,
    publish: impl FnMut(DesktopUpdate) + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    if !std::path::Path::new(&root).is_dir() {
        return Err(format!("workspace must be an existing directory: {root}").into());
    }
    let workspace = if writable {
        Workspace::new(root)?
    } else {
        Workspace::read_only(root)?
    };
    let app = &mut App::new(workspace)?;
    let (ui, mut sources) = workspace_ui::configured_launcher(app, Some(host), true)?;
    run_workspace(
        app,
        ui,
        &mut sources,
        Some(host),
        initial_path,
        input,
        publish,
    )
}

pub(crate) fn run_workspace<P: TrustedClosureProvider + 'static>(
    app: &mut AppState<P>,
    mut ui: WorkspaceTui<P>,
    sources: &mut ScratchBuffers,
    host: Option<&mut dyn ReadOnlyCommand>,
    initial_path: Option<&str>,
    input: mpsc::Receiver<DesktopInput>,
    publish: impl FnMut(DesktopUpdate) + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    ui.enable_desktop_tabs(app);
    let files = Ok(app
        .files()
        .iter()
        .map(|entry| entry.path.to_string())
        .collect());
    let mut desktop = DesktopIo {
        input,
        publish: Box::new(publish),
        terminal: Terminal::new(TestBackend::new(100, 32))?,
        document: None,
        tabs: Vec::new(),
        unsaved: false,
        draft: false,
        published_unsaved: false,
        confirm_close: false,
    };
    if let Some(path) = initial_path {
        desktop.terminal.draw(|frame| {
            ui.render_with_sources(frame, frame.area(), app, sources);
        })?;
        ui.open_app_document(app, path)?;
    }
    (desktop.publish)(DesktopUpdate::Files(files));
    ui.run_desktop(app, host, sources, desktop)
}
