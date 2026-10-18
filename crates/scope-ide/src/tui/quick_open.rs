use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;

#[derive(Default)]
pub(super) struct QuickOpen {
    query: String,
    selected: usize,
    error: Option<String>,
}

impl QuickOpen {
    fn matches(&self, app: &App) -> Vec<usize> {
        let query = self.query.to_lowercase();
        app.files()
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry
                    .path
                    .display()
                    .to_string()
                    .to_lowercase()
                    .contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> bool {
        match key.code {
            KeyCode::Esc => return true,
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.push(ch);
                self.selected = 0;
                self.error = None;
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
                self.error = None;
            }
            KeyCode::Up => {
                self.selected = self.selected.min(self.matches(app).len().saturating_sub(1));
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down => {
                self.selected = self
                    .selected
                    .saturating_add(1)
                    .min(self.matches(app).len().saturating_sub(1));
            }
            KeyCode::Enter => {
                let matches = self.matches(app);
                self.selected = self.selected.min(matches.len().saturating_sub(1));
                let Some(&index) = matches.get(self.selected) else {
                    self.error = Some("No matching files".into());
                    return false;
                };
                if app
                    .open_document()
                    .is_some_and(|document| document.is_dirty())
                {
                    self.error = Some(
                        "Unsaved changes: save the current document before opening a file".into(),
                    );
                    return false;
                }
                let path = app.files()[index].path.clone();
                match app.open_path(path) {
                    Ok(_) => return true,
                    Err(error) => self.error = Some(format!("Could not open file: {error}")),
                }
            }
            _ => {}
        }
        false
    }

    pub(super) fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
        let width = area.width.min(80);
        let height = area.height.min(20);
        let modal = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, modal);
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Quick open — Esc cancels");
        let inner = block.inner(modal);
        frame.render_widget(block, modal);
        if inner.is_empty() {
            return;
        }
        frame.render_widget(
            Paragraph::new(format!("> {}", self.query)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        let error_height = u16::from(self.error.is_some() && inner.height > 1);
        let rows = inner.height.saturating_sub(1 + error_height) as usize;
        let matches = self.matches(app);
        let selected = self.selected.min(matches.len().saturating_sub(1));
        // Keep the selected result on screen without mutable render state.
        let offset = selected.saturating_sub(rows.saturating_sub(1));
        let lines = if matches.is_empty() {
            vec![Line::raw("No matching files")]
        } else {
            matches
                .iter()
                .enumerate()
                .skip(offset)
                .take(rows)
                .map(|(position, &index)| {
                    let path = app.files()[index].path.display().to_string();
                    if position == selected {
                        Line::styled(
                            format!("> {path}"),
                            Style::default().add_modifier(Modifier::REVERSED),
                        )
                    } else {
                        Line::raw(format!("  {path}"))
                    }
                })
                .collect()
        };
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(inner.x, inner.y + 1, inner.width, rows as u16),
        );
        if let Some(error) = &self.error
            && error_height > 0
        {
            frame.render_widget(
                Paragraph::new(error.as_str()).style(Style::default().fg(Color::Red)),
                Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempWorkspace;
    use crate::text::{Position, TextEdit};
    use ratatui::{Terminal, backend::TestBackend};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_query(picker: &mut QuickOpen, app: &mut App, query: &str) {
        for ch in query.chars() {
            assert!(!picker.handle_key(key(KeyCode::Char(ch)), app));
        }
    }

    #[test]
    fn filters_paths_case_insensitively_and_resets_selection() {
        let root = TempWorkspace::new("quick_open_filter");
        root.write_file("src/Main.rs", "main");
        root.write_file("tests/main.rs", "test");
        let mut app = App::new(root.workspace()).unwrap();
        let mut picker = QuickOpen::default();
        picker.handle_key(key(KeyCode::Down), &mut app);
        assert_eq!(picker.selected, 1);
        type_query(&mut picker, &mut app, "SRC/MA");
        assert_eq!(picker.selected, 0);
        assert_eq!(picker.matches(&app).len(), 1);
        picker.selected = 1;
        picker.handle_key(key(KeyCode::Backspace), &mut app);
        assert_eq!(picker.query, "SRC/M");
        assert_eq!(picker.selected, 0);
        assert!(picker.handle_key(key(KeyCode::Enter), &mut app));
        assert_eq!(app.open_document().unwrap().text(), "main");
    }

    #[test]
    fn navigation_is_bounded_and_control_characters_are_ignored() {
        let root = TempWorkspace::new("quick_open_keys");
        root.write_file("a.txt", "a");
        root.write_file("b.txt", "b");
        let mut app = App::new(root.workspace()).unwrap();
        let mut picker = QuickOpen::default();
        picker.handle_key(key(KeyCode::Up), &mut app);
        assert_eq!(picker.selected, 0);
        for _ in 0..5 {
            picker.handle_key(key(KeyCode::Down), &mut app);
        }
        assert_eq!(picker.selected, 1);
        picker.handle_key(
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            &mut app,
        );
        assert!(picker.query.is_empty());
        assert_eq!(picker.selected, 1);
        assert!(picker.handle_key(key(KeyCode::Esc), &mut app));
        assert!(app.open_document().is_none());
    }

    #[test]
    fn empty_results_do_not_close() {
        let root = TempWorkspace::new("quick_open_empty");
        let mut app = App::new(root.workspace()).unwrap();
        let mut picker = QuickOpen::default();
        picker.handle_key(key(KeyCode::Down), &mut app);
        picker.handle_key(key(KeyCode::Up), &mut app);
        assert_eq!(picker.selected, 0);
        assert!(!picker.handle_key(key(KeyCode::Enter), &mut app));
        assert!(picker.error.is_some());
    }

    #[test]
    fn failed_open_displays_error_and_allows_retry() {
        let root = TempWorkspace::new("quick_open_failure");
        root.write_file("a.txt", "a");
        let mut app = App::new(root.workspace()).unwrap();
        std::fs::remove_file(root.root().join("a.txt")).unwrap();
        let mut picker = QuickOpen::default();
        assert!(!picker.handle_key(key(KeyCode::Enter), &mut app));
        let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
        terminal
            .draw(|frame| picker.render(frame, frame.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Could not open file:"));
        root.write_file("a.txt", "restored");
        assert!(picker.handle_key(key(KeyCode::Enter), &mut app));
        assert_eq!(app.open_document().unwrap().text(), "restored");
    }

    #[test]
    fn dirty_document_is_preserved_even_when_reopening_same_path() {
        let root = TempWorkspace::new("quick_open_dirty");
        root.write_file("a.txt", "a");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("a.txt").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 1), " changed"))
            .unwrap();
        let id = app.open_document().unwrap().id();
        let mut picker = QuickOpen::default();
        assert!(!picker.handle_key(key(KeyCode::Enter), &mut app));
        assert!(picker.error.as_ref().unwrap().contains("Unsaved changes"));
        assert_eq!(app.open_document().unwrap().id(), id);
        assert_eq!(app.open_document().unwrap().text(), "a changed");
        assert!(app.open_document().unwrap().is_dirty());
    }

    #[test]
    fn selected_result_scrolls_into_view_and_small_areas_render() {
        let root = TempWorkspace::new("quick_open_scroll");
        for index in 0..30 {
            root.write_file(format!("file{index:02}.txt"), "text");
        }
        let mut app = App::new(root.workspace()).unwrap();
        let mut picker = QuickOpen::default();
        for _ in 0..29 {
            picker.handle_key(key(KeyCode::Down), &mut app);
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| picker.render(frame, frame.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("> file29.txt"));
        assert_eq!(buffer[(0, 0)].symbol(), " ");
        for size in 0..4 {
            terminal
                .draw(|frame| picker.render(frame, Rect::new(2, 2, size, size), &app))
                .unwrap();
        }
    }
}
