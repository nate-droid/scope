use crate::path::{ScopePath, ToScopePath};

pub mod meta;
pub mod proposal;
pub use meta::{SnapshotOptions, WorkspaceMeta, WorkspaceMetaError};
pub use proposal::{Acceptance, Proposal, ProposalChange, ProposalError, ProposalLimits};

pub use cell_capability_fs::{FsWatch, RescanReason, WatchEvent, WatchOptions};

const DEFAULT_MOUNT_NAME: &str = "project";

#[derive(Clone)]
pub struct Workspace {
    root: String,
    writable: bool,
    fs: cell_capability_fs::Workspace,
    cell: std::sync::Arc<cell::Cell>,
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Workspace")
            .field("root", &self.root)
            .field("fs", &self.fs)
            .field("capabilities", &self.capabilities())
            .finish()
    }
}

impl Workspace {
    pub fn new(root: impl Into<String>) -> Result<Self, WorkspaceError> {
        Self::with_write_policy(root, true)
    }

    pub fn read_only(root: impl Into<String>) -> Result<Self, WorkspaceError> {
        Self::with_write_policy(root, false)
    }

    fn with_write_policy(root: impl Into<String>, writable: bool) -> Result<Self, WorkspaceError> {
        let root = root.into();
        let fs = if writable {
            cell_capability_fs::Workspace::new().mount(DEFAULT_MOUNT_NAME, root.clone())
        } else {
            cell_capability_fs::Workspace::new().mount_read_only(DEFAULT_MOUNT_NAME, root.clone())
        }
        .map_err(|source| WorkspaceError::CreateWorkspace {
            root: root.clone(),
            message: source.to_string(),
        })?;

        let cell = cell::Cell::new()
            .and_then(|cell| cell.with(fs.clone()))
            .map_err(|source| WorkspaceError::CreateWorkspace {
                root: root.clone(),
                message: source.to_string(),
            })?;

        Ok(Self {
            root,
            writable,
            fs,
            cell: std::sync::Arc::new(cell),
        })
    }

    /// Installed Cell interface names, not mount policies or semantic tools.
    pub fn capabilities(&self) -> &[&'static str] {
        self.cell.capabilities()
    }

    /// Advisory Cell authority metadata, not an authorization check.
    /// Descriptors expose logical mounts and policies, never physical host paths.
    pub fn capability_descriptors(&self) -> &[cell::CapabilityDescriptor] {
        self.cell.capability_descriptors()
    }

    /// Observe the entire filesystem grant using Cell's native snapshot watch.
    /// Event paths are mount-prefixed (for example, `project/src/main.rs`),
    /// not the relative `ScopePath` values returned by `list_files`.
    ///
    /// Construction and every `FsWatch::poll` synchronously scan the entire
    /// grant, including `target` and `.git` despite `list_files` filtering them.
    /// Symlinks are observed without following them. There is no background
    /// polling; callers choose the polling cadence. The initial snapshot is
    /// quiet, and changes between polls can be missed. Renames appear as
    /// deletion and creation. Read-only workspaces can observe external writes.
    ///
    /// `RescanRequired` means incremental events are incomplete: callers must
    /// refresh their own view. Event-budget overflow retains the new baseline;
    /// scan failure or entry-budget overflow discards it, and the next successful
    /// poll establishes a baseline while reporting `ScanFailed` again. Initial
    /// scan failures and invalid budgets return Cell's structured `FsError`.
    /// Cancelling or dropping the watch releases its retained grant authority.
    pub fn watch(&self, options: WatchOptions) -> Result<FsWatch, cell_capability_fs::FsError> {
        self.fs.watch(options)
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn load_file(&self, path: impl ToScopePath) -> Result<WorkspaceFile, WorkspaceError> {
        let path = path
            .to_scope_path()
            .map_err(|source| WorkspaceError::InvalidPath {
                message: source.to_string(),
            })?;
        let workspace_path = self.cell_path(&path);
        let contents =
            self.fs
                .read_text(&workspace_path)
                .map_err(|message| WorkspaceError::ReadFile {
                    path: path.clone(),
                    message: message.to_string(),
                })?;

        Ok(WorkspaceFile { path, contents })
    }

    pub fn load_file_revision(
        &self,
        path: impl ToScopePath,
    ) -> Result<RevisionedWorkspaceFile, WorkspaceError> {
        let path = path
            .to_scope_path()
            .map_err(|source| WorkspaceError::InvalidPath {
                message: source.to_string(),
            })?;
        self.read_document_source(&path)
            .map_err(|source| WorkspaceError::ReadFile {
                path,
                message: source.to_string(),
            })
    }

    /// Read a confined text file while preserving Cell's structured filesystem errors.
    pub fn read_document_source(
        &self,
        path: &ScopePath,
    ) -> Result<RevisionedWorkspaceFile, cell_capability_fs::FsError> {
        let file = self.fs.read_text_revision(self.cell_path(path))?;
        Ok(RevisionedWorkspaceFile {
            path: path.clone(),
            contents: file.contents,
            revision: file.revision,
        })
    }

    /// Exclusively create through Cell's writable mount and return its revision.
    /// Unix only; no durability guarantee. The parent must not have hostile writers.
    pub fn create_document_source(
        &self,
        path: &ScopePath,
        contents: &str,
    ) -> Result<String, cell_capability_fs::FsError> {
        self.fs.create_text(self.cell_path(path), contents)
    }

    /// Revision-checked atomic replacement, preserving structured Cell failures.
    /// Conflict checking is best-effort, not atomic compare-and-swap.
    pub fn save_document_source(
        &self,
        path: &ScopePath,
        contents: &str,
        expected_revision: &str,
    ) -> Result<cell_capability_fs::SaveOutcome, cell_capability_fs::FsError> {
        self.fs
            .save_text(self.cell_path(path), contents, Some(expected_revision))
    }

    /// Check the disk revision before saving, returning the new revision.
    /// This is best-effort conflict detection, not atomic compare-and-swap:
    /// an external writer can change the file between the check and replacement.
    pub fn save_file_checked(
        &self,
        path: impl ToScopePath,
        contents: impl AsRef<str>,
        expected_revision: &str,
    ) -> Result<String, WorkspaceError> {
        let path = path
            .to_scope_path()
            .map_err(|source| WorkspaceError::InvalidPath {
                message: source.to_string(),
            })?;
        let outcome = self
            .fs
            .save_text(self.cell_path(&path), contents, Some(expected_revision))
            .map_err(|source| WorkspaceError::WriteFile {
                path: path.clone(),
                message: source.to_string(),
            })?;
        match outcome {
            cell_capability_fs::SaveOutcome::Saved(revision) => Ok(revision),
            cell_capability_fs::SaveOutcome::Conflict(current_revision) => {
                Err(WorkspaceError::Conflict {
                    path,
                    current_revision,
                })
            }
        }
    }

    pub fn save_file(
        &self,
        path: impl ToScopePath,
        contents: impl AsRef<str>,
    ) -> Result<(), WorkspaceError> {
        let path = path
            .to_scope_path()
            .map_err(|source| WorkspaceError::InvalidPath {
                message: source.to_string(),
            })?;
        let workspace_path = self.cell_path(&path);
        self.fs
            .write_text(&workspace_path, contents.as_ref())
            .map_err(|message| WorkspaceError::WriteFile {
                path,
                message: message.to_string(),
            })
    }

    /// Recursively list regular files without following symlinks or special entries.
    /// Directory lookup failures are propagated rather than classified as files.
    pub fn list_files(&self) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
        let mut entries = Vec::new();
        self.collect_files(ScopePath::root(), &mut entries)?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }

    fn collect_files(
        &self,
        directory: ScopePath,
        entries: &mut Vec<WorkspaceEntry>,
    ) -> Result<(), WorkspaceError> {
        let workspace_path = self.cell_path(&directory);
        let directory_entries =
            self.fs
                .list_dir(&workspace_path)
                .map_err(|message| WorkspaceError::ReadDirectory {
                    path: directory.clone(),
                    message: message.to_string(),
                })?;

        for entry in directory_entries {
            if should_skip(&entry.name) {
                continue;
            }

            let path =
                directory
                    .join(entry.name)
                    .map_err(|source| WorkspaceError::InvalidPath {
                        message: source.to_string(),
                    })?;
            match entry.file_type {
                cell_capability_fs::FileType::Directory => self.collect_files(path, entries)?,
                cell_capability_fs::FileType::File => entries.push(WorkspaceEntry {
                    path,
                    kind: WorkspaceEntryKind::File,
                }),
                cell_capability_fs::FileType::Symlink | cell_capability_fs::FileType::Other => {}
            }
        }

        Ok(())
    }

    pub(crate) fn meta_list_dir(
        &self,
        directory: &ScopePath,
    ) -> Result<Vec<cell_capability_fs::DirectoryEntry>, WorkspaceError> {
        self.fs
            .list_dir(self.cell_path(directory))
            .map_err(|error| WorkspaceError::ReadDirectory {
                path: directory.clone(),
                message: error.to_string(),
            })
    }

    pub(crate) fn meta_is_directory(&self, path: &ScopePath) -> Result<bool, WorkspaceError> {
        self.fs
            .symlink_metadata(self.cell_path(path))
            .map(|metadata| matches!(metadata.file_type, cell_capability_fs::FileType::Directory))
            .map_err(|error| WorkspaceError::ReadDirectory {
                path: path.clone(),
                message: error.to_string(),
            })
    }

    fn cell_path(&self, path: &ScopePath) -> String {
        if path.as_str() == "." {
            DEFAULT_MOUNT_NAME.to_string()
        } else {
            format!("{DEFAULT_MOUNT_NAME}/{}", path.as_str())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub path: ScopePath,
    pub kind: WorkspaceEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceEntryKind {
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub path: ScopePath,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionedWorkspaceFile {
    pub path: ScopePath,
    pub contents: String,
    pub revision: String,
}

#[derive(Debug)]
pub enum WorkspaceError {
    CreateWorkspace {
        root: String,
        message: String,
    },
    InvalidPath {
        message: String,
    },
    ReadDirectory {
        path: ScopePath,
        message: String,
    },
    ReadFile {
        path: ScopePath,
        message: String,
    },
    WriteFile {
        path: ScopePath,
        message: String,
    },
    Conflict {
        path: ScopePath,
        current_revision: String,
    },
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateWorkspace { root, message } => {
                write!(
                    formatter,
                    "failed to create workspace for {root}: {message}"
                )
            }
            Self::InvalidPath { message } => write!(formatter, "invalid workspace path: {message}"),
            Self::ReadDirectory { path, message } => {
                write!(
                    formatter,
                    "failed to read directory {}: {message}",
                    path.display()
                )
            }
            Self::ReadFile { path, message } => {
                write!(
                    formatter,
                    "failed to read file {}: {message}",
                    path.display()
                )
            }
            Self::Conflict {
                path,
                current_revision,
            } => write!(
                formatter,
                "file {} changed on disk (current revision: {current_revision})",
                path.display()
            ),
            Self::WriteFile { path, message } => {
                write!(
                    formatter,
                    "failed to write file {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for WorkspaceError {}

fn should_skip(name: &str) -> bool {
    matches!(name, ".git" | "target")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempWorkspace;
    #[cfg(unix)]
    use crate::test_support::symlink_workspace;

    #[test]
    fn lists_installed_capabilities_for_both_write_policies() {
        let root = TempWorkspace::new("lists_installed_capabilities");
        let writable = root.workspace();
        let read_only = Workspace::read_only(root.root().to_string_lossy()).unwrap();
        assert_eq!(writable.capabilities(), &["cell:fs/fs@0.1.0"]);
        assert_eq!(read_only.capabilities(), writable.capabilities());
        assert_eq!(writable.clone().capabilities(), writable.capabilities());
    }

    #[test]
    fn descriptors_distinguish_read_only_and_write_authority() {
        use cell::{AuthorityDescription, FilesystemOperation, FilesystemPolicy};
        let root = TempWorkspace::new("workspace_descriptors");
        let writable = root.workspace();
        let read_only = Workspace::read_only(root.root().to_string_lossy()).unwrap();
        for (workspace, policy) in [
            (&writable, FilesystemPolicy::WithinRoot),
            (&read_only, FilesystemPolicy::Denied),
        ] {
            let descriptors: &[cell::CapabilityDescriptor] = workspace.capability_descriptors();
            assert_eq!(descriptors.len(), 1);
            assert_eq!(descriptors[0].interface, workspace.capabilities()[0]);
            let AuthorityDescription::Filesystem { roots } = &descriptors[0].authority else {
                panic!("expected filesystem authority");
            };
            assert_eq!(roots.len(), 1);
            assert_eq!(roots[0].name, "project");
            assert_eq!(roots[0].read_policy, FilesystemPolicy::WithinRoot);
            assert_eq!(roots[0].write_policy, policy);
            assert!(roots[0].operations.contains(&FilesystemOperation::ReadText));
            for operation in [
                FilesystemOperation::WriteText,
                FilesystemOperation::SaveText,
            ] {
                assert_eq!(
                    roots[0].operations.contains(&operation),
                    policy == FilesystemPolicy::WithinRoot
                );
            }
            assert!(!format!("{descriptors:?}").contains(root.root().to_str().unwrap()));
            assert_eq!(workspace.clone().capability_descriptors(), descriptors);
        }
    }

    #[test]
    fn watch_is_initially_quiet_and_reports_created_and_deleted_paths() {
        let root = TempWorkspace::new("workspace_watch_changes");
        root.write_file("existing.txt", "existing");
        let workspace = root.workspace();
        let mut watch = workspace.watch(WatchOptions::default()).unwrap();
        assert!(watch.poll().is_empty());
        root.write_file("new.txt", "new");
        assert_eq!(
            watch.poll(),
            vec![WatchEvent::Created("project/new.txt".into())]
        );
        std::fs::remove_file(root.root().join("new.txt")).unwrap();
        assert_eq!(
            watch.poll(),
            vec![WatchEvent::Deleted("project/new.txt".into())]
        );
        assert!(watch.poll().is_empty());
    }

    #[test]
    fn read_only_watch_observes_external_writes_in_filtered_directories() {
        let root = TempWorkspace::new("workspace_read_only_watch");
        root.create_dir("target");
        root.create_dir(".git");
        let workspace = Workspace::read_only(root.root().to_string_lossy()).unwrap();
        let mut watch = workspace.watch(WatchOptions::default()).unwrap();
        assert!(watch.poll().is_empty());
        assert!(workspace.save_file("denied.txt", "no").is_err());
        root.write_file("target/output", "build");
        root.write_file(".git/external", "metadata");
        let events = watch.poll();
        assert!(events.contains(&WatchEvent::Created("project/target/output".into())));
        assert!(events.contains(&WatchEvent::Created("project/.git/external".into())));
        assert!(workspace.list_files().unwrap().is_empty());
        watch.cancel();
        assert!(watch.is_cancelled());
        root.write_file("after-cancel.txt", "ignored");
        assert!(watch.poll().is_empty());
        watch.cancel();
        assert!(watch.poll().is_empty());
    }

    #[test]
    fn watch_preserves_structured_initial_errors() {
        use cell_capability_fs::{FsError, FsErrorKind};
        let root = TempWorkspace::new("workspace_watch_errors");
        let workspace = root.workspace();
        for options in [
            WatchOptions {
                max_entries: 0,
                max_events: 1,
            },
            WatchOptions {
                max_entries: 1,
                max_events: 0,
            },
        ] {
            let error: FsError = workspace.watch(options).err().unwrap();
            assert_eq!(error.kind, FsErrorKind::InvalidPath);
            assert_eq!(error.operation, "watch");
            assert_eq!(error.path, ".");
        }
        root.write_file("one", "1");
        root.write_file("two", "2");
        let error = workspace
            .watch(WatchOptions {
                max_entries: 1,
                max_events: 1,
            })
            .err()
            .unwrap();
        assert_eq!(error.kind, FsErrorKind::Other("watch-entry-limit".into()));
        assert_eq!(error.operation, "watch");
    }

    #[test]
    fn watch_budget_rescans_follow_native_baseline_semantics() {
        let root = TempWorkspace::new("workspace_watch_budgets");
        let workspace = root.workspace();
        let mut entry_watch = workspace
            .watch(WatchOptions {
                max_entries: 1,
                max_events: 10,
            })
            .unwrap();
        let mut event_watch = workspace
            .watch(WatchOptions {
                max_entries: 10,
                max_events: 1,
            })
            .unwrap();
        root.write_file("one", "1");
        root.write_file("two", "2");
        assert_eq!(
            entry_watch.poll(),
            vec![WatchEvent::RescanRequired(RescanReason::EntryLimit)]
        );
        assert_eq!(
            event_watch.poll(),
            vec![WatchEvent::RescanRequired(RescanReason::EventLimit)]
        );
        assert!(event_watch.poll().is_empty());
        std::fs::remove_file(root.root().join("two")).unwrap();
        assert_eq!(
            entry_watch.poll(),
            vec![WatchEvent::RescanRequired(RescanReason::ScanFailed)]
        );
        assert!(entry_watch.poll().is_empty());
        assert_eq!(
            event_watch.poll(),
            vec![WatchEvent::Deleted("project/two".into())]
        );
    }

    #[test]
    fn lists_workspace_files() {
        let root = TempWorkspace::new("lists_workspace_files");
        root.write_file("Cargo.toml", "[package]");
        root.write_file("src/main.rs", "fn main() {}");
        root.write_file("target/ignored", "ignored");

        let workspace = root.workspace();
        let files = workspace.list_files().unwrap();
        let paths = files
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            vec![
                ScopePath::new("Cargo.toml").unwrap(),
                ScopePath::new("src/main.rs").unwrap()
            ]
        );
    }

    #[test]
    #[cfg(unix)]
    fn lists_regular_files_without_following_symlinks() {
        let root = TempWorkspace::new("lists_regular_files_without_following_symlinks");
        let outside = TempWorkspace::new("lists_regular_files-outside");
        root.write_file("src/main.rs", "fn main() {}");
        outside.write_file("secret.txt", "outside");
        root.symlink("src", "directory-link");
        root.symlink("src/main.rs", "file-link");
        root.symlink(".", "cycle");
        root.symlink("missing", "dangling");
        root.symlink("cyclic-b", "cyclic-a");
        root.symlink("cyclic-a", "cyclic-b");
        root.symlink(outside.root(), "outside-link");

        let workspace = root.workspace();
        let read_only = Workspace::read_only(root.root().to_string_lossy()).unwrap();
        let expected = vec![WorkspaceEntry {
            path: ScopePath::new("src/main.rs").unwrap(),
            kind: WorkspaceEntryKind::File,
        }];
        assert_eq!(workspace.list_files().unwrap(), expected);
        assert_eq!(read_only.list_files().unwrap(), expected);

        // Non-following discovery does not prohibit an explicit confined read.
        assert_eq!(
            workspace.load_file("file-link").unwrap().contents,
            "fn main() {}"
        );
        assert!(matches!(
            workspace.load_file("outside-link/secret.txt"),
            Err(WorkspaceError::ReadFile { .. })
        ));
    }

    #[test]
    fn directory_lookup_failures_are_propagated() {
        let root = TempWorkspace::new("directory_lookup_failures_are_propagated");
        root.write_file("not-a-directory", "text");
        let workspace = root.workspace();
        let mut entries = Vec::new();
        for path in ["missing", "not-a-directory"] {
            assert!(matches!(
                workspace.collect_files(ScopePath::new(path).unwrap(), &mut entries),
                Err(WorkspaceError::ReadDirectory { .. })
            ));
        }
        assert!(entries.is_empty());
    }

    #[test]
    fn loads_workspace_file() {
        let root = TempWorkspace::new("loads_workspace_file");
        root.write_file("README.md", "hello");

        let workspace = root.workspace();
        let file = workspace.load_file("README.md").unwrap();

        assert_eq!(file.path, ScopePath::new("README.md").unwrap());
        assert_eq!(file.contents, "hello");
    }

    #[test]
    #[cfg(unix)]
    fn loads_workspace_file_through_symlinked_root() {
        let real_root = TempWorkspace::new("loads_workspace_file_through_symlinked_root-real");
        real_root.write_file("README.md", "hello");
        let linked_root = symlink_workspace(
            "loads_workspace_file_through_symlinked_root-link",
            real_root.root(),
        );

        let workspace = linked_root.workspace();
        let file = workspace.load_file("README.md").unwrap();

        assert_eq!(file.path, ScopePath::new("README.md").unwrap());
        assert_eq!(file.contents, "hello");
    }

    #[test]
    fn saves_workspace_file() {
        let root = TempWorkspace::new("saves_workspace_file");
        root.write_file("src/main.rs", "fn main() {}");

        let workspace = root.workspace();
        workspace
            .save_file("src/main.rs", "fn main() { println!(\"hi\"); }")
            .unwrap();

        assert_eq!(
            root.read_file("src/main.rs"),
            "fn main() { println!(\"hi\"); }"
        );
    }

    #[test]
    fn refuses_to_save_outside_workspace() {
        let root = TempWorkspace::new("refuses_to_save_outside_workspace");

        let workspace = root.workspace();
        let error = workspace.save_file("../outside.txt", "nope").unwrap_err();

        assert!(matches!(error, WorkspaceError::InvalidPath { .. }));
        assert!(!root.host_path_exists("../outside.txt"));
    }

    #[test]
    fn read_only_workspace_denies_writes() {
        let root = TempWorkspace::new("read_only_workspace_denies_writes");
        root.write_file("README.md", "hello");

        let workspace = Workspace::read_only(root.root().to_string_lossy()).unwrap();
        let error = workspace.save_file("README.md", "changed").unwrap_err();

        assert!(matches!(error, WorkspaceError::WriteFile { .. }));
        assert_eq!(root.read_file("README.md"), "hello");
    }
}
