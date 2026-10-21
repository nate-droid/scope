//! Bounded human-readable values and lossless JSON conversion.
//!
//! Human output is JSON when no limit is reached. Truncation markers deliberately
//! are not JSON; use [`value_json`] when a complete machine-readable value is needed.
//! Flat collections stay inline; collections with visible nonempty child
//! collections use two-space indentation.

use crate::Value;
use std::fmt::Write;

/// Limits for human output. String limits apply to object keys as well as values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatOptions {
    /// Maximum expanded collection depth; zero abbreviates every nonempty collection.
    pub max_depth: usize,
    /// Maximum entries shown in each array or object.
    pub max_collection_items: usize,
    /// Maximum Unicode scalar values shown in each string or object key.
    pub max_string_chars: usize,
    /// Global number of values visited, including collections and the root.
    pub max_nodes: usize,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_collection_items: 20,
            max_string_chars: 200,
            max_nodes: 200,
        }
    }
}

pub fn format_value(value: &Value) -> String {
    format_value_with(value, &FormatOptions::default())
}

pub fn format_value_with(value: &Value, options: &FormatOptions) -> String {
    let mut formatter = Formatter {
        options,
        remaining: options.max_nodes,
        output: String::new(),
    };
    formatter.value(value, 0);
    formatter.output
}

struct Formatter<'a> {
    options: &'a FormatOptions,
    remaining: usize,
    output: String,
}

impl Formatter<'_> {
    fn string(&mut self, text: &str) {
        let end = text
            .char_indices()
            .nth(self.options.max_string_chars)
            .map(|(i, _)| i);
        // Serialize only the bounded prefix, never an arbitrarily large input.
        self.output.push_str(
            &serde_json::to_string(&text[..end.unwrap_or(text.len())])
                .expect("serializing a string cannot fail"),
        );
        if end.is_some() {
            self.output.push_str("<string truncated>");
        }
    }

    fn value(&mut self, value: &Value, depth: usize) {
        if self.remaining == 0 {
            self.output.push_str("<node budget exhausted>");
            return;
        }
        self.remaining -= 1;
        match value {
            Value::Null => self.output.push_str("null"),
            Value::Boolean(value) => self.output.push_str(if *value { "true" } else { "false" }),
            Value::Integer(value) => write!(self.output, "{value}").unwrap(),
            Value::String(value) => self.string(value),
            Value::Array(values) => {
                self.output.push('[');
                let multiline = self.multiline(values.iter(), depth);
                self.collection(values.len(), depth, multiline, |this, index| {
                    this.value(&values[index], depth + 1);
                });
                self.output.push(']');
            }
            Value::Object(values) => {
                self.output.push('{');
                let multiline = self.multiline(values.values(), depth);
                let mut entries = values.iter();
                self.collection(values.len(), depth, multiline, |this, _| {
                    let (key, value) = entries.next().unwrap();
                    this.string(key);
                    this.output.push_str(": ");
                    this.value(value, depth + 1);
                });
                self.output.push('}');
            }
        }
    }

    fn multiline<'a>(&self, values: impl Iterator<Item = &'a Value>, depth: usize) -> bool {
        // Layout lookahead must be bounded too, even for enormous flat collections.
        depth < self.options.max_depth
            && values
                .take(self.options.max_collection_items.min(self.remaining))
                .any(|value| match value {
                    Value::Array(values) => !values.is_empty(),
                    Value::Object(values) => !values.is_empty(),
                    _ => false,
                })
    }

    fn newline(&mut self, depth: usize) {
        self.output.push('\n');
        for _ in 0..depth {
            self.output.push_str("  ");
        }
    }

    fn separator(&mut self, shown: usize, depth: usize, multiline: bool) {
        if shown > 0 {
            self.output.push(',');
            if !multiline {
                self.output.push(' ');
            }
        }
        if multiline {
            self.newline(depth + 1);
        }
    }

    fn collection(
        &mut self,
        len: usize,
        depth: usize,
        multiline: bool,
        mut entry: impl FnMut(&mut Self, usize),
    ) {
        if len == 0 {
            return;
        }
        if depth >= self.options.max_depth {
            self.output.push_str("<depth truncated>");
            return;
        }
        let mut shown = 0;
        while shown < len && shown < self.options.max_collection_items && self.remaining > 0 {
            self.separator(shown, depth, multiline);
            entry(self, shown);
            shown += 1;
        }
        if shown < len {
            self.separator(shown, depth, multiline);
            let reason = if self.remaining == 0 {
                "node budget exhausted"
            } else {
                "items truncated"
            };
            write!(self.output, "<{reason}: {} omitted>", len - shown).unwrap();
        }
        if multiline {
            self.newline(depth);
        }
    }
}

/// Convert the complete value without presentation limits, preserving signed i64s.
/// Like the input value itself, this recursive conversion requires stack space
/// proportional to the nesting depth.
pub fn value_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Boolean(value) => (*value).into(),
        Value::Integer(value) => (*value).into(),
        Value::String(value) => value.clone().into(),
        Value::Array(values) => values.iter().map(value_json).collect(),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| (key.clone(), value_json(value)))
            .collect(),
    }
}

/// Return a one-based (line, Unicode-scalar column) for a byte offset.
/// Newlines are LF (CR in CRLF occupies a column). Out-of-range offsets clamp
/// to EOF; offsets inside a UTF-8 character clamp to that character's start.
pub fn source_location(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let mut line = 1;
    let mut column = 1;
    for (index, ch) in source.char_indices() {
        if index + ch.len_utf8() > offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn object(entries: &[(&str, Value)]) -> Value {
        Value::Object(
            entries
                .iter()
                .map(|(key, value)| ((*key).into(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn full_json_roundtrip_preserves_integers_and_escapes() {
        let value = object(&[
            (
                "quote\"\n雪",
                Value::String("é雪😀\"\\\n\r\t\0\u{0008}\u{000c}".into()),
            ),
            (
                "values",
                Value::Array(vec![
                    Value::Null,
                    Value::Boolean(true),
                    Value::Boolean(false),
                    Value::Integer(i64::MIN),
                    Value::Integer(i64::MAX),
                ]),
            ),
        ]);
        let json = value_json(&value);
        let human = format_value(&value);
        assert!(human.contains("é雪😀"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&human).unwrap(),
            json
        );
        assert_eq!(json["values"][3].as_i64(), Some(i64::MIN));
        assert_eq!(json["values"][4].as_i64(), Some(i64::MAX));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json.to_string()).unwrap(),
            json
        );
    }

    #[test]
    fn empty_collections_need_no_expansion() {
        let options = FormatOptions {
            max_depth: 0,
            max_collection_items: 0,
            ..Default::default()
        };
        assert_eq!(format_value_with(&Value::Array(vec![]), &options), "[]");
        assert_eq!(format_value_with(&object(&[]), &options), "{}");
        assert_eq!(format_value(&Value::String(String::new())), "\"\"");
    }

    #[test]
    fn deep_arrays_and_objects_stop_at_depth() {
        let mut value = Value::Null;
        for _ in 0..100 {
            value = object(&[("nested", Value::Array(vec![value]))]);
        }
        let output = format_value(&value);
        assert!(output.contains("<depth truncated>"));
        assert!(output.len() < 400);
        assert!(serde_json::from_str::<serde_json::Value>(&output).is_err());
    }

    #[test]
    fn strings_and_keys_are_bounded_by_characters() {
        let options = FormatOptions {
            max_string_chars: 2,
            ..Default::default()
        };
        assert_eq!(
            format_value_with(&Value::String("é😀雪".into()), &options),
            "\"é😀\"<string truncated>"
        );
        assert_eq!(
            format_value_with(&Value::String("é😀".into()), &options),
            "\"é😀\""
        );
        assert!(
            format_value_with(
                &object(&[("雪".repeat(100_000).as_str(), Value::Null)]),
                &options
            )
            .len()
                < 100
        );
        let zero = FormatOptions {
            max_string_chars: 0,
            ..options
        };
        assert_eq!(
            format_value_with(&Value::String("x".into()), &zero),
            "\"\"<string truncated>"
        );
    }

    #[test]
    fn huge_collections_and_global_budget() {
        let large = Value::Array(vec![Value::Integer(1); 100_000]);
        assert!(format_value(&large).contains("<items truncated: 99980 omitted>"));
        let tree = Value::Array(vec![Value::Array(vec![Value::Null; 100]); 100]);
        let options = FormatOptions {
            max_nodes: 5,
            max_collection_items: usize::MAX,
            ..Default::default()
        };
        let output = format_value_with(&tree, &options);
        assert_eq!(
            output,
            "[\n  [null, null, null, <node budget exhausted: 97 omitted>],\n  <node budget exhausted: 99 omitted>\n]"
        );
        let options = FormatOptions {
            max_nodes: 0,
            ..options
        };
        assert_eq!(
            format_value_with(&tree, &options),
            "<node budget exhausted>"
        );
        let options = FormatOptions {
            max_collection_items: 0,
            ..Default::default()
        };
        assert_eq!(
            format_value_with(&object(&[("a", Value::Null)]), &options),
            "{<items truncated: 1 omitted>}"
        );
    }

    #[test]
    fn flat_collections_and_empty_children_stay_inline() {
        assert_eq!(
            format_value(&object(&[
                ("a", Value::Integer(1)),
                ("b", Value::Boolean(true))
            ])),
            "{\"a\": 1, \"b\": true}"
        );
        assert_eq!(
            format_value(&Value::Array(vec![
                Value::Null,
                Value::Array(vec![]),
                object(&[])
            ])),
            "[null, [], {}]"
        );
    }

    #[test]
    fn nested_collections_use_two_space_indentation() {
        let value = object(&[
            (
                "a",
                Value::Array(vec![object(&[(
                    "b",
                    Value::Array(vec![Value::Integer(1), Value::Integer(2)]),
                )])]),
            ),
            ("z", Value::Null),
        ]);
        let output = format_value(&value);
        assert_eq!(
            output,
            "{\n  \"a\": [\n    {\n      \"b\": [1, 2]\n    }\n  ],\n  \"z\": null\n}"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            value_json(&value)
        );
    }

    #[test]
    fn multiline_truncation_keeps_indentation_and_closing_delimiters() {
        let value = Value::Array(vec![Value::Array(vec![Value::Null]), Value::Null]);
        let options = FormatOptions {
            max_collection_items: 1,
            ..Default::default()
        };
        assert_eq!(
            format_value_with(&value, &options),
            "[\n  [null],\n  <items truncated: 1 omitted>\n]"
        );
        let options = FormatOptions {
            max_depth: 1,
            ..Default::default()
        };
        assert_eq!(
            format_value_with(&value, &options),
            "[\n  [<depth truncated>],\n  null\n]"
        );
        let options = FormatOptions {
            max_depth: 0,
            ..options
        };
        assert_eq!(format_value_with(&value, &options), "[<depth truncated>]");
    }

    #[test]
    fn locations_are_one_based_and_utf8_safe() {
        assert_eq!(source_location("", usize::MAX), (1, 1));
        let text = "é😀\n雪\r\nx";
        assert_eq!(source_location(text, 0), (1, 1));
        assert_eq!(source_location(text, 1), (1, 1));
        assert_eq!(source_location(text, 2), (1, 2));
        assert_eq!(source_location(text, 5), (1, 2));
        assert_eq!(source_location(text, 6), (1, 3));
        assert_eq!(source_location(text, 7), (2, 1));
        assert_eq!(source_location(text, 11), (2, 3));
        assert_eq!(source_location(text, 12), (3, 1));
        assert_eq!(source_location(text, usize::MAX), (3, 2));
    }
}
