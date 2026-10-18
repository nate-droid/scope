//! Language-neutral command recall, with optional capability-backed persistence.

/// Oldest-to-newest command history. Only consecutive duplicates are suppressed.
#[derive(Debug, Clone)]
pub struct CommandHistory {
    max: usize,
    entries: Vec<String>,
    cursor: Option<usize>,
    draft: String,
}

impl CommandHistory {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            entries: Vec::new(),
            cursor: None,
            draft: String::new(),
        }
    }

    /// Records verbatim nonblank input, resetting navigation even if rejected.
    pub fn record(&mut self, command: &str) -> bool {
        self.reset_navigation();
        if self.max == 0
            || command.trim().is_empty()
            || self.entries.last().is_some_and(|last| last == command)
        {
            return false;
        }
        if self.entries.len() == self.max {
            self.entries.remove(0);
        }
        self.entries.push(command.to_owned());
        true
    }

    /// Captures the current draft on first use. Edits to recalled commands do
    /// not overwrite recorded entries or the draft. At the oldest entry, stays put.
    pub fn previous(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let index = match self.cursor {
            None => {
                self.draft = current.to_owned();
                self.entries.len() - 1
            }
            Some(index) => index.saturating_sub(1),
        };
        self.cursor = Some(index);
        Some(self.entries[index].clone())
    }

    /// Moving past the newest entry restores the draft exactly once.
    #[expect(
        clippy::should_implement_trait,
        reason = "Bidirectional history cursor navigation restores the draft, rather than consuming an Iterator sequence"
    )]
    pub fn next(&mut self) -> Option<String> {
        let index = self.cursor? + 1;
        if index < self.entries.len() {
            self.cursor = Some(index);
            Some(self.entries[index].clone())
        } else {
            self.cursor = None;
            Some(std::mem::take(&mut self.draft))
        }
    }

    pub fn reset_navigation(&mut self) {
        self.cursor = None;
        self.draft.clear();
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Optional persistence through the workspace's existing filesystem authority.
#[derive(Debug, Clone)]
pub struct HistoryStore {
    workspace: crate::workspace::Workspace,
    path: crate::path::ScopePath,
}

impl HistoryStore {
    pub const MAX_STORAGE_BYTES: usize = 1024 * 1024;

    pub fn new(workspace: crate::workspace::Workspace, path: crate::path::ScopePath) -> Self {
        Self { workspace, path }
    }

    /// Replaces the model on a valid load; missing, denied, malformed or oversized
    /// files leave it unchanged. The model's own capacity and record rules apply.
    pub fn load(&self, history: &mut CommandHistory) {
        let Ok(file) = self.workspace.load_file(&self.path) else {
            return;
        };
        if file.contents.len() > Self::MAX_STORAGE_BYTES {
            return;
        }
        let Ok(entries) = serde_json::from_str::<Vec<String>>(&file.contents) else {
            return;
        };
        let mut loaded = CommandHistory::new(history.max);
        for command in entries {
            loaded.record(&command);
        }
        *history = loaded;
    }

    /// Saves the newest entries that fit, without modifying the in-memory model.
    /// Workspace errors (including read-only denial) are returned to the caller.
    pub fn save(&self, history: &CommandHistory) -> Result<(), crate::workspace::WorkspaceError> {
        let mut selected = Vec::new();
        let mut bytes = 2; // JSON array brackets.
        for command in history.entries.iter().rev() {
            // JSON encoding cannot be shorter than the source UTF-8 bytes.
            if command.len() > Self::MAX_STORAGE_BYTES - 2 {
                continue;
            }
            let encoded = serde_json::to_string(command).expect("strings serialize to JSON");
            if encoded.len() > Self::MAX_STORAGE_BYTES - 2 {
                continue;
            }
            let additional = encoded.len() + usize::from(!selected.is_empty());
            if bytes + additional > Self::MAX_STORAGE_BYTES {
                break;
            }
            bytes += additional;
            selected.push(encoded);
        }
        selected.reverse();
        self.workspace
            .save_file(&self.path, format!("[{}]", selected.join(",")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_and_record_rules() {
        let mut history = CommandHistory::new(2);
        assert!(!history.record(" \n\t"));
        assert!(history.record(" a "));
        assert!(!history.record(" a "));
        assert!(history.record("b"));
        assert!(history.record(" a "));
        assert_eq!(history.entries(), &["b", " a "]);
        assert!(!CommandHistory::new(0).record("a"));
        let mut default = CommandHistory::default();
        for i in 0..1001 {
            default.record(&i.to_string());
        }
        assert_eq!(default.entries().len(), 1000);
        assert_eq!(default.entries()[0], "1");
    }

    #[test]
    fn navigation_preserves_draft_and_does_not_mutate_entries() {
        let mut history = CommandHistory::new(3);
        assert_eq!(history.previous("draft"), None);
        assert_eq!(history.next(), None);
        history.record("one");
        history.record("two");
        assert_eq!(history.next(), None);
        assert_eq!(history.previous("draft λ"), Some("two".into()));
        assert_eq!(history.previous("edited two"), Some("one".into()));
        assert_eq!(history.previous("edited one"), Some("one".into()));
        assert_eq!(history.next(), Some("two".into()));
        assert_eq!(history.next(), Some("draft λ".into()));
        assert_eq!(history.next(), None);
        assert_eq!(history.previous("new draft"), Some("two".into()));
        assert_eq!(history.next(), Some("new draft".into()));
        assert_eq!(history.entries(), &["one", "two"]);
    }

    #[test]
    fn reset_and_rejected_record_end_navigation() {
        let mut history = CommandHistory::new(2);
        history.record("one");
        history.previous("draft");
        history.reset_navigation();
        assert_eq!(history.next(), None);
        history.previous("replacement");
        assert!(!history.record("one"));
        assert_eq!(history.next(), None);
        history.previous("replacement");
        assert!(!history.record(" "));
        assert_eq!(history.next(), None);
        history.previous("replacement");
        history.record("two");
        assert_eq!(history.previous("fresh"), Some("two".into()));
        assert_eq!(history.next(), Some("fresh".into()));
    }

    mod persistence {
        use super::*;
        use crate::{path::ScopePath, test_support::TempWorkspace, workspace::Workspace};

        fn store(workspace: Workspace) -> HistoryStore {
            HistoryStore::new(workspace.clone(), ScopePath::new("history.json").unwrap())
        }

        #[test]
        fn round_trip_and_load_capacity() {
            let root = TempWorkspace::new("history_round_trip");
            let store = store(root.workspace());
            let mut history = CommandHistory::default();
            for command in ["one", "λ\n\"two\"", "three"] {
                history.record(command);
            }
            store.save(&history).unwrap();
            let mut loaded = CommandHistory::new(2);
            loaded.record("old");
            loaded.previous("draft");
            store.load(&mut loaded);
            assert_eq!(loaded.entries(), &history.entries()[1..]);
            assert_eq!(loaded.next(), None);
        }

        #[test]
        fn unusable_loads_leave_model_intact() {
            let root = TempWorkspace::new("history_bad_load");
            let store = store(root.workspace());
            let mut history = CommandHistory::default();
            history.record("keep");
            store.load(&mut history);
            for contents in ["", "{", "{}", "[1]", "[\"valid\", null]"] {
                root.write_file("history.json", contents);
                store.load(&mut history);
                assert_eq!(history.entries(), &["keep"]);
            }
            root.write_file(
                "history.json",
                " ".repeat(HistoryStore::MAX_STORAGE_BYTES + 1),
            );
            store.load(&mut history);
            assert_eq!(history.entries(), &["keep"]);
            root.write_bytes("history.json", &[0xff]);
            store.load(&mut history);
            assert_eq!(history.entries(), &["keep"]);
        }

        #[test]
        fn bounded_storage_skips_oversized_commands_without_mutation() {
            let root = TempWorkspace::new("history_bounds");
            let store = store(root.workspace());
            let mut history = CommandHistory::default();
            history.record("old");
            history.record(&"x".repeat(HistoryStore::MAX_STORAGE_BYTES));
            history.record(&"\"".repeat(HistoryStore::MAX_STORAGE_BYTES / 2));
            history.record("new");
            store.save(&history).unwrap();
            let mut loaded = CommandHistory::default();
            store.load(&mut loaded);
            assert_eq!(loaded.entries(), &["old", "new"]);
            assert_eq!(history.entries().len(), 4);
            history.record(&"a".repeat(600_000));
            history.record(&"b".repeat(600_000));
            store.save(&history).unwrap();
            assert!(root.read_file("history.json").len() <= HistoryStore::MAX_STORAGE_BYTES);
            store.load(&mut loaded);
            assert_eq!(loaded.entries(), &history.entries()[5..]);
        }

        #[test]
        fn read_only_denies_save_but_allows_load() {
            let root = TempWorkspace::new("history_read_only");
            root.write_file("history.json", "[\"original\"]");
            let store = store(Workspace::read_only(root.root().to_string_lossy()).unwrap());
            let mut history = CommandHistory::default();
            store.load(&mut history);
            history.record("new");
            assert!(matches!(
                store.save(&history),
                Err(crate::workspace::WorkspaceError::WriteFile { .. })
            ));
            assert_eq!(root.read_file("history.json"), "[\"original\"]");
            assert_eq!(history.entries(), &["original", "new"]);
        }

        #[cfg(unix)]
        #[test]
        fn denied_escape_load_and_save() {
            let root = TempWorkspace::new("history_denied");
            let outside = TempWorkspace::new("history_outside");
            outside.write_file("history.json", "[\"outside\"]");
            root.symlink(outside.root().join("history.json"), "history.json");
            let store = store(root.workspace());
            let mut history = CommandHistory::default();
            history.record("keep");
            store.load(&mut history);
            assert_eq!(history.entries(), &["keep"]);
            assert!(store.save(&history).is_err());
            assert_eq!(outside.read_file("history.json"), "[\"outside\"]");
        }
    }
}
