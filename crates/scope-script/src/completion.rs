//! Read-only, lexical completion of names and direct `scope.invoke` string arguments.
//! Does not evaluate expressions, infer object fields, or resolve provider expressions.
use crate::{Host, Interpreter};
use scope_terminal::{CompletionCandidate, CompletionProvider};
use std::ops::Range;

/// Borrows current bindings and host metadata without executing script or host calls.
pub struct ScriptCompletionProvider<'a, H: Host + ?Sized> {
    interpreter: &'a Interpreter,
    host: &'a H,
}

impl<'a, H: Host + ?Sized> ScriptCompletionProvider<'a, H> {
    pub fn new(interpreter: &'a Interpreter, host: &'a H) -> Self {
        Self { interpreter, host }
    }
}

impl<H: Host + ?Sized> CompletionProvider for ScriptCompletionProvider<'_, H> {
    fn complete(&mut self, input: &str, cursor: usize) -> Vec<CompletionCandidate> {
        if !input.is_char_boundary(cursor) {
            return Vec::new();
        }
        let tokens = tokens(input);
        for (index, token) in tokens.iter().enumerate() {
            if token.string && token.range.start < cursor && cursor <= token.content_end {
                let before = &tokens[..index];
                let provider = if invoke_start(before, input) {
                    None
                } else if before.len() >= 2
                    && before[before.len() - 1].text(input) == ","
                    && before[before.len() - 2].string
                    && invoke_start(&before[..before.len() - 2], input)
                {
                    let previous = &before[before.len() - 2];
                    let Some(provider) =
                        decode(&input[previous.range.start + 1..previous.content_end])
                    else {
                        return Vec::new();
                    };
                    Some(provider)
                } else {
                    return Vec::new();
                };
                let Some(prefix) = decode(&input[token.range.start + 1..cursor]) else {
                    return Vec::new();
                };
                let names = self
                    .host
                    .operation_names()
                    .into_iter()
                    .filter_map(|(p, a)| match &provider {
                        None => Some(p),
                        Some(provider) if provider == &p => Some(a),
                        _ => None,
                    });
                let candidates = names
                    .filter(|name| name.starts_with(&prefix))
                    .filter_map(|name| {
                        quote(&name).map(|insertion| CompletionCandidate {
                            replacement: token.range.clone(),
                            insertion,
                            label: None,
                        })
                    })
                    .collect();
                return sorted(candidates);
            }
        }

        let name_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
        let start = input[..cursor]
            .rfind(|c| !name_char(c))
            .map_or(0, |i| i + input[i..].chars().next().unwrap().len_utf8());
        let end = input[cursor..]
            .find(|c| !name_char(c))
            .map_or(input.len(), |i| cursor + i);
        let prefix = &input[start..cursor];
        let mut names = vec![
            "let",
            "true",
            "false",
            "null",
            "print",
            "scope.operations",
            "scope.invoke",
        ];
        names.extend(self.interpreter.bindings.keys().map(String::as_str));
        sorted(
            names
                .into_iter()
                .filter(|name| name.starts_with(prefix))
                .map(|name| CompletionCandidate {
                    replacement: start..end,
                    insertion: name.into(),
                    label: None,
                })
                .collect(),
        )
    }
}

fn sorted(mut candidates: Vec<CompletionCandidate>) -> Vec<CompletionCandidate> {
    candidates.sort_by(|a, b| a.insertion.cmp(&b.insertion));
    candidates.dedup_by(|a, b| a.insertion == b.insertion);
    candidates
}

struct Token {
    range: Range<usize>,
    content_end: usize,
    string: bool,
}
impl Token {
    fn text<'a>(&self, input: &'a str) -> &'a str {
        &input[self.range.clone()]
    }
}

// Only enough lexing to distinguish strings, punctuation, and direct call names.
// Strings consume escaped quotes even when the rest of the input is incomplete.
fn tokens(input: &str) -> Vec<Token> {
    let mut chars = input.char_indices().peekable();
    let mut result = Vec::new();
    while let Some((start, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        let mut end = start + c.len_utf8();
        let mut content_end = end;
        if c == '"' {
            content_end = input.len();
            while let Some((at, next)) = chars.next() {
                end = at + next.len_utf8();
                if next == '\\' {
                    if let Some((at, escaped)) = chars.next() {
                        end = at + escaped.len_utf8();
                    }
                } else if next == '"' {
                    content_end = at;
                    break;
                }
            }
        } else if c.is_ascii_alphanumeric() || c == '_' {
            while chars
                .peek()
                .is_some_and(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
            {
                let (at, next) = chars.next().unwrap();
                end = at + next.len_utf8();
            }
        }
        result.push(Token {
            range: start..end,
            content_end,
            string: c == '"',
        });
    }
    result
}

fn invoke_start(tokens: &[Token], input: &str) -> bool {
    let Some(start) = tokens.len().checked_sub(4) else {
        return false;
    };
    tokens[start..]
        .iter()
        .zip(["scope", ".", "invoke", "("])
        .all(|(token, expected)| !token.string && token.text(input) == expected)
        && (start == 0 || tokens[start - 1].text(input) != ".")
}

fn decode(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        result.push(if c == '\\' {
            match chars.next()? {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                _ => return None,
            }
        } else if c.is_control() {
            return None;
        } else {
            c
        });
    }
    Some(result)
}

fn quote(text: &str) -> Option<String> {
    let mut result = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => return None,
            c => result.push(c),
        }
    }
    result.push('"');
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorKind, Value, host::NoHost};

    struct MetadataHost;
    impl Host for MetadataHost {
        fn operation_names(&self) -> Vec<(String, String)> {
            [
                ("zeta", "read"),
                ("alpha", "write"),
                ("alpha", "read"),
                ("alpha", "read"),
                ("quoted\"\\\n", "café"),
                ("bad\u{1b}", "skip"),
            ]
            .into_iter()
            .map(|(p, a)| (p.into(), a.into()))
            .collect()
        }
        fn operations(&mut self) -> Result<Value, ErrorKind> {
            panic!("completion must not execute")
        }
        fn invoke(&mut self, _: &str, _: &str, _: Value) -> Result<Value, ErrorKind> {
            panic!("completion must not invoke")
        }
    }

    fn complete(input: &str, cursor: usize) -> Vec<CompletionCandidate> {
        ScriptCompletionProvider::new(&Interpreter::new(), &MetadataHost).complete(input, cursor)
    }
    fn insertions(candidates: &[CompletionCandidate]) -> Vec<&str> {
        candidates.iter().map(|c| c.insertion.as_str()).collect()
    }

    #[test]
    fn keywords_builtins_and_current_bindings_are_sorted() {
        let mut interpreter = Interpreter::new();
        interpreter
            .eval("let zebra = 1; let alpha = 2; print(alpha)", &mut NoHost)
            .unwrap();
        let bindings = interpreter.bindings.clone();
        let output = interpreter.output.clone();
        let candidates = ScriptCompletionProvider::new(&interpreter, &MetadataHost).complete("", 0);
        assert_eq!(
            insertions(&candidates),
            [
                "alpha",
                "false",
                "let",
                "null",
                "print",
                "scope.invoke",
                "scope.operations",
                "true",
                "zebra"
            ]
        );
        assert_eq!(interpreter.bindings, bindings);
        assert_eq!(interpreter.output, output);
        assert_eq!(insertions(&complete("scope.i", 7)), ["scope.invoke"]);
    }

    #[test]
    fn replaces_entire_name_at_intermediate_cursor() {
        let candidates = complete("print(scope.incorrect)", 13);
        assert_eq!(insertions(&candidates), ["scope.invoke"]);
        assert_eq!(candidates[0].replacement, 6..21);
        let candidates = complete("trailing", 2);
        assert_eq!(insertions(&candidates), ["true"]);
        assert_eq!(candidates[0].replacement, 0..8);
    }

    #[test]
    fn strings_are_suppressed_except_direct_invoke_arguments() {
        for input in [
            "\"scope.i",
            "print(\"a",
            "{key: \"a",
            "scope.invoke(\"alpha\", \"read\", {key: \"a",
            "other.invoke(\"a",
            "object.scope.invoke(\"a",
            "scope.invoke(provider, \"r",
            "scope.invoke(\"alpha\" + suffix, \"r",
            "\"escaped\\\"scope.invoke(\\\"a",
        ] {
            assert!(complete(input, input.len()).is_empty(), "{input}");
        }
        assert!(
            ScriptCompletionProvider::new(&Interpreter::new(), &NoHost)
                .complete("scope.invoke(\"", 14)
                .is_empty()
        );
    }

    #[test]
    fn provider_and_action_context_is_filtered_sorted_and_deduplicated() {
        let input = "print(scope . invoke ( \"a";
        assert_eq!(insertions(&complete(input, input.len())), ["\"alpha\""]);
        let input = "scope.invoke(\"alpha\", \"";
        assert_eq!(
            insertions(&complete(input, input.len())),
            ["\"read\"", "\"write\""]
        );
        let input = "scope.invoke(\"unknown\", \"";
        assert!(complete(input, input.len()).is_empty());
        let input = "scope.invoke(\"zeta\", \"r";
        assert_eq!(insertions(&complete(input, input.len())), ["\"read\""]);
    }

    #[test]
    fn replaces_whole_string_including_quotes_and_suffix() {
        let input = "scope.invoke(\"alwrong\", \"read\", {})";
        let candidates = complete(input, 16);
        assert_eq!(insertions(&candidates), ["\"alpha\""]);
        assert_eq!(candidates[0].replacement, 13..22);
        let mut applied = input.to_owned();
        applied.replace_range(candidates[0].replacement.clone(), &candidates[0].insertion);
        assert_eq!(applied, "scope.invoke(\"alpha\", \"read\", {})");
        let input = "scope.invoke(\"alpha\", \"rubbish\")";
        let candidates = complete(input, 24);
        assert_eq!(insertions(&candidates), ["\"read\""]);
        assert_eq!(&input[candidates[0].replacement.clone()], "\"rubbish\"");
    }

    #[test]
    fn escapes_metadata_and_decodes_literal_provider() {
        let input = "scope.invoke(\"";
        assert_eq!(
            insertions(&complete(input, input.len())),
            ["\"alpha\"", "\"quoted\\\"\\\\\\n\"", "\"zeta\""]
        );
        let input = "scope.invoke(\"quoted\\\"\\\\\\n\", \"ca";
        assert_eq!(insertions(&complete(input, input.len())), ["\"café\""]);
        let input = "scope.invoke(\"bad\\q";
        assert!(complete(input, input.len()).is_empty());
        let input = "scope.invoke(\"quoted\\";
        assert!(complete(input, input.len()).is_empty());
    }

    #[test]
    fn utf8_and_invalid_cursors_are_safe() {
        let input = "\"é\"; scope.i";
        assert_eq!(insertions(&complete(input, input.len())), ["scope.invoke"]);
        for cursor in [2, input.len() + 1, usize::MAX] {
            assert!(complete(input, cursor).is_empty());
        }
        let input = "scope.invoke(\"quoted\\\"\\\\\\n\", \"caféwrong\")";
        let cursor = input.find("é").unwrap() + "é".len();
        let candidates = complete(input, cursor);
        assert_eq!(insertions(&candidates), ["\"café\""]);
        assert_eq!(&input[candidates[0].replacement.clone()], "\"caféwrong\"");
    }
}
