//! Owned, bounded structural comparison; never retains or clones input results.
use std::cmp::Ordering;

use crossterm::event::KeyCode;
use serde_json::Value;

const MAX_NODES: usize = 1024;
const MAX_DEPTH: usize = 32;
const MAX_BYTES: usize = 64 * 1024;
const MAX_PATH: usize = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Added,
    Removed,
    Changed,
    Unchanged,
    Unknown,
}

impl Status {
    fn difference(self) -> bool {
        matches!(self, Self::Added | Self::Removed | Self::Changed)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
            Self::Unchanged => "unchanged",
            Self::Unknown => "unknown",
        }
    }
}

struct Row {
    path: String,
    old: String,
    new: String,
    status: Status,
    incomplete: bool,
}

pub(super) struct Comparison {
    rows: Vec<Row>,
    selected: usize,
}

impl Comparison {
    pub(super) fn new(old: &Value, new: &Value) -> Self {
        let mut comparison = Self {
            rows: Vec::new(),
            selected: 0,
        };
        let mut bytes = MAX_BYTES;
        comparison.visit(Some(old), Some(new), "$".into(), 0, &mut bytes);
        comparison.selected = comparison
            .rows
            .iter()
            .position(|row| row.status.difference())
            .unwrap_or(0);
        comparison
    }

    pub(super) fn key(&mut self, key: KeyCode) {
        let last = self.rows.len().saturating_sub(1);
        match key {
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(last),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = last,
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
            KeyCode::PageDown => self.selected = (self.selected + 10).min(last),
            KeyCode::Char('n') | KeyCode::Char('p') => {
                let len = self.rows.len();
                for step in 1..=len {
                    let index = if key == KeyCode::Char('n') {
                        (self.selected + step) % len
                    } else {
                        (self.selected + len - step) % len
                    };
                    if self.rows[index].status.difference() {
                        self.selected = index;
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn text(&self, height: usize) -> String {
        let row = &self.rows[self.selected];
        let mut text = format!(
            "Comparison: {}{}\nSelected path: {}\nOld: {}\nNew: {}\n↑/↓ select • n/p next/previous difference • Home/End • Esc back\n",
            self.rows[0].status.label(),
            if self.rows[0].incomplete {
                " (incomplete)"
            } else {
                ""
            },
            display_path(&row.path),
            row.old,
            row.new,
        );
        let height = height.clamp(1, MAX_NODES);
        let start = self
            .selected
            .saturating_sub(height / 2)
            .min(self.rows.len().saturating_sub(height));
        for (index, row) in self.rows.iter().enumerate().skip(start).take(height) {
            text.push_str(&format!(
                "{} {}{} {}: {} → {}\n",
                if index == self.selected { ">" } else { " " },
                row.status.label(),
                if row.incomplete { " (incomplete)" } else { "" },
                display_path(&row.path),
                row.old,
                row.new
            ));
        }
        text.push_str("Bounded comparison: 1024 nodes, depth 32, 65536 comparison bytes; paths and previews may be truncated.\n");
        text
    }

    fn visit(
        &mut self,
        old: Option<&Value>,
        new: Option<&Value>,
        path: String,
        depth: usize,
        bytes: &mut usize,
    ) -> (bool, bool) {
        if self.rows.len() == MAX_NODES {
            return (false, true);
        }
        let index = self.rows.len();
        let mut status = match (old, new) {
            (None, Some(_)) => Status::Added,
            (Some(_), None) => Status::Removed,
            _ => Status::Unchanged,
        };
        self.rows.push(Row {
            path: path.clone(),
            old: preview(old),
            new: preview(new),
            status,
            incomplete: false,
        });
        let mut incomplete = false;
        let mut changed = status.difference();
        match (old, new) {
            (Some(Value::Array(a)), Some(Value::Array(b))) => {
                changed |= a.len() != b.len();
                if depth == MAX_DEPTH {
                    incomplete = !a.is_empty() || !b.is_empty();
                } else {
                    for i in 0..a.len().max(b.len()) {
                        if self.rows.len() == MAX_NODES {
                            incomplete = true;
                            break;
                        }
                        let (diff, partial) = self.visit(
                            a.get(i),
                            b.get(i),
                            child_path(&path, &i.to_string()),
                            depth + 1,
                            bytes,
                        );
                        changed |= diff;
                        incomplete |= partial;
                    }
                }
            }
            (Some(Value::Object(a)), Some(Value::Object(b))) => {
                changed |= a.len() != b.len();
                if depth == MAX_DEPTH || a.len().max(b.len()) > MAX_NODES {
                    incomplete = !a.is_empty() || !b.is_empty();
                } else {
                    // References only; sorting is bounded in both entry count and key-byte work.
                    let mut entries: Vec<_> = a
                        .iter()
                        .map(|(k, v)| (k.as_str(), Some(v), None))
                        .chain(b.iter().map(|(k, v)| (k.as_str(), None, Some(v))))
                        .collect();
                    // Stop sorting immediately on exhaustion rather than supplying
                    // an inconsistent comparator to a standard sorting algorithm.
                    'sort: for i in 1..entries.len() {
                        let mut j = i;
                        while j > 0 {
                            match compare_bytes(entries[j - 1].0, entries[j].0, bytes) {
                                Some(Ordering::Greater) => entries.swap(j - 1, j),
                                Some(_) => break,
                                None => {
                                    incomplete = true;
                                    break 'sort;
                                }
                            }
                            j -= 1;
                        }
                    }
                    // A budget-exhausted ordering is not a trustworthy key union.
                    if !incomplete {
                        let mut i = 0;
                        while i < entries.len() {
                            if self.rows.len() == MAX_NODES {
                                incomplete = true;
                                break;
                            }
                            let (key, mut left, mut right) = entries[i];
                            i += 1;
                            if i < entries.len() {
                                match compare_bytes(key, entries[i].0, bytes) {
                                    Some(Ordering::Equal) => {
                                        left = left.or(entries[i].1);
                                        right = right.or(entries[i].2);
                                        i += 1;
                                    }
                                    None => {
                                        incomplete = true;
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            let (diff, partial) =
                                self.visit(left, right, child_path(&path, key), depth + 1, bytes);
                            changed |= diff;
                            incomplete |= partial;
                        }
                    }
                }
            }
            (Some(Value::String(a)), Some(Value::String(b))) => {
                if a.len() != b.len() {
                    changed = true;
                } else {
                    match compare_bytes(a, b, bytes) {
                        Some(order) => changed = order != Ordering::Equal,
                        None => incomplete = true,
                    }
                }
            }
            (Some(Value::Null), Some(Value::Null)) => {}
            (Some(Value::Bool(a)), Some(Value::Bool(b))) => changed = a != b,
            (Some(Value::Number(a)), Some(Value::Number(b))) => changed = a != b,
            (Some(_), Some(_)) => changed = true,
            // A missing subtree is already an exact structural difference. Its summary
            // needs no recursive scan, even when the present value is a container.
            _ => {}
        }
        if !status.difference() {
            status = if changed {
                Status::Changed
            } else if incomplete {
                Status::Unknown
            } else {
                Status::Unchanged
            };
        }
        self.rows[index].status = status;
        self.rows[index].incomplete = incomplete;
        (changed, incomplete)
    }
}

fn compare_bytes(a: &str, b: &str, budget: &mut usize) -> Option<Ordering> {
    for (left, right) in a.bytes().zip(b.bytes()) {
        if *budget < 2 {
            return None;
        }
        *budget -= 2;
        if left != right {
            return Some(left.cmp(&right));
        }
    }
    Some(a.len().cmp(&b.len()))
}

fn child_path(parent: &str, key: &str) -> String {
    // Keep raw pointer text until rendering so escaping cannot cut an escape sequence.
    let mut path: String = parent.chars().take(MAX_PATH).collect();
    if path.ends_with('…') {
        return path;
    }
    path.push('/');
    for ch in key.chars().take(MAX_PATH + 1) {
        match ch {
            '~' => path.push_str("~0"),
            '/' => path.push_str("~1"),
            ch => path.push(ch),
        }
        if path.chars().count() > MAX_PATH {
            path = path.chars().take(MAX_PATH).collect();
            path.push('…');
            break;
        }
    }
    path
}

fn display_path(path: &str) -> String {
    let escaped = serde_json::to_string(path).expect("string serializes");
    escaped[1..escaped.len() - 1].to_owned()
}

fn quoted(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let short: String = chars.by_ref().take(limit).collect();
    let suffix = if chars.next().is_some() { "…" } else { "" };
    format!(
        "{}{suffix}",
        serde_json::to_string(&short).expect("string serializes")
    )
}

fn preview(value: Option<&Value>) -> String {
    match value {
        None => "(missing)".into(),
        Some(Value::String(s)) => quoted(s, 120),
        Some(Value::Array(a)) => format!(
            "array [{}]{}",
            a.len(),
            if a.is_empty() { " (empty)" } else { "" }
        ),
        Some(Value::Object(o)) => format!(
            "object {{{}}}{}",
            o.len(),
            if o.is_empty() { " (empty)" } else { "" }
        ),
        Some(value) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn structural_rows_and_pointer_paths() {
        let c = Comparison::new(
            &json!({"a/b": {"~": [1, 2]}, "gone": null}),
            &json!({"a/b": {"~": [1, 3, 4]}, "new": false}),
        );
        for (path, status) in [
            ("$/a~1b/~0/0", Status::Unchanged),
            ("$/a~1b/~0/1", Status::Changed),
            ("$/a~1b/~0/2", Status::Added),
            ("$/gone", Status::Removed),
            ("$/new", Status::Added),
        ] {
            assert_eq!(
                c.rows.iter().find(|r| r.path == path).unwrap().status,
                status
            );
        }
        assert_eq!(c.rows[0].status, Status::Changed);
        assert!(!c.rows[0].incomplete);
    }

    #[test]
    fn scalar_types_empty_containers_and_ownership() {
        for value in [
            json!(null),
            json!(true),
            json!(42),
            json!(1.25),
            json!("hi"),
            json!([]),
            json!({}),
        ] {
            let c = Comparison::new(&value, &value);
            assert_eq!(c.rows[0].status, Status::Unchanged);
            assert!(!c.rows[0].incomplete);
        }
        for (a, b) in [
            (json!(1), json!("1")),
            (json!([]), json!({})),
            (json!(false), json!(true)),
        ] {
            assert_eq!(Comparison::new(&a, &b).rows[0].status, Status::Changed);
        }
        let c = {
            let a = json!([1]);
            Comparison::new(&a, &json!([2]))
        };
        assert!(c.text(10).contains("1 → 2"));
    }

    #[test]
    fn node_and_depth_limits_preserve_known_differences() {
        let a = json!(vec![0; MAX_NODES * 2]);
        let mut b = a.clone();
        b[0] = json!(1);
        let c = Comparison::new(&a, &b);
        assert_eq!(c.rows.len(), MAX_NODES);
        assert_eq!(c.rows[0].status, Status::Changed);
        assert!(c.rows[0].incomplete);
        let mut deep = json!(0);
        for _ in 0..100 {
            deep = json!([deep]);
        }
        let c = Comparison::new(&deep, &deep);
        assert_eq!(c.rows.len(), MAX_DEPTH + 1);
        assert_eq!(c.rows[0].status, Status::Unknown);
        assert!(c.text(0).contains("incomplete"));
    }

    #[test]
    fn string_and_key_work_is_bounded() {
        let huge = "x".repeat(MAX_BYTES * 2);
        let c = Comparison::new(&json!(huge), &json!(huge));
        assert_eq!(c.rows[0].status, Status::Unknown);
        assert!(c.rows[0].incomplete);
        assert!(c.text(usize::MAX).len() < 3000);
        assert_eq!(
            Comparison::new(&json!(huge), &json!("x")).rows[0].status,
            Status::Changed
        );
        let value = json!({huge: 1});
        assert!(Comparison::new(&value, &value).rows[0].incomplete);
        let mut budget = 2;
        assert_eq!(compare_bytes("abc", "axc", &mut budget), None);
        assert_eq!(budget, 0);
    }

    #[test]
    fn ordering_navigation_and_render_bounds() {
        let mut c = Comparison::new(
            &json!({"z": 0, "a": 0, "b": 0}),
            &json!({"b": 1, "a": 0, "z": 2}),
        );
        assert_eq!(
            c.rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            ["$", "$/a", "$/b", "$/z"]
        );
        c.key(KeyCode::Char('n'));
        assert_eq!(c.selected, 2);
        c.key(KeyCode::Char('p'));
        assert_eq!(c.selected, 0);
        c.key(KeyCode::Char('p'));
        assert_eq!(c.selected, 3);
        c.key(KeyCode::Home);
        c.key(KeyCode::Up);
        assert_eq!(c.selected, 0);
        c.key(KeyCode::PageDown);
        assert_eq!(c.selected, 3);
        c.key(KeyCode::PageUp);
        assert_eq!(c.selected, 0);
        c.key(KeyCode::Down);
        assert!(c.text(1).contains("> unchanged $/a"));
        c.key(KeyCode::End);
        c.key(KeyCode::Down);
        assert_eq!(c.selected, 3);
        let c = Comparison::new(&json!("\n\u{1b}"), &json!(null));
        assert!(!c.text(1).contains('\u{1b}'));
    }

    #[test]
    fn path_previews_are_bounded() {
        let key = format!("\n{}", "界/~".repeat(10000));
        let c = Comparison::new(&json!({}), &json!({key: 1}));
        assert!(
            c.rows
                .iter()
                .all(|r| r.path.chars().count() <= MAX_PATH + 1)
        );
        assert!(c.text(usize::MAX).len() < 10000);
    }
}
