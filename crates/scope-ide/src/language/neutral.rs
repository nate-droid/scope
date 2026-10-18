use crate::language::{
    Diagnostic, LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, Symbol,
};
use crate::text::{Position, TextRange};

pub struct NeutralLanguage;

impl LanguageService for NeutralLanguage {
    fn language_id(&self) -> &'static str {
        "scope.neutral"
    }

    fn language_name(&self) -> &'static str {
        "Language Neutral"
    }

    fn matches_path(&self, _path: &LanguagePath<'_>) -> bool {
        true
    }

    fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis {
            diagnostics: detect_conflict_markers(document.text()),
            symbols: Vec::new(),
        }
    }

    fn outline(&self, _document: LanguageDocument<'_>) -> Vec<Symbol> {
        Vec::new()
    }
}

fn detect_conflict_markers(text: &str) -> Vec<Diagnostic> {
    text.lines()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let trimmed_start = line.len() - line.trim_start().len();
            let trimmed = &line[trimmed_start..];
            let marker = ["<<<<<<<", "=======", ">>>>>>>"]
                .into_iter()
                .find(|marker| trimmed.starts_with(marker))?;
            let start = Position::new(line_index, trimmed_start);
            let end = Position::new(line_index, trimmed_start + marker.len());

            Some(
                Diagnostic::error(
                    TextRange::new(start, end),
                    "unresolved merge conflict marker",
                )
                .with_source("scope.neutral"),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn neutral_outline_is_empty() {
        let document = Document::new(
            crate::document::DocumentId::new(1),
            "README.md",
            "# Hello\n\nPlain text.\n",
        );

        assert_eq!(
            NeutralLanguage.outline(document.as_language_document()),
            Vec::new()
        );
    }

    #[test]
    fn reports_merge_conflict_markers_in_any_text_file() {
        let document = Document::new(
            crate::document::DocumentId::new(1),
            "README.md",
            "<<<<<<< ours\nhello\n=======\nworld\n>>>>>>> theirs\n",
        );

        let diagnostics = NeutralLanguage.diagnostics(document.as_language_document());

        assert_eq!(diagnostics.len(), 3);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.message == "unresolved merge conflict marker")
        );
    }
}
