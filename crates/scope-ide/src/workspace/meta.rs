//! Capability-backed, immutable workspace tree snapshots. Files are discovered,
//! never opened; their presence does not guarantee readability. Traversal is not
//! atomic: directory types are rechecked without following the final symlink,
//! but Cell remains the confinement authority during races. `list_dir` allocates
//! a whole directory before its entries can be charged against the budget.

use super::{Workspace, WorkspaceError};
use crate::path::ScopePath;
use cell_capability_fs::{FileType, FsWatch, WatchOptions};
use scope_meta::{
    GraphId, MetaEdge, MetaGraph, MetaNode, NodeId, Provenance, RevisionId, SourceRef, TypeId,
    TypeRegistry,
};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_GRAPH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotOptions {
    /// Includes the root and every observed entry, even excluded names and links.
    pub max_entries: usize,
    /// Root depth is zero. Directories at this depth are recorded, not scanned.
    pub max_depth: usize,
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_depth: 64,
        }
    }
}

#[derive(Debug)]
pub enum WorkspaceMetaError {
    InvalidOptions,
    EntryLimit {
        max_entries: usize,
        directory: ScopePath,
    },
    NotDirectory {
        path: ScopePath,
    },
    StaleSnapshot,
    WatchNotEnabled,
    Workspace(WorkspaceError),
    Watch(cell_capability_fs::FsError),
    Graph(scope_meta::Error),
}

impl std::fmt::Display for WorkspaceMetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOptions => write!(f, "snapshot max_entries must be nonzero"),
            Self::EntryLimit {
                max_entries,
                directory,
            } => write!(
                f,
                "snapshot entry limit {max_entries} exceeded in {directory}"
            ),
            Self::NotDirectory { path } => {
                write!(f, "snapshot directory is no longer a directory: {path}")
            }
            Self::StaleSnapshot => write!(f, "workspace snapshot is stale; refresh required"),
            Self::WatchNotEnabled => write!(f, "workspace snapshot watch is not enabled"),
            Self::Workspace(error) => error.fmt(f),
            Self::Watch(error) => error.fmt(f),
            Self::Graph(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for WorkspaceMetaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Workspace(error) => Some(error),
            Self::Watch(error) => Some(error),
            Self::Graph(error) => Some(error),
            _ => None,
        }
    }
}
impl From<WorkspaceError> for WorkspaceMetaError {
    fn from(error: WorkspaceError) -> Self {
        Self::Workspace(error)
    }
}
impl From<scope_meta::Error> for WorkspaceMetaError {
    fn from(error: scope_meta::Error) -> Self {
        Self::Graph(error)
    }
}

/// One tracker has a stable graph identity and never reuses snapshot revisions.
/// Construction does not enable a watcher or acquire additional watch authority.
pub struct WorkspaceMeta {
    workspace: Workspace,
    options: SnapshotOptions,
    graph_id: GraphId,
    revision: u64,
    graph: Option<MetaGraph>,
    truncated_paths: Vec<ScopePath>,
    watcher: Option<FsWatch>,
}

impl WorkspaceMeta {
    pub fn new(workspace: Workspace, options: SnapshotOptions) -> Result<Self, WorkspaceMetaError> {
        if options.max_entries == 0 {
            return Err(WorkspaceMetaError::InvalidOptions);
        }
        let identity = NEXT_GRAPH
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("workspace graph identity exhausted");
        let mut tracker = Self {
            workspace,
            options,
            graph_id: GraphId::new(format!("scope.workspace:{}:{identity}", std::process::id())),
            revision: 0,
            graph: None,
            truncated_paths: Vec::new(),
            watcher: None,
        };
        tracker.refresh()?;
        Ok(tracker)
    }

    pub fn graph(&self) -> Result<&MetaGraph, WorkspaceMetaError> {
        self.graph.as_ref().ok_or(WorkspaceMetaError::StaleSnapshot)
    }

    /// Directories deliberately not scanned because of the depth bound.
    pub fn truncated_paths(&self) -> Result<&[ScopePath], WorkspaceMetaError> {
        self.graph()?;
        Ok(&self.truncated_paths)
    }

    pub fn invalidate(&mut self) {
        self.graph = None;
        self.truncated_paths.clear();
        self.revision = self
            .revision
            .checked_add(1)
            .expect("workspace snapshot revision exhausted");
    }

    /// Invalidates first; failure never exposes the previous graph or truncation.
    pub fn refresh(&mut self) -> Result<(), WorkspaceMetaError> {
        self.invalidate();
        let (graph, truncated) = self.build()?;
        self.revision = self
            .revision
            .checked_add(1)
            .expect("workspace snapshot revision exhausted");
        self.graph = Some(graph);
        self.truncated_paths = truncated;
        Ok(())
    }

    /// Explicitly watches the whole Cell filesystem grant, including excluded
    /// `.git` and `target` trees. Construction/polling scan synchronously, with
    /// separate Cell budgets. Refresh after subscription closes the initial gap;
    /// concurrent changes still require polling. No background watcher is used.
    pub fn enable_watch(&mut self, options: WatchOptions) -> Result<(), WorkspaceMetaError> {
        self.invalidate();
        self.watcher = None;
        let watcher = self
            .workspace
            .watch(options)
            .map_err(WorkspaceMetaError::Watch)?;
        self.watcher = Some(watcher);
        self.refresh()
    }

    /// Conservatively invalidates on all events, including rescan/scan failures.
    /// Cell reports polling failures as `RescanRequired`, not a `Result` error.
    pub fn poll_changes(&mut self) -> Result<bool, WorkspaceMetaError> {
        let watcher = self
            .watcher
            .as_mut()
            .ok_or(WorkspaceMetaError::WatchNotEnabled)?;
        let changed = !watcher.poll().is_empty();
        if changed {
            self.invalidate();
        }
        Ok(changed)
    }

    pub fn node_for_path(&self, path: &ScopePath) -> Result<&MetaNode, WorkspaceMetaError> {
        Ok(self.graph()?.get(&NodeId::new(path.as_str()))?)
    }

    pub fn children(&self, id: &NodeId) -> Result<Vec<&MetaNode>, WorkspaceMetaError> {
        let graph = self.graph()?;
        graph
            .related_by(id, &kind("contains"))?
            .into_iter()
            .map(|edge| graph.get(&edge.to).map_err(Into::into))
            .collect()
    }

    pub fn parent(&self, id: &NodeId) -> Result<Option<&MetaNode>, WorkspaceMetaError> {
        let graph = self.graph()?;
        let description = graph.describe(id)?;
        description
            .incoming
            .into_iter()
            .find(|edge| edge.relation == kind("contains"))
            .map(|edge| graph.get(&edge.from).map_err(Into::into))
            .transpose()
    }

    fn build(&self) -> Result<(MetaGraph, Vec<ScopePath>), WorkspaceMetaError> {
        let mut registry = TypeRegistry::new();
        for name in ["workspace", "directory", "file"] {
            registry.register_node(kind(name))?;
        }
        registry.register_relation(kind("contains"), kind("workspace"), kind("directory"))?;
        for (from, to) in [
            ("workspace", "file"),
            ("directory", "directory"),
            ("directory", "file"),
        ] {
            registry.register_relation_pair(kind("contains"), kind(from), kind(to))?;
        }
        let root = ScopePath::root();
        let mut nodes = vec![node(&root, "workspace", "Workspace")];
        let mut edges = Vec::new();
        let mut truncated = Vec::new();
        let mut stack = vec![(root, 0usize)];
        let mut observed = 1usize;
        while let Some((directory, depth)) = stack.pop() {
            if !self.workspace.meta_is_directory(&directory)? {
                return Err(WorkspaceMetaError::NotDirectory { path: directory });
            }
            if depth == self.options.max_depth {
                truncated.push(directory);
                continue;
            }
            let mut entries = self.workspace.meta_list_dir(&directory)?;
            if entries.len() > self.options.max_entries - observed {
                return Err(WorkspaceMetaError::EntryLimit {
                    max_entries: self.options.max_entries,
                    directory,
                });
            }
            observed += entries.len();
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            let mut directories = Vec::new();
            for entry in entries {
                if matches!(entry.name.as_str(), ".git" | "target") {
                    continue;
                }
                let node_kind = match entry.file_type {
                    FileType::Directory => "directory",
                    FileType::File => "file",
                    FileType::Symlink | FileType::Other => continue,
                };
                let path = directory.join(entry.name.as_str()).map_err(|error| {
                    WorkspaceError::InvalidPath {
                        message: error.to_string(),
                    }
                })?;
                nodes.push(node(&path, node_kind, &entry.name));
                edges.push(MetaEdge {
                    from: NodeId::new(directory.as_str()),
                    relation: kind("contains"),
                    to: NodeId::new(path.as_str()),
                    provenance: Provenance::Observed {
                        provider: "scope.workspace@1".into(),
                    },
                });
                if node_kind == "directory" {
                    directories.push((path, depth + 1));
                }
            }
            stack.extend(directories.into_iter().rev());
        }
        truncated.sort();
        let revision = self
            .revision
            .checked_add(1)
            .expect("workspace snapshot revision exhausted");
        Ok((
            MetaGraph::new(
                self.graph_id.clone(),
                RevisionId::new(revision.to_string()),
                registry,
                nodes,
                edges,
            )?,
            truncated,
        ))
    }
}

impl scope_meta::MetaProvider for WorkspaceMeta {
    fn description(&self) -> scope_meta::ProviderDescription {
        scope_meta::ProviderDescription::new(
            "scope.workspace@1",
            "Capability-backed workspace tree",
        )
    }

    fn snapshot(&self) -> Result<scope_meta::MetaSnapshot<'_>, scope_meta::ProviderError> {
        let graph = self.graph().map_err(|error| {
            scope_meta::ProviderError::with_source(
                scope_meta::ProviderErrorKind::Unavailable,
                error.to_string(),
                error,
            )
        })?;
        if self.truncated_paths.is_empty() {
            Ok(scope_meta::MetaSnapshot::complete(graph))
        } else {
            let warnings = self
                .truncated_paths
                .iter()
                .map(|path| {
                    let locator = if path.as_str() == "." {
                        "project".to_owned()
                    } else {
                        format!("project/{}", path.as_str())
                    };
                    scope_meta::SnapshotWarning::new(
                        "scope.workspace.depth_limit",
                        "Directory not scanned because the snapshot depth limit was reached",
                    )
                    .with_locator(locator)
                })
                .collect();
            Ok(scope_meta::MetaSnapshot::incomplete(graph, warnings))
        }
    }
}

fn kind(name: &str) -> TypeId {
    TypeId::new(format!("scope.{name}"), "1")
}
fn node(path: &ScopePath, name: &str, label: &str) -> MetaNode {
    MetaNode {
        id: NodeId::new(path.as_str()),
        kind: kind(name),
        label: label.into(),
        source: Some(SourceRef {
            locator: if path.as_str() == "." {
                "project".into()
            } else {
                format!("project/{}", path.as_str())
            },
            span: None,
            revision: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempWorkspace;

    fn path(text: &str) -> ScopePath {
        ScopePath::new(text).unwrap()
    }
    fn tracker(fixture: &TempWorkspace) -> WorkspaceMeta {
        WorkspaceMeta::new(
            Workspace::read_only(fixture.root().to_string_lossy()).unwrap(),
            SnapshotOptions::default(),
        )
        .unwrap()
    }

    #[test]
    fn common_provider_reports_truncation_and_staleness() {
        use scope_meta::{MetaProvider, ProviderErrorKind, SnapshotCompleteness};
        use std::error::Error;
        let fixture = TempWorkspace::new("meta-provider");
        fixture.write_file("src/a.txt", "text");
        let mut meta = WorkspaceMeta::new(
            Workspace::read_only(fixture.root().to_string_lossy()).unwrap(),
            SnapshotOptions {
                max_depth: 1,
                ..SnapshotOptions::default()
            },
        )
        .unwrap();
        let provider: &dyn MetaProvider = &meta;
        assert_eq!(provider.description().id, "scope.workspace@1");
        let snapshot = provider.snapshot().unwrap();
        assert!(std::ptr::eq(snapshot.graph, meta.graph().unwrap()));
        let SnapshotCompleteness::Incomplete { warnings } = snapshot.completeness else {
            panic!("depth-bound snapshot must be incomplete");
        };
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].locator.as_deref(), Some("project/src"));
        assert_eq!(warnings[0].code, "scope.workspace.depth_limit");
        meta.invalidate();
        let error = meta.snapshot().unwrap_err();
        assert_eq!(error.kind, ProviderErrorKind::Unavailable);
        assert!(error.source().unwrap().is::<WorkspaceMetaError>());
        meta.refresh().unwrap();
        assert!(meta.snapshot().is_ok());
        let complete = tracker(&fixture);
        assert_eq!(
            complete.snapshot().unwrap().completeness,
            SnapshotCompleteness::Complete
        );
    }

    #[test]
    fn tree_queries_and_sources_are_typed_and_deterministic() {
        let fixture = TempWorkspace::new("meta-tree");
        fixture.write_file("z.txt", "not read");
        fixture.write_file("src/nested/a.txt", "contents");
        fixture.create_dir("empty");
        fixture.write_file("src/.git/config", "excluded");
        fixture.write_file("target/build", "excluded");
        let meta = tracker(&fixture);
        let root = meta.node_for_path(&ScopePath::root()).unwrap();
        assert_eq!(root.kind, kind("workspace"));
        assert_eq!(
            meta.children(&root.id)
                .unwrap()
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            ["empty", "src", "z.txt"]
        );
        assert!(meta.parent(&root.id).unwrap().is_none());
        let src = meta.node_for_path(&path("src")).unwrap();
        assert_eq!(src.kind, kind("directory"));
        assert_eq!(src.source.as_ref().unwrap().locator, "project/src");
        let file = meta.node_for_path(&path("src/nested/a.txt")).unwrap();
        assert_eq!(file.kind, kind("file"));
        assert!(file.source.as_ref().unwrap().revision.is_none());
        assert_eq!(
            meta.parent(&file.id).unwrap().unwrap().id.as_str(),
            "src/nested"
        );
        assert!(meta.node_for_path(&path("target")).is_err());
        assert!(meta.node_for_path(&path("src/.git")).is_err());
        assert!(matches!(
            meta.children(&NodeId::new("absent")),
            Err(WorkspaceMetaError::Graph(scope_meta::Error::UnknownNode(_)))
        ));
    }

    #[test]
    fn discovery_does_not_require_readable_text_and_source_reads_can_fail() {
        let fixture = TempWorkspace::new("meta-binary");
        std::fs::write(fixture.root().join("binary"), [0xff, 0x00, 0xfe]).unwrap();
        let workspace = Workspace::read_only(fixture.root().to_string_lossy()).unwrap();
        let mut meta = WorkspaceMeta::new(workspace.clone(), SnapshotOptions::default()).unwrap();
        assert_eq!(
            meta.node_for_path(&path("binary")).unwrap().kind,
            kind("file")
        );
        assert!(matches!(
            workspace.load_file_revision("binary"),
            Err(WorkspaceError::ReadFile { .. })
        ));
        std::fs::remove_file(fixture.root().join("binary")).unwrap();
        assert!(matches!(
            workspace.load_file_revision("binary"),
            Err(WorkspaceError::ReadFile { .. })
        ));
        meta.invalidate();
        meta.refresh().unwrap();
        assert!(matches!(
            meta.node_for_path(&path("binary")),
            Err(WorkspaceMetaError::Graph(scope_meta::Error::UnknownNode(_)))
        ));
    }

    #[test]
    fn depth_and_entry_bounds_include_excluded_entries() {
        let fixture = TempWorkspace::new("meta-bounds");
        fixture.write_file("a/b/file", "x");
        fixture.create_dir("target");
        assert!(matches!(
            WorkspaceMeta::new(
                fixture.workspace(),
                SnapshotOptions {
                    max_entries: 0,
                    max_depth: 64
                }
            ),
            Err(WorkspaceMetaError::InvalidOptions)
        ));
        assert!(matches!(
            WorkspaceMeta::new(
                fixture.workspace(),
                SnapshotOptions {
                    max_entries: 2,
                    max_depth: 64
                }
            ),
            Err(WorkspaceMetaError::EntryLimit { .. })
        ));
        let root_only = WorkspaceMeta::new(
            fixture.workspace(),
            SnapshotOptions {
                max_entries: 1,
                max_depth: 0,
            },
        )
        .unwrap();
        assert_eq!(root_only.graph().unwrap().nodes().len(), 1);
        assert_eq!(root_only.truncated_paths().unwrap(), &[ScopePath::root()]);
        let shallow = WorkspaceMeta::new(
            fixture.workspace(),
            SnapshotOptions {
                max_entries: 3,
                max_depth: 1,
            },
        )
        .unwrap();
        assert_eq!(shallow.truncated_paths().unwrap(), &[path("a")]);
        assert_eq!(shallow.graph().unwrap().nodes().len(), 2);
    }

    #[test]
    fn refresh_identity_and_failed_refresh_never_reuse_old_refs() {
        let fixture = TempWorkspace::new("meta-refresh");
        let mut meta = WorkspaceMeta::new(
            fixture.workspace(),
            SnapshotOptions {
                max_entries: 1,
                max_depth: 64,
            },
        )
        .unwrap();
        let old = meta.graph().unwrap().node_ref(&NodeId::new(".")).unwrap();
        let other = tracker(&fixture);
        assert_ne!(
            meta.graph().unwrap().graph_id(),
            other.graph().unwrap().graph_id()
        );
        meta.refresh().unwrap();
        assert!(matches!(
            meta.graph().unwrap().resolve(&old),
            Err(scope_meta::Error::StaleRevision { .. })
        ));
        let revision = meta.revision;
        fixture.write_file("new", "x");
        assert!(matches!(
            meta.refresh(),
            Err(WorkspaceMetaError::EntryLimit { .. })
        ));
        assert!(meta.revision > revision);
        assert!(matches!(
            meta.graph(),
            Err(WorkspaceMetaError::StaleSnapshot)
        ));
        assert!(meta.truncated_paths().is_err());
        std::fs::remove_file(fixture.root().join("new")).unwrap();
        meta.refresh().unwrap();
        assert!(meta.graph().unwrap().resolve(&old).is_err());
        meta.invalidate();
        assert!(matches!(
            meta.node_for_path(&ScopePath::root()),
            Err(WorkspaceMetaError::StaleSnapshot)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn links_are_skipped_but_count_toward_budget() {
        let fixture = TempWorkspace::new("meta-links");
        fixture.write_file("file", "x");
        fixture.create_dir("directory");
        fixture.symlink("file", "file-link");
        fixture.symlink("directory", "dir-link");
        fixture.symlink("missing", "broken-link");
        let meta = tracker(&fixture);
        assert_eq!(meta.graph().unwrap().nodes().len(), 3);
        assert!(matches!(
            WorkspaceMeta::new(
                fixture.workspace(),
                SnapshotOptions {
                    max_entries: 5,
                    max_depth: 64
                }
            ),
            Err(WorkspaceMetaError::EntryLimit { .. })
        ));
    }

    #[test]
    fn missing_directory_lookup_returns_workspace_error() {
        let fixture = TempWorkspace::new("meta-missing");
        let workspace = fixture.workspace();
        assert!(matches!(
            workspace.meta_list_dir(&path("missing")),
            Err(WorkspaceError::ReadDirectory { .. })
        ));
        assert!(matches!(
            workspace.meta_is_directory(&path("missing")),
            Err(WorkspaceError::ReadDirectory { .. })
        ));
    }

    // macOS filesystems reject this fixture at creation; Linux permits it.
    #[cfg(target_os = "linux")]
    #[test]
    fn capability_scan_failure_does_not_publish_partial_snapshot() {
        use std::os::unix::ffi::OsStringExt;
        let fixture = TempWorkspace::new("meta-scan-error");
        let mut meta = tracker(&fixture);
        let invalid = std::ffi::OsString::from_vec(vec![0xff]);
        std::fs::write(fixture.root().join(&invalid), b"x").unwrap();
        assert!(matches!(
            meta.refresh(),
            Err(WorkspaceMetaError::Workspace(
                WorkspaceError::ReadDirectory { .. }
            ))
        ));
        assert!(matches!(
            meta.graph(),
            Err(WorkspaceMetaError::StaleSnapshot)
        ));
        std::fs::remove_file(fixture.root().join(&invalid)).unwrap();
        meta.refresh().unwrap();
    }

    #[test]
    fn failed_watch_subscription_invalidates_snapshot() {
        let fixture = TempWorkspace::new("meta-watch-failure");
        fixture.write_file("file", "x");
        let mut meta = tracker(&fixture);
        assert!(matches!(
            meta.enable_watch(WatchOptions {
                max_entries: 0,
                max_events: 0
            }),
            Err(WorkspaceMetaError::Watch(_))
        ));
        assert!(matches!(
            meta.graph(),
            Err(WorkspaceMetaError::StaleSnapshot)
        ));
        assert!(matches!(
            meta.poll_changes(),
            Err(WorkspaceMetaError::WatchNotEnabled)
        ));
        meta.refresh().unwrap();
    }

    #[test]
    fn optional_watch_invalidates_modifications_deletions_and_excluded_changes() {
        let fixture = TempWorkspace::new("meta-watch");
        fixture.write_file("file", "x");
        fixture.create_dir("target");
        let mut meta = tracker(&fixture);
        assert!(matches!(
            meta.poll_changes(),
            Err(WorkspaceMetaError::WatchNotEnabled)
        ));
        let old = meta
            .graph()
            .unwrap()
            .node_ref(&NodeId::new("file"))
            .unwrap();
        meta.enable_watch(WatchOptions::default()).unwrap();
        assert!(meta.graph().unwrap().resolve(&old).is_err());
        assert!(!meta.poll_changes().unwrap());
        fixture.write_file("file", "longer contents");
        assert!(meta.poll_changes().unwrap());
        assert!(meta.graph().is_err());
        meta.refresh().unwrap();
        std::fs::remove_file(fixture.root().join("file")).unwrap();
        assert!(meta.poll_changes().unwrap());
        meta.refresh().unwrap();
        assert!(meta.node_for_path(&path("file")).is_err());
        fixture.write_file("target/new", "excluded but observed");
        assert!(meta.poll_changes().unwrap());
    }

    #[test]
    fn watch_rescan_invalidates_snapshot() {
        let fixture = TempWorkspace::new("meta-watch-rescan");
        let mut meta = tracker(&fixture);
        meta.enable_watch(WatchOptions {
            max_entries: 1,
            max_events: 1,
        })
        .unwrap();
        fixture.write_file("one", "x");
        fixture.write_file("two", "x");
        assert!(meta.poll_changes().unwrap());
        assert!(matches!(
            meta.graph(),
            Err(WorkspaceMetaError::StaleSnapshot)
        ));
    }
}
