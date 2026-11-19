use std::{
    collections::BTreeMap,
    sync::mpsc,
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, Key, Pos2, Rect, Vec2};
use scope_ide::tui::desktop::{
    Color, DesktopFrame, DesktopInput, DesktopUpdate, Event, KeyCode, KeyEvent, KeyModifiers,
    Modifier,
};

/// Options explicitly selected by the launcher; the host chooses its mount policy.
pub struct Project {
    pub root: String,
    pub writable: bool,
    pub initial_path: Option<String>,
}

type Worker = dyn Fn(
        Project,
        mpsc::Receiver<DesktopInput>,
        Box<dyn FnMut(DesktopUpdate)>,
    ) -> Result<(), Box<dyn std::error::Error>>
    + Send
    + Sync;

/// Run the native window on the main thread. The supplied launcher constructs and
/// runs its configured workspace on a worker; the window only exchanges frames and input.
pub fn launch(
    root: Option<String>,
    writable: bool,
    initial_path: Option<String>,
    worker: impl Fn(
        Project,
        mpsc::Receiver<DesktopInput>,
        Box<dyn FnMut(DesktopUpdate)>,
    ) -> Result<(), Box<dyn std::error::Error>>
    + Send
    + Sync
    + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    eframe::run_native(
        "Scope Desktop",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1120.0, 740.0])
                .with_min_inner_size([610.0, 360.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let mut app = Desktop {
                writable,
                initial_path,
                worker: Some(std::sync::Arc::new(worker)),
                ..Default::default()
            };
            let launch = root.is_some() || app.initial_path.is_some();
            app.root = match root {
                Some(root) => root,
                None => std::env::current_dir()?.display().to_string(),
            };
            if launch {
                app.open_project(&cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}

#[derive(Default)]
struct Tree {
    dirs: BTreeMap<String, Tree>,
    files: Vec<(String, String)>,
}

impl Tree {
    fn add(&mut self, path: &str) {
        let mut tree = self;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                tree = tree.dirs.entry(part.into()).or_default();
            } else {
                tree.files.push((part.into(), path.into()));
            }
        }
    }

    fn show(&self, ui: &mut egui::Ui, selected: Option<&str>, open: &mut Option<String>) {
        for (name, dir) in &self.dirs {
            egui::CollapsingHeader::new(name).show(ui, |ui| dir.show(ui, selected, open));
        }
        for (name, path) in &self.files {
            file_button(ui, selected, name, path, open);
        }
    }
}

fn file_button(
    ui: &mut egui::Ui,
    selected: Option<&str>,
    name: &str,
    path: &str,
    open: &mut Option<String>,
) {
    let response = ui
        .selectable_label(selected == Some(path), name)
        .on_hover_text(path);
    if response.clicked() {
        ui.memory_mut(|memory| memory.surrender_focus(response.id));
        *open = Some(path.to_owned());
    }
}

#[derive(Default)]
struct Desktop {
    worker: Option<std::sync::Arc<Worker>>,
    root: String,
    writable: bool,
    initial_path: Option<String>,
    input: Option<mpsc::Sender<DesktopInput>>,
    updates: Option<mpsc::Receiver<Result<DesktopUpdate, String>>>,
    frame: Option<DesktopFrame>,
    files: Vec<String>,
    tree: Tree,
    recent: Vec<String>,
    query: String,
    error: Option<String>,
    size: (u16, u16),
    waiting: Option<Instant>,
    closed: bool,
}

impl Desktop {
    fn open_project(&mut self, ctx: &egui::Context) {
        let (input, events) = mpsc::channel();
        let (output, updates) = mpsc::channel();
        let (root, writable, ctx) = (self.root.clone(), self.writable, ctx.clone());
        let initial_path = self.initial_path.clone();
        let worker = self
            .worker
            .as_ref()
            .expect("desktop launcher installed")
            .clone();
        match std::thread::Builder::new()
            .name("scope-workspace".into())
            .spawn(move || {
                let tx = output.clone();
                let repaint = ctx.clone();
                let result = worker(
                    Project {
                        root,
                        writable,
                        initial_path,
                    },
                    events,
                    Box::new(move |update| {
                        let _ = tx.send(Ok(update));
                        repaint.request_repaint();
                    }),
                );
                if let Err(error) = result {
                    let _ = output.send(Err(error.to_string()));
                }
                drop(output);
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.error = None;
                self.input = Some(input);
                self.updates = Some(updates);
                self.waiting = Some(Instant::now());
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn send(&mut self, input: DesktopInput) {
        if self.input.as_ref().is_some_and(|tx| tx.send(input).is_ok()) {
            self.waiting.get_or_insert_with(Instant::now);
        }
    }

    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.send(DesktopInput::Event(Event::Key(KeyEvent::new(
            code, modifiers,
        ))));
    }

    fn receive(&mut self, ctx: &egui::Context) {
        let Some(updates) = &self.updates else { return };
        loop {
            match updates.try_recv() {
                Ok(Ok(DesktopUpdate::Frame(frame))) => {
                    if let Some(path) = &frame.document
                        && self.recent.first() != Some(path)
                    {
                        self.recent.retain(|p| p != path);
                        self.recent.insert(0, path.clone());
                        self.recent.truncate(8);
                    }
                    self.frame = Some(frame);
                    self.waiting = None;
                }
                Ok(Ok(DesktopUpdate::Clipboard(text))) => ctx.copy_text(text),
                Ok(Ok(DesktopUpdate::Files(Ok(files)))) => {
                    for path in &files {
                        self.tree.add(path);
                    }
                    self.files = files;
                }
                Ok(Ok(DesktopUpdate::Files(Err(error)))) | Ok(Err(error)) => {
                    self.error = Some(error)
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.input = None;
                    self.updates = None;
                    self.waiting = None;
                    if self.error.is_none() || self.frame.is_some() {
                        self.closed = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    break;
                }
            }
        }
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.receive(ctx);
        if self.closed {
            return;
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.input.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.send(DesktopInput::Close);
        }
        if self.input.is_none() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Open a Scope project");
                ui.label("Local project directory");
                let path = ui.text_edit_singleline(&mut self.root);
                ui.checkbox(&mut self.writable, "Allow explicit saves to this project");
                if ui.button("Open project").clicked()
                    || (path.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)))
                {
                    self.open_project(ctx);
                }
                if let Some(error) = &self.error {
                    ui.label(error);
                }
            });
            return;
        }
        let mut toolbar_key = None;
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Scope");
                ui.separator();
                if ui.button("Actions  F1").clicked() {
                    toolbar_key = Some((KeyCode::F(1), KeyModifiers::NONE));
                }
                if ui.button("Quick Open").clicked() {
                    toolbar_key = Some((KeyCode::Char('o'), KeyModifiers::CONTROL));
                }
                if ui
                    .button(if self.writable && self.frame.as_ref().is_some_and(|frame| frame.document.is_some()) {
                        "Save  ⌘S / Ctrl+S"
                    } else {
                        "Publish  ⌘S / Ctrl+S"
                    })
                    .on_hover_text("Save writes a writable App document. Publish in Actions only updates memory.")
                    .clicked()
                {
                    toolbar_key = Some((KeyCode::Char('s'), KeyModifiers::CONTROL));
                }
                ui.separator();
                let name = std::path::Path::new(&self.root)
                    .file_name()
                    .and_then(|p| p.to_str())
                    .unwrap_or(&self.root);
                ui.label(name).on_hover_text(&self.root);
                if let Some(frame) = &self.frame {
                    let status = if frame.draft {
                        Some("Unpublished draft")
                    } else if frame.published_unsaved {
                        Some("Published · not saved")
                    } else if frame.unsaved {
                        Some("Unsaved work")
                    } else {
                        None
                    };
                    if let Some(status) = status {
                        ui.colored_label(Color32::GOLD, status);
                    } else if frame.document.is_some() {
                        ui.weak("Saved");
                    }
                }
                if !self.writable {
                    ui.weak("Read-only");
                }
                if let Some(since) = self.waiting {
                    let delay = Duration::from_millis(180);
                    if since.elapsed() >= delay {
                        ui.spinner();
                        ui.weak("Workspace processing…");
                    } else {
                        ctx.request_repaint_after(delay.saturating_sub(since.elapsed()));
                    }
                }
            });
        });
        let mut search_focused = false;
        let mut open = None;
        let selected = self
            .frame
            .as_ref()
            .and_then(|frame| frame.document.as_deref());
        egui::SidePanel::left("project")
            .resizable(true)
            .default_width(228.0)
            .show(ctx, |ui| {
                ui.heading("Project files");
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.query).hint_text("Filter by filename"),
                );
                search_focused = search.has_focus() || search.lost_focus();
                if let Some(error) = &self.error {
                    ui.label(error);
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.query.is_empty() {
                            if !self.recent.is_empty() {
                                ui.weak("Recent");
                                for path in &self.recent {
                                    file_button(ui, selected, path, path, &mut open);
                                }
                                ui.separator();
                            }
                            self.tree.show(ui, selected, &mut open);
                        } else {
                            let query = self.query.to_lowercase();
                            let mut matches = self.files.iter().filter(|path| {
                                path.rsplit('/')
                                    .next()
                                    .is_some_and(|name| name.to_lowercase().contains(&query))
                            });
                            if search.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) {
                                open = matches.next().cloned();
                            }
                            for path in matches.take(200) {
                                file_button(ui, selected, path, path, &mut open);
                            }
                        }
                    });
            });
        if let Some(path) = open {
            self.send(DesktopInput::Open(path));
        }
        if let Some((code, modifiers)) = toolbar_key {
            self.key(code, modifiers);
        }
        let mut tab_input = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(frame) = &self.frame
                && !frame.tabs.is_empty()
            {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for tab in &frame.tabs {
                            let name = tab.path.rsplit('/').next().unwrap_or(&tab.path);
                            let label = if tab.dirty {
                                format!("{name} •")
                            } else {
                                name.to_owned()
                            };
                            if ui
                                .selectable_label(
                                    frame.document.as_deref() == Some(&tab.path),
                                    label,
                                )
                                .on_hover_text(&tab.path)
                                .clicked()
                            {
                                tab_input = Some(DesktopInput::Open(tab.path.clone()));
                            }
                            if ui
                                .small_button("×")
                                .on_hover_text("Close document")
                                .clicked()
                            {
                                tab_input = Some(DesktopInput::CloseDocument(tab.path.clone()));
                            }
                            ui.separator();
                        }
                    });
                });
            }
            let font = egui::FontId::monospace(14.0);
            let cell = Vec2::new(ui.fonts_mut(|f| f.glyph_width(&font, 'M')), 17.0);
            let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
            let size = (
                ((rect.width() / cell.x) as u16).clamp(32, 240),
                ((rect.height() / cell.y) as u16).clamp(12, 100),
            );
            if size != self.size {
                self.size = size;
                self.send(DesktopInput::Resize(size.0, size.1));
            }
            if let Some(frame) = &self.frame {
                paint(&ui.painter_at(rect), rect.min, cell, font, frame);
            }
        });
        if let Some(input) = tab_input {
            self.send(input);
        }
        if self.frame.as_ref().is_some_and(|frame| frame.confirm_close) {
            let modal = egui::Modal::new(egui::Id::new("discard-work")).show(ctx, |ui| {
                ui.heading("Discard unsaved work?");
                ui.horizontal(|ui| {
                    (
                        ui.button("Keep editing").clicked(),
                        ui.button("Discard and close").clicked(),
                    )
                })
                .inner
            });
            if modal.inner.1 {
                self.send(DesktopInput::ConfirmClose);
            } else if modal.inner.0 || modal.should_close() {
                self.key(KeyCode::Esc, KeyModifiers::NONE);
            }
            return;
        }
        if !search_focused && ctx.input(|i| i.focused) {
            for event in ctx.input(|i| i.events.clone()) {
                match event {
                    egui::Event::Copy => self.send(DesktopInput::Copy { cut: false }),
                    egui::Event::Cut => self.send(DesktopInput::Copy { cut: true }),
                    egui::Event::Paste(text) => self.send(DesktopInput::Event(Event::Paste(text))),
                    egui::Event::Text(text) => {
                        for ch in text.chars().filter(|ch| !ch.is_control()) {
                            self.key(KeyCode::Char(ch), KeyModifiers::NONE);
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        repeat,
                        ..
                    } if !(repeat
                        && (modifiers.command || modifiers.ctrl || key == Key::Enter)) =>
                    {
                        if key == Key::Tab && modifiers.ctrl {
                            self.send(DesktopInput::CycleDocument {
                                backwards: modifiers.shift,
                            });
                        } else if key == Key::W && modifiers.command {
                            if let Some(path) =
                                self.frame.as_ref().and_then(|frame| frame.document.clone())
                            {
                                self.send(DesktopInput::CloseDocument(path));
                            }
                        } else if let Some((code, mods)) = translate_key(key, modifiers) {
                            self.key(code, mods);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn translate_key(key: Key, modifiers: egui::Modifiers) -> Option<(KeyCode, KeyModifiers)> {
    let mut mods = KeyModifiers::NONE;
    if modifiers.ctrl || modifiers.command {
        mods |= KeyModifiers::CONTROL;
    }
    if modifiers.alt {
        mods |= KeyModifiers::ALT;
    }
    if modifiers.shift {
        mods |= KeyModifiers::SHIFT;
    }
    let code = match key {
        Key::ArrowUp => KeyCode::Up,
        Key::ArrowDown => KeyCode::Down,
        Key::ArrowLeft => KeyCode::Left,
        Key::ArrowRight => KeyCode::Right,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
        Key::Home => KeyCode::Home,
        Key::End => KeyCode::End,
        Key::Escape => KeyCode::Esc,
        Key::Enter => KeyCode::Enter,
        Key::Tab if modifiers.shift => KeyCode::BackTab,
        Key::Tab => KeyCode::Tab,
        Key::Backspace => KeyCode::Backspace,
        Key::Delete => KeyCode::Delete,
        Key::F1 => KeyCode::F(1),
        Key::F3 => KeyCode::F(3),
        Key::F4 => KeyCode::F(4),
        Key::F5 => KeyCode::F(5),
        Key::F6 => KeyCode::F(6),
        Key::F7 => KeyCode::F(7),
        Key::F8 => KeyCode::F(8),
        Key::P if modifiers.command => KeyCode::Char('o'),
        Key::Z if modifiers.command && modifiers.shift => {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::Char('y')
        }
        Key::C | Key::V | Key::X if modifiers.command && !modifiers.ctrl => return None,
        _ if mods.contains(KeyModifiers::CONTROL) => {
            let mut chars = key.name().chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyCode::Char(first.to_ascii_lowercase())
        }
        _ => return None,
    };
    Some((code, mods))
}

fn paint(
    painter: &egui::Painter,
    origin: Pos2,
    cell: Vec2,
    font: egui::FontId,
    frame: &DesktopFrame,
) {
    let default_fg = Color32::from_rgb(215, 221, 232);
    let default_bg = Color32::from_rgb(22, 25, 32);
    let point = |x: u16, y: u16| origin + Vec2::new(f32::from(x) * cell.x, f32::from(y) * cell.y);
    let area = frame.cells.area;
    painter.rect_filled(
        Rect::from_min_max(origin, point(area.width, area.height)),
        0.0,
        default_bg,
    );
    for y in 0..area.height {
        for x in 0..area.width {
            let c = &frame.cells[(x, y)];
            let pos = point(x, y);
            let (mut fg, mut bg) = (color(c.fg, default_fg), color(c.bg, default_bg));
            if c.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if bg != default_bg {
                painter.rect_filled(Rect::from_min_size(pos, cell), 0.0, bg);
            }
            if c.symbol() != " " && !c.modifier.contains(Modifier::HIDDEN) {
                painter.text(pos, egui::Align2::LEFT_TOP, c.symbol(), font.clone(), fg);
                if c.modifier.contains(Modifier::UNDERLINED) {
                    let start = pos + Vec2::new(0.0, cell.y - 1.0);
                    painter.line_segment(
                        [start, start + Vec2::new(cell.x, 0.0)],
                        egui::Stroke::new(1.0_f32, fg),
                    );
                }
            }
        }
    }
    if let Some(cursor) = frame.cursor {
        painter.rect_filled(
            Rect::from_min_size(point(cursor.x, cursor.y), Vec2::new(2.0, cell.y)),
            0.0,
            Color32::LIGHT_BLUE,
        );
    }
}

fn color(color: Color, fallback: Color32) -> Color32 {
    let palette = [
        0x262a33, 0xdf6678, 0x9dcf92, 0xe6c384, 0x8cb4ef, 0xc7a3ea, 0x77c9ce, 0xc5cad4, 0x727b90,
        0xff8897, 0xb8e9ab, 0xffda98, 0xa9caff, 0xdfbafa, 0x9ee0e3, 0xf4f5fa,
    ];
    let rgb = match color {
        Color::Reset => return fallback,
        Color::Rgb(r, g, b) => return Color32::from_rgb(r, g, b),
        Color::Indexed(i @ 16..=231) => {
            let i = i - 16;
            let level = |n| if n == 0 { 0 } else { 55 + 40 * n };
            return Color32::from_rgb(level(i / 36), level(i / 6 % 6), level(i % 6));
        }
        Color::Indexed(i @ 232..=255) => return Color32::from_gray(8 + 10 * (i - 232)),
        Color::Indexed(i) => palette[i as usize],
        Color::Black => palette[0],
        Color::Red => palette[1],
        Color::Green => palette[2],
        Color::Yellow => palette[3],
        Color::Blue => palette[4],
        Color::Magenta => palette[5],
        Color::Cyan => palette[6],
        Color::Gray => palette[7],
        Color::DarkGray => palette[8],
        Color::LightRed => palette[9],
        Color::LightGreen => palette[10],
        Color::LightYellow => palette[11],
        Color::LightBlue => palette[12],
        Color::LightMagenta => palette[13],
        Color::LightCyan => palette[14],
        Color::White => palette[15],
    };
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}
