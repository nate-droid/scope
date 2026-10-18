use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::pane::{Pane, TuiContext, TuiContextMut};
use crate::app::App;

pub struct FilesPane;

impl Pane for FilesPane {
    fn id(&self) -> &'static str {
        "scope.files"
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        let height = area.height.saturating_sub(2).max(1) as usize;
        let offset = ctx
            .app()
            .selected_index()
            .unwrap_or(0)
            .saturating_sub(height - 1);
        frame.render_widget(
            file_list(ctx.app(), focus_style(ctx))
                .scroll((offset.min(u16::MAX as usize) as u16, 0)),
            area,
        );
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut TuiContextMut<'_>) -> bool {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                ctx.app_mut().move_selection_up();
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                ctx.app_mut().move_selection_down();
                true
            }
            KeyCode::Enter | KeyCode::Char('o') => {
                let _ = ctx.app_mut().open_selected();
                true
            }
            KeyCode::Char('r') => {
                let _ = ctx.app_mut().refresh_files();
                true
            }
            _ => false,
        }
    }
}

pub struct DocumentPane;

impl Pane for DocumentPane {
    fn id(&self) -> &'static str {
        "scope.document"
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        frame.render_widget(
            document_view(ctx.app(), focus_style(ctx)).scroll((ctx.scroll(), 0)),
            area,
        );
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut TuiContextMut<'_>) -> bool {
        let max = max_scroll(document_view(ctx.app(), Style::default()), ctx.viewport());
        scroll_key(key, ctx, max)
    }
}

pub struct LanguagePane;

impl Pane for LanguagePane {
    fn id(&self) -> &'static str {
        "scope.language"
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &TuiContext<'_>) {
        frame.render_widget(
            language_pane(ctx.app(), focus_style(ctx)).scroll((ctx.scroll(), 0)),
            area,
        );
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut TuiContextMut<'_>) -> bool {
        let max = max_scroll(language_pane(ctx.app(), Style::default()), ctx.viewport());
        scroll_key(key, ctx, max)
    }
}

fn focus_style(ctx: &TuiContext<'_>) -> Style {
    if ctx.is_focused() {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    }
}

fn max_scroll(paragraph: Paragraph<'_>, area: Rect) -> u16 {
    paragraph
        .line_count(area.width.saturating_sub(2).max(1))
        .saturating_sub(area.height.saturating_sub(2).max(1) as usize)
        .min(u16::MAX as usize) as u16
}

fn scroll_key(key: KeyEvent, ctx: &mut TuiContextMut<'_>, max: u16) -> bool {
    let current = ctx.scroll().min(max);
    let page = ctx.viewport().height.saturating_sub(2).max(1);
    let next = match key.code {
        KeyCode::Up | KeyCode::Char('k') => current.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => current.saturating_add(1).min(max),
        KeyCode::PageUp => current.saturating_sub(page),
        KeyCode::PageDown => current.saturating_add(page).min(max),
        KeyCode::Home => 0,
        KeyCode::End => max,
        _ => return false,
    };
    ctx.set_scroll(next);
    true
}

fn file_list(app: &App, border: Style) -> Paragraph<'_> {
    let selected = app.selected_index();
    let lines = app
        .files()
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let is_selected = Some(index) == selected;
            let marker = if is_selected { "> " } else { "  " };
            let style = if is_selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            Line::from(vec![
                Span::raw(marker),
                Span::styled(entry.path.display().to_string(), style),
            ])
        })
        .collect::<Vec<_>>();

    Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .title("Files"),
    )
}

fn language_pane(app: &App, border: Style) -> Paragraph<'_> {
    let Some(document) = app.open_document() else {
        return Paragraph::new("No document open.")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border)
                    .title("Language"),
            )
            .wrap(Wrap { trim: false });
    };

    let service = app.semantic_service_for_document(document);
    let analysis = app.open_analysis().unwrap_or_default();
    let diagnostics = analysis.diagnostics;
    let outline = analysis.symbols;
    let actions = app.open_actions().unwrap_or_default();

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Service: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!(
                "{} ({})",
                service.language_name(),
                service.language_id()
            )),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            format!("Actions ({})", actions.len()),
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ];

    if actions.is_empty() {
        lines.push(Line::from("  none"));
    } else {
        lines.extend(
            actions
                .into_iter()
                .map(|action| Line::from(format!("  {} — {}", action.id, action.label))),
        );
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("Diagnostics ({})", diagnostics.len()),
        Style::default().add_modifier(Modifier::BOLD),
    )));

    if diagnostics.is_empty() {
        lines.push(Line::from("  none"));
    } else {
        lines.extend(diagnostics.into_iter().map(|diagnostic| {
            Line::from(format!(
                "  {}:{} {}: {}",
                diagnostic.range.start.line + 1,
                diagnostic.range.start.column + 1,
                diagnostic.severity,
                diagnostic.message
            ))
        }));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("Outline ({})", outline.len()),
        Style::default().add_modifier(Modifier::BOLD),
    )));

    if outline.is_empty() {
        lines.push(Line::from("  none"));
    } else {
        lines.extend(outline.into_iter().map(|symbol| {
            Line::from(format!(
                "  {} {} @ {}:{}",
                symbol.kind,
                symbol.name,
                symbol.range.start.line + 1,
                symbol.range.start.column + 1
            ))
        }));
    }

    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border)
                .title("Language"),
        )
        .wrap(Wrap { trim: false })
}

fn document_view(app: &App, border: Style) -> Paragraph<'_> {
    match app.open_document() {
        Some(document) => {
            let dirty = if document.is_dirty() {
                "dirty"
            } else {
                "clean"
            };
            let title = format!("{} ({dirty})", document.path().display());
            Paragraph::new(document.text().to_owned())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border)
                        .title(title),
                )
                .wrap(Wrap { trim: false })
        }
        None => Paragraph::new("No document open. Select a file and press Enter.")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border)
                    .title("Document"),
            )
            .wrap(Wrap { trim: false }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{
        Diagnostic, LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, Position,
        Symbol, SymbolKind, TextRange,
    };
    use crate::test_support::TempWorkspace;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn language_pane_uses_one_combined_analysis_per_render() {
        struct CountingLanguage(AtomicUsize);

        impl LanguageService for CountingLanguage {
            fn language_id(&self) -> &'static str {
                "test.counting"
            }
            fn language_name(&self) -> &'static str {
                "Counting"
            }
            fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
                path.extension() == Some("counting")
            }
            fn diagnostics(&self, _: LanguageDocument<'_>) -> Vec<Diagnostic> {
                panic!("pane should request combined analysis")
            }
            fn outline(&self, _: LanguageDocument<'_>) -> Vec<Symbol> {
                panic!("pane should request combined analysis")
            }
            fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
                self.0.fetch_add(1, Ordering::SeqCst);
                assert_eq!(document.text(), "example");
                let range = TextRange::new(Position::new(0, 0), Position::new(0, 7));
                LanguageAnalysis {
                    diagnostics: vec![Diagnostic::warning(range, "example warning")],
                    symbols: vec![Symbol {
                        name: "example".into(),
                        kind: SymbolKind::Function,
                        range,
                    }],
                }
            }
        }

        let language = Box::leak(Box::new(CountingLanguage(AtomicUsize::new(0))));
        let root = TempWorkspace::new("combined_language_pane");
        root.write_file("main.counting", "example");
        let mut app = App::builder(root.workspace())
            .language(language)
            .build()
            .unwrap();
        assert!(matches!(
            app.open_analysis(),
            Err(crate::app::AppError::NoOpenDocument)
        ));
        app.open_path("main.counting").unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();

        for expected_calls in 1..=2 {
            terminal
                .draw(|frame| {
                    LanguagePane.render(frame, frame.area(), &TuiContext::new(&app));
                })
                .unwrap();
            assert_eq!(language.0.load(Ordering::SeqCst), expected_calls);
            let buffer = terminal.backend().buffer();
            let rendered: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            assert!(rendered.contains("example warning"));
            assert!(rendered.contains("function example"));
        }
    }
}
