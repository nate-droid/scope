use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::TrustedClosureProvider;
use crate::app::AppState;

#[derive(Default)]
pub(super) struct QuickOpen {
    targets: Option<Vec<(String, String)>>,
    app_documents: bool,
    query: String,
    selected: usize,
    error: Option<String>,
}

impl QuickOpen {
    /// App-owned destinations, including enrolled documents no longer present on disk.
    pub(super) fn app_documents() -> Self {
        Self {
            app_documents: true,
            ..Self::default()
        }
    }

    pub(super) fn supplied(targets: Vec<(String, String)>) -> Self {
        Self {
            targets: Some(targets),
            ..Self::default()
        }
    }

    fn entries<P: TrustedClosureProvider>(&self, app: &AppState<P>) -> Vec<(String, String)> {
        if let Some(targets) = &self.targets {
            return targets.clone();
        }
        let entries: Vec<_> = app
            .files()
            .iter()
            .map(|entry| {
                let path = entry.path.display().to_string();
                (path.clone(), path)
            })
            .collect();
        #[cfg(not(feature = "experimental-sprout-closure"))]
        let _ = self.app_documents;
        #[cfg(feature = "experimental-sprout-closure")]
        if self.app_documents {
            let mut entries: std::collections::BTreeMap<_, _> = entries.into_iter().collect();
            if let Some(documents) = app.registered_documents() {
                for (module, document) in documents.documents() {
                    let path = document.path().display().to_string();
                    entries.insert(
                        path.clone(),
                        format!(
                            "{path} — registered {module}{}",
                            if document.is_dirty() {
                                " (unsaved)"
                            } else {
                                ""
                            }
                        ),
                    );
                }
            }
            return entries.into_iter().collect();
        }
        entries
    }

    pub(super) fn set_query(&mut self, query: &str) {
        if self.query != query {
            self.query = query.into();
            self.selected = 0;
            self.error = None;
        }
    }

    pub(super) fn selected_path<P: TrustedClosureProvider>(
        &self,
        app: &AppState<P>,
    ) -> Option<String> {
        let matches = self.matches(app);
        matches
            .get(self.selected.min(matches.len().saturating_sub(1)))
            .map(|&index| self.entries(app)[index].0.clone())
    }

    pub(super) fn navigate<P: TrustedClosureProvider>(
        &mut self,
        code: KeyCode,
        app: &AppState<P>,
    ) -> bool {
        let last = self.matches(app).len().saturating_sub(1);
        self.selected = self.selected.min(last);
        match code {
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = last,
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = self.selected.saturating_add(1).min(last),
            _ => return false,
        }
        true
    }

    fn matches<P: TrustedClosureProvider>(&self, app: &AppState<P>) -> Vec<usize> {
        let query = self.query.to_lowercase();
        let entries = self.entries(app);
        let mut matches: Vec<_> = entries
            .iter()
            .enumerate()
            .filter(|(_, (target, label))| {
                target.to_lowercase().contains(&query) || label.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect();
        // A basename like main.sprout is also a suffix of domain.sprout. Exact
        // path/name/stem selections must win over incidental substring matches.
        if !query.is_empty() {
            matches.sort_by_key(|&index| {
                let target = entries[index].0.to_lowercase();
                let name = target.rsplit(['/', '\\']).next().unwrap_or(&target);
                let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
                let rank = if target == query {
                    0
                } else if name == query {
                    1
                } else if stem == query {
                    2
                } else {
                    3
                };
                (rank, index)
            });
        }
        matches
    }

    pub(super) fn handle_key<P: TrustedClosureProvider>(
        &mut self,
        key: KeyEvent,
        app: &mut AppState<P>,
    ) -> bool {
        if self.navigate(key.code, app) {
            return false;
        }
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
            KeyCode::Tab => {
                if let Some(path) = self.selected_path(app) {
                    self.set_query(&path);
                }
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

    pub(super) fn render<P: TrustedClosureProvider>(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        app: &AppState<P>,
    ) {
        self.render_titled(
            frame,
            area,
            app,
            "Quick open — Home/End first/last · Esc cancels",
        );
    }

    pub(super) fn render_titled<P: TrustedClosureProvider>(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        app: &AppState<P>,
        title: &str,
    ) {
        let width = area.width.min(80);
        let height = area.height.min(20);
        let modal = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, modal);
        let block = Block::default().borders(Borders::ALL).title(title);
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
        let entries = self.entries(app);
        let lines = if matches.is_empty() {
            vec![Line::raw("No matching files")]
        } else {
            matches
                .iter()
                .enumerate()
                .skip(offset)
                .take(rows)
                .map(|(position, &index)| {
                    let path = &entries[index].1;
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
    use crate::app::App;
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
    fn home_and_end_jump_within_filtered_results() {
        let root = TempWorkspace::new("quick_open_home_end");
        root.write_file("a.txt", "a");
        root.write_file("b.rs", "b");
        root.write_file("c.txt", "c");
        let mut app = App::new(root.workspace()).unwrap();
        let mut picker = QuickOpen::default();

        assert!(!picker.handle_key(key(KeyCode::End), &mut app));
        assert_eq!(picker.selected, 2);
        assert!(!picker.handle_key(key(KeyCode::Home), &mut app));
        assert_eq!(picker.selected, 0);

        type_query(&mut picker, &mut app, ".txt");
        assert!(!picker.handle_key(key(KeyCode::End), &mut app));
        assert_eq!(picker.selected, 1);
        assert_eq!(picker.query, ".txt");
        assert!(picker.handle_key(key(KeyCode::Enter), &mut app));
        assert_eq!(app.open_document().unwrap().text(), "c");
    }

    #[test]
    fn home_and_end_are_safe_with_no_matches() {
        let root = TempWorkspace::new("quick_open_home_end_empty");
        root.write_file("a.txt", "a");
        let mut app = App::new(root.workspace()).unwrap();
        let mut picker = QuickOpen::default();
        type_query(&mut picker, &mut app, "missing");
        for code in [KeyCode::End, KeyCode::Home] {
            assert!(!picker.handle_key(key(code), &mut app));
            assert_eq!(picker.selected, 0);
        }
        assert!(!picker.handle_key(key(KeyCode::Enter), &mut app));
        assert_eq!(picker.error.as_deref(), Some("No matching files"));
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
    #[test]
    fn exact_name_and_stem_outrank_substrings_in_other_filenames() {
        let root = TempWorkspace::new("quick_open_exact_names");
        root.write_file("workflow/domain.sprout", "domain");
        root.write_file("workflow/main.sprout", "main");
        let app = App::new(root.workspace()).unwrap();
        let mut picker = QuickOpen::app_documents();
        for query in ["main.sprout", "main", "MAIN", "workflow/main.sprout"] {
            picker.set_query(query);
            assert_eq!(
                picker.selected_path(&app).as_deref(),
                Some("workflow/main.sprout")
            );
        }
        picker.set_query("domain");
        assert_eq!(
            picker.selected_path(&app).as_deref(),
            Some("workflow/domain.sprout")
        );
        picker.set_query("main.sprout");
        assert_eq!(
            picker.selected_path(&app).as_deref(),
            Some("workflow/main.sprout")
        );
        assert_eq!(
            picker.matches(&app).len(),
            2,
            "substring matches remain selectable"
        );
    }
}
