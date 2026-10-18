use scope_language::{
    Diagnostic, LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, Position,
    Symbol, SymbolKind, TextRange,
};

pub struct CalcLanguage;

pub static LANGUAGE: CalcLanguage = CalcLanguage;

impl LanguageService for CalcLanguage {
    fn language_id(&self) -> &'static str {
        "example.calc"
    }

    fn language_name(&self) -> &'static str {
        "Calc"
    }

    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("calc")
    }

    fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
        analyze(document.text())
    }
}

fn analyze(text: &str) -> LanguageAnalysis {
    let mut analysis = LanguageAnalysis::default();
    for (line_index, line) in text.lines().enumerate() {
        let indentation = line.len() - line.trim_start().len();
        let Some(rest) = line[indentation..].strip_prefix("def ") else {
            continue;
        };
        let name_len = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        if name_len == 0 {
            analysis.diagnostics.push(
                Diagnostic::error(
                    TextRange::new(
                        Position::new(line_index, indentation),
                        Position::new(line_index, indentation + 3),
                    ),
                    "expected definition name after `def`",
                )
                .with_source("example.calc")
                .with_code("missing-name"),
            );
        } else {
            let start = indentation + 4;
            analysis.symbols.push(Symbol {
                name: rest[..name_len].to_owned(),
                kind: SymbolKind::Function,
                range: TextRange::new(
                    Position::new(line_index, start),
                    Position::new(line_index, start + name_len),
                ),
            });
        }
    }
    analysis
}

#[cfg(test)]
mod tests {
    use super::*;
    use scope_ide::{app::App, workspace::Workspace};

    #[test]
    fn analyzes_without_a_host() {
        assert!(LANGUAGE.matches_path(&LanguagePath::new("main.calc")));
        assert!(!LANGUAGE.matches_path(&LanguagePath::new("main.txt")));
        let document = LanguageDocument::new(
            LanguagePath::new("main.calc"),
            "  def add = 1\ndef = 2\n",
            1,
        );
        let analysis = LANGUAGE.analyze(document.clone());
        let symbols = LANGUAGE.outline(document.clone());
        assert_eq!(analysis.symbols, symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "add");
        assert_eq!(symbols[0].range.start, Position::new(0, 6));
        let diagnostics = LANGUAGE.diagnostics(document);
        assert_eq!(analysis.diagnostics, diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some("missing-name"));
    }

    #[test]
    fn registers_and_analyzes_through_the_public_app_api() -> Result<(), Box<dyn std::error::Error>>
    {
        let workspace = Workspace::read_only(format!("{}/fixtures", env!("CARGO_MANIFEST_DIR")))?;
        assert_eq!(workspace.capabilities(), &["cell:fs/fs@0.1.0"]);
        let original = workspace.load_file("main.calc")?.contents;
        let denied = workspace.save_file("main.calc", "changed").unwrap_err();
        assert!(matches!(
            denied,
            scope_ide::workspace::WorkspaceError::WriteFile { .. }
        ));
        assert_eq!(workspace.load_file("main.calc")?.contents, original);
        assert!(workspace.load_file("../Cargo.toml").is_err());
        let mut app = App::builder(workspace).language(&LANGUAGE).build()?;
        app.open_path("main.calc")?;
        assert_eq!(app.open_language().unwrap().language_id(), "example.calc");
        let symbols = app.open_outline()?;
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "answer");
        let diagnostics = app.open_diagnostics()?;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some("missing-name"));
        assert!(app.save_open_document().is_err());
        Ok(())
    }
}
