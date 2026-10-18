use crate::language::{
    Diagnostic, LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService, Symbol,
    SymbolKind,
};
use crate::text::{Position, TextRange};
use std::collections::{BTreeMap, BTreeSet};

pub struct SimpleLanguage;

impl LanguageService for SimpleLanguage {
    fn language_id(&self) -> &'static str {
        "scope.simple"
    }

    fn language_name(&self) -> &'static str {
        "Scope Simple"
    }

    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("scope")
    }

    fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
        analyze(document.text())
    }
}

fn analyze(text: &str) -> LanguageAnalysis {
    let mut analysis = LanguageAnalysis::default();
    let mut functions = BTreeMap::<String, TextRange>::new();
    let mut duplicate_functions = BTreeSet::<String>::new();
    let mut calls = Vec::<(String, TextRange)>::new();
    let mut brace_stack = Vec::<Position>::new();

    for (line_index, line) in text.lines().enumerate() {
        let trimmed_start = line.len() - line.trim_start().len();
        let trimmed = &line[trimmed_start..];

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((name, range)) = parse_function(line_index, line, trimmed_start, trimmed) {
            if let Some(first_range) = functions.get(&name) {
                duplicate_functions.insert(name.clone());
                analysis.diagnostics.push(
                    Diagnostic::error(
                        range,
                        format!(
                            "duplicate function `{name}`; first declared at line {}",
                            first_range.start.line + 1
                        ),
                    )
                    .with_source("scope.simple"),
                );
            } else {
                functions.insert(name.clone(), range);
            }

            analysis.symbols.push(Symbol {
                name,
                kind: SymbolKind::Function,
                range,
            });
        }

        if let Some((name, range)) = parse_let(line_index, line, trimmed_start, trimmed) {
            analysis.symbols.push(Symbol {
                name,
                kind: SymbolKind::Variable,
                range,
            });
        }

        collect_calls(line_index, line, &mut calls);
        update_braces(
            line_index,
            line,
            &mut brace_stack,
            &mut analysis.diagnostics,
        );
    }

    for position in brace_stack {
        analysis.diagnostics.push(
            Diagnostic::error(
                TextRange::new(position, Position::new(position.line, position.column + 1)),
                "unclosed `{`",
            )
            .with_source("scope.simple"),
        );
    }

    for (name, range) in calls {
        if !functions.contains_key(&name) && !duplicate_functions.contains(&name) {
            analysis.diagnostics.push(
                Diagnostic::warning(range, format!("call to unknown function `{name}`"))
                    .with_source("scope.simple"),
            );
        }
    }

    analysis
}

fn parse_function(
    line_index: usize,
    line: &str,
    trimmed_start: usize,
    trimmed: &str,
) -> Option<(String, TextRange)> {
    let rest = trimmed.strip_prefix("fn ")?;
    let name_start = trimmed_start + 3;
    let name_len = identifier_len(rest);

    if name_len == 0 {
        return None;
    }

    let after_name = &rest[name_len..];
    if !after_name.trim_start().starts_with('(') {
        return None;
    }

    let name = rest[..name_len].to_owned();
    let range = TextRange::new(
        Position::new(line_index, name_start),
        Position::new(line_index, name_start + name_len),
    );

    if !line[name_start + name_len..].contains('{') {
        return Some((name, range));
    }

    Some((name, range))
}

fn parse_let(
    line_index: usize,
    _line: &str,
    trimmed_start: usize,
    trimmed: &str,
) -> Option<(String, TextRange)> {
    let rest = trimmed.strip_prefix("let ")?;
    let name_start = trimmed_start + 4;
    let name_len = identifier_len(rest);

    if name_len == 0 {
        return None;
    }

    let after_name = &rest[name_len..];
    if !after_name.trim_start().starts_with('=') {
        return None;
    }

    Some((
        rest[..name_len].to_owned(),
        TextRange::new(
            Position::new(line_index, name_start),
            Position::new(line_index, name_start + name_len),
        ),
    ))
}

fn collect_calls(line_index: usize, line: &str, calls: &mut Vec<(String, TextRange)>) {
    let mut offset = 0;

    while offset < line.len() {
        let Some((relative_start, character)) = line[offset..].char_indices().next() else {
            break;
        };
        let start = offset + relative_start;

        if !is_identifier_start(character) {
            offset = start + character.len_utf8();
            continue;
        }

        let name_len = identifier_len(&line[start..]);
        let end = start + name_len;
        let after = &line[end..];
        let before = &line[..start];

        if after.trim_start().starts_with('(') && !before.trim_end().ends_with("fn") {
            let name = line[start..end].to_owned();
            calls.push((
                name,
                TextRange::new(
                    Position::new(line_index, start),
                    Position::new(line_index, end),
                ),
            ));
        }

        offset = end;
    }
}

fn update_braces(
    line_index: usize,
    line: &str,
    brace_stack: &mut Vec<Position>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (column, character) in line.char_indices() {
        match character {
            '{' => brace_stack.push(Position::new(line_index, column)),
            '}' if brace_stack.pop().is_none() => {
                diagnostics.push(
                    Diagnostic::error(
                        TextRange::new(
                            Position::new(line_index, column),
                            Position::new(line_index, column + 1),
                        ),
                        "unmatched `}`",
                    )
                    .with_source("scope.simple"),
                );
            }
            '}' => {}
            _ => {}
        }
    }
}

fn identifier_len(text: &str) -> usize {
    let mut length = 0;

    for (index, character) in text.char_indices() {
        if index == 0 {
            if !is_identifier_start(character) {
                return 0;
            }
        } else if !is_identifier_continue(character) {
            return length;
        }

        length = index + character.len_utf8();
    }

    length
}

fn is_identifier_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

fn is_identifier_continue(character: char) -> bool {
    is_identifier_start(character) || character.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn extracts_function_and_variable_symbols() {
        let document = Document::new(
            crate::document::DocumentId::new(1),
            "main.scope",
            "fn add(x, y) {\n  let result = x\n  result\n}\n",
        );

        let symbols = SimpleLanguage.outline(document.as_language_document());

        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "add");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
        assert_eq!(symbols[1].name, "result");
        assert_eq!(symbols[1].kind, SymbolKind::Variable);
    }

    #[test]
    fn reports_duplicate_functions_and_unknown_calls() {
        let document = Document::new(
            crate::document::DocumentId::new(1),
            "main.scope",
            "fn main() {\n  missing()\n}\nfn main() {}\n",
        );

        let diagnostics = SimpleLanguage.diagnostics(document.as_language_document());

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("unknown function `missing`"))
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("duplicate function `main`"))
        );
    }

    #[test]
    fn reports_unmatched_braces() {
        let document = Document::new(
            crate::document::DocumentId::new(1),
            "main.scope",
            "fn main() {\n  ok()\n",
        );

        let diagnostics = SimpleLanguage.diagnostics(document.as_language_document());

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == "unclosed `{`")
        );
    }
}
