//! Read-only navigation of native JSON result values, never parsed provider text.
use std::collections::BTreeSet;

use crossterm::event::KeyCode;
use serde_json::Value;

const MAX_ROWS: usize = 1024;
const MAX_DEPTH: usize = 32;

#[derive(Default)]
pub(super) struct ResultExplorer {
    selected: usize,
    expanded: BTreeSet<Vec<usize>>,
}

struct Row<'a> {
    path: Vec<usize>,
    label: String,
    depth: usize,
    value: &'a Value,
}

impl ResultExplorer {
    fn rows<'a>(&self, value: &'a Value) -> Vec<Row<'a>> {
        let mut rows = Vec::new();
        self.visit(value, &mut Vec::new(), "$".into(), &mut rows);
        rows
    }

    // Depth and emitted-node budgets bound recursion and work independently of
    // collection size. Ordinal identities never copy potentially enormous keys.
    fn visit<'a>(
        &self,
        value: &'a Value,
        path: &mut Vec<usize>,
        label: String,
        rows: &mut Vec<Row<'a>>,
    ) {
        if rows.len() >= MAX_ROWS {
            return;
        }
        rows.push(Row {
            path: path.clone(),
            label,
            depth: path.len(),
            value,
        });
        if path.len() >= MAX_DEPTH || !self.expanded.contains(path) {
            return;
        }
        match value {
            Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    if rows.len() >= MAX_ROWS {
                        break;
                    }
                    path.push(index);
                    self.visit(value, path, format!("[{index}]"), rows);
                    path.pop();
                }
            }
            Value::Object(values) => {
                for (index, (key, value)) in values.iter().enumerate() {
                    if rows.len() >= MAX_ROWS {
                        break;
                    }
                    path.push(index);
                    self.visit(value, path, preview(key), rows);
                    path.pop();
                }
            }
            _ => {}
        }
    }

    pub(super) fn key(&mut self, key: KeyCode, value: &Value) {
        let rows = self.rows(value);
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        let row = &rows[self.selected];
        match key {
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(rows.len() - 1),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = rows.len() - 1,
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
            KeyCode::PageDown => self.selected = (self.selected + 10).min(rows.len() - 1),
            KeyCode::Right | KeyCode::Enter if container(row.value) && row.depth < MAX_DEPTH => {
                if key == KeyCode::Enter && self.expanded.contains(&row.path) {
                    self.expanded.remove(&row.path);
                } else if self.expanded.len() < MAX_ROWS {
                    self.expanded.insert(row.path.clone());
                }
            }
            KeyCode::Left if !self.expanded.remove(&row.path) && row.depth > 0 => {
                self.selected = rows[..self.selected]
                    .iter()
                    .rposition(|parent| parent.depth < row.depth)
                    .unwrap_or(0);
            }
            _ => {}
        }
    }

    pub(super) fn text(&self, value: &Value, height: usize) -> String {
        let rows = self.rows(value);
        let selected = self.selected.min(rows.len() - 1);
        let mut text = format!(
            "Selected path: ${}\nSelected value: {}\n↑/↓ select • → expand • ← collapse/parent • Enter toggle • Esc back • d Details\n",
            pointer_preview(value, &rows[selected].path),
            scalar_preview(rows[selected].value)
        );
        let height = height.max(1);
        let start = selected.saturating_sub(height - 1);
        for (index, row) in rows.iter().enumerate().skip(start).take(height) {
            let indicator = if container(row.value) {
                if self.expanded.contains(&row.path) {
                    "▼"
                } else {
                    "▶"
                }
            } else {
                "·"
            };
            let scalar = scalar_preview(row.value);
            text.push_str(&format!(
                "{} {}{indicator} {}: {scalar}\n",
                if index == selected { ">" } else { " " },
                "  ".repeat(row.depth.min(8)),
                row.label
            ));
        }
        text.push_str("Bounded view: first 1024 visible nodes, depth 32; strings previewed to 120 characters.\n");
        text
    }
}

fn pointer_preview(mut value: &Value, path: &[usize]) -> String {
    let mut pointer = String::new();
    for &index in path {
        pointer.push('/');
        match value {
            Value::Array(values) => {
                pointer.push_str(&index.to_string());
                value = &values[index];
            }
            Value::Object(values) => {
                let (key, child) = values.iter().nth(index).expect("retained ordinal");
                let bounded: String = key.chars().take(120).collect();
                pointer.push_str(&bounded.replace('~', "~0").replace('/', "~1"));
                if key.chars().nth(120).is_some() {
                    pointer.push('…');
                }
                value = child;
            }
            _ => break,
        }
    }
    let bounded: String = pointer.chars().take(240).collect();
    let escaped = serde_json::to_string(&bounded).expect("string serializes");
    format!(
        "{}{}",
        &escaped[1..escaped.len() - 1],
        if pointer.chars().nth(240).is_some() {
            "…"
        } else {
            ""
        }
    )
}

fn scalar_preview(value: &Value) -> String {
    match value {
        Value::Array(values) => format!(
            "array [{}]{}",
            values.len(),
            if values.is_empty() { " (empty)" } else { "" }
        ),
        Value::Object(values) => format!(
            "object {{{}}}{}",
            values.len(),
            if values.is_empty() { " (empty)" } else { "" }
        ),
        Value::String(value) => preview(value),
        value => value.to_string(),
    }
}

fn container(value: &Value) -> bool {
    value.is_array() || value.is_object()
}

fn preview(value: &str) -> String {
    let mut chars = value.chars();
    let short: String = chars.by_ref().take(120).collect();
    let suffix = if chars.next().is_some() { "…" } else { "" };
    format!(
        "{}{}",
        serde_json::to_string(&short).expect("string serializes"),
        suffix
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_navigation_and_collapse_are_stable() {
        let value = json!({"a/b": {"~items": [42, "leaf"]}, "z": null});
        let mut tree = ResultExplorer::default();
        assert_eq!(tree.rows(&value).len(), 1);
        for key in [
            KeyCode::Right,
            KeyCode::Down,
            KeyCode::Right,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Down,
        ] {
            tree.key(key, &value);
        }
        assert!(
            tree.text(&value, 10)
                .contains("Selected path: $/a~1b/~0items/0")
        );
        tree.key(KeyCode::Left, &value);
        tree.key(KeyCode::Left, &value);
        assert!(
            tree.text(&value, 10)
                .contains("Selected path: $/a~1b/~0items\n")
        );
        assert_eq!(tree.rows(&value).len(), 4);
    }

    #[test]
    fn wide_containers_do_not_starve_nested_children_and_paths_are_bounded() {
        let mut values = vec![json!({"child": [17]})];
        values.extend(vec![Value::Null; 100000]);
        let value = Value::Array(values);
        let mut tree = ResultExplorer::default();
        for key in [KeyCode::Right, KeyCode::Down, KeyCode::Right] {
            tree.key(key, &value);
        }
        assert_eq!(tree.rows(&value)[2].label, "\"child\"");
        assert_eq!(tree.rows(&value).len(), MAX_ROWS);
        let value = json!({format!("\n{}", "x".repeat(1000000)): 17});
        tree = ResultExplorer::default();
        tree.key(KeyCode::Right, &value);
        tree.key(KeyCode::Down, &value);
        let text = tree.text(&value, 10);
        assert!(text.len() < 2000);
        assert!(text.contains("Selected path: $/\\n"));
        assert!(text.contains("Selected value: 17"));
        let mut value = json!(u64::MAX);
        for _ in 0..100 {
            value = json!([value]);
        }
        tree = ResultExplorer::default();
        for _ in 0..100 {
            tree.key(KeyCode::Right, &value);
            tree.key(KeyCode::Down, &value);
        }
        assert_eq!(tree.rows(&value).len(), MAX_DEPTH + 1);
        assert_eq!(scalar_preview(&json!(u64::MAX)), u64::MAX.to_string());
    }

    #[test]
    fn bounded_and_empty_scalar_previews() {
        let mut tree = ResultExplorer::default();
        let value = json!([true, null, -1, 1.25, "x".repeat(10000), [], {}]);
        tree.key(KeyCode::Right, &value);
        let text = tree.text(&value, 20);
        for expected in [
            "true",
            "null",
            "-1",
            "1.25",
            "…",
            "array [0] (empty)",
            "object {0} (empty)",
        ] {
            assert!(text.contains(expected), "{expected}");
        }
        assert!(text.len() < 2000);
        let large = Value::Array(vec![Value::Null; 100000]);
        assert_eq!(tree.rows(&large).len(), MAX_ROWS);
        assert_eq!(large.as_array().unwrap().len(), 100000);
        assert!(!preview("\n\u{1b}").contains('\u{1b}'));
    }
}
