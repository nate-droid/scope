//! Capability-backed, language-neutral source sessions with explicitly authorized editing.

#[path = "editing.rs"]
pub mod editing;

use std::sync::Arc;

use super::DocumentId;
use crate::path::{ScopePath, ToScopePath};
use crate::workspace::{RevisionedWorkspaceFile, Workspace, WorkspaceError};

/// Opaque session-local expected-state token, not a persisted-file save token.
/// Equality identifies the same accepted snapshot in the same session. Reloading
/// unchanged text preserves it; accepting changed text creates a new token.
#[derive(Debug, Clone)]
pub struct Revision(Arc<()>);

impl PartialEq for Revision {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Revision {}

/// An owned, immutable-by-session source value. Retaining it does not imply that
/// it is still current; use `snapshot_at` before accepting derived results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSnapshot {
    pub document_id: DocumentId,
    pub path: ScopePath,
    pub revision: Revision,
    pub text: String,
}

#[derive(Debug)]
pub enum DocumentError {
    InvalidPath(WorkspaceError),
    Filesystem(cell_capability_fs::FsError),
    StaleRevision,
    Closed,
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath(source) => source.fmt(f),
            Self::Filesystem(source) => source.fmt(f),
            Self::StaleRevision => {
                f.write_str("document source revision is stale; reload required")
            }
            Self::Closed => f.write_str("document session is closed"),
        }
    }
}

impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidPath(source) => Some(source),
            Self::Filesystem(source) => Some(source),
            _ => None,
        }
    }
}

/// A single confined document, opened through an explicitly supplied workspace.
/// Caller-provided identity follows the existing `DocumentId` allocation contract.
///
/// Every read rechecks the persisted source through the capability. A changed or
/// unreadable source invalidates the retained snapshot until explicit reload.
/// Checks describe the observed read only: external writers may act immediately
/// afterward, and changes reverted between reads cannot be detected. No atomic
/// compare-and-swap, filesystem watch, or language cache is implied. Authorized
/// editing uses the workspace's best-effort checked save, not atomic CAS.
#[derive(Debug)]
pub struct DocumentSession {
    workspace: Option<Workspace>,
    document_id: DocumentId,
    path: ScopePath,
    snapshot: Option<DocumentSnapshot>,
    disk_revision: String,
}

impl DocumentSession {
    pub fn open(
        workspace: Workspace,
        document_id: DocumentId,
        path: impl ToScopePath,
    ) -> Result<Self, DocumentError> {
        let path = path.to_scope_path().map_err(|source| {
            DocumentError::InvalidPath(WorkspaceError::InvalidPath {
                message: source.to_string(),
            })
        })?;
        let file = workspace
            .read_document_source(&path)
            .map_err(DocumentError::Filesystem)?;
        let mut session = Self {
            workspace: Some(workspace),
            document_id,
            path,
            snapshot: None,
            disk_revision: String::new(),
        };
        session.accept(file);
        Ok(session)
    }

    fn read_source(&self) -> Result<RevisionedWorkspaceFile, DocumentError> {
        self.workspace
            .as_ref()
            .ok_or(DocumentError::Closed)?
            .read_document_source(&self.path)
            .map_err(DocumentError::Filesystem)
    }

    /// Read and validate the accepted snapshot. Does not silently accept changes.
    pub fn snapshot(&mut self) -> Result<DocumentSnapshot, DocumentError> {
        let file = match self.read_source() {
            Ok(file) => file,
            Err(error) => {
                self.snapshot = None;
                return Err(error);
            }
        };
        match &self.snapshot {
            Some(snapshot)
                if file.revision == self.disk_revision && file.contents == snapshot.text =>
            {
                Ok(snapshot.clone())
            }
            _ => {
                self.snapshot = None;
                Err(DocumentError::StaleRevision)
            }
        }
    }

    /// Validate both the observed source and a downstream result's session token.
    /// A conflicting caller token does not invalidate an otherwise current source.
    pub fn snapshot_at(&mut self, expected: &Revision) -> Result<DocumentSnapshot, DocumentError> {
        let snapshot = self.snapshot()?;
        if &snapshot.revision != expected {
            return Err(DocumentError::StaleRevision);
        }
        Ok(snapshot)
    }

    /// Explicitly accept the current source, replacing any invalidated snapshot.
    pub fn reload(&mut self) -> Result<DocumentSnapshot, DocumentError> {
        let file = match self.read_source() {
            Ok(file) => file,
            Err(error) => {
                self.snapshot = None;
                return Err(error);
            }
        };
        Ok(self.accept(file))
    }

    fn accept(&mut self, file: RevisionedWorkspaceFile) -> DocumentSnapshot {
        let revision = match &self.snapshot {
            Some(snapshot)
                if file.revision == self.disk_revision && file.contents == snapshot.text =>
            {
                snapshot.revision.clone()
            }
            _ => Revision(Arc::new(())),
        };
        let snapshot = DocumentSnapshot {
            document_id: self.document_id,
            path: self.path.clone(),
            revision,
            text: file.contents,
        };
        self.disk_revision = file.revision;
        self.snapshot = Some(snapshot.clone());
        snapshot
    }

    /// Idempotently release the retained workspace grant and cached source.
    /// Other workspace clones and previously returned snapshots remain owned by callers.
    pub fn close(&mut self) {
        self.snapshot = None;
        self.workspace = None;
        self.disk_revision.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempWorkspace;

    fn open(fixture: &TempWorkspace) -> DocumentSession {
        DocumentSession::open(
            Workspace::read_only(fixture.root().to_string_lossy()).unwrap(),
            DocumentId::new(1),
            "example.txt",
        )
        .unwrap()
    }

    #[test]
    fn stable_snapshot_and_session_scoped_tokens() {
        let fixture = TempWorkspace::new("session-stable");
        fixture.write_file("example.txt", "hello 🦀\n");
        let mut session = open(&fixture);
        let first = session.snapshot().unwrap();
        assert_eq!(first.text, "hello 🦀\n");
        assert_eq!(first.document_id, DocumentId::new(1));
        assert_eq!(first, session.snapshot_at(&first.revision).unwrap());
        assert_eq!(first, session.reload().unwrap());
        let foreign = open(&fixture).snapshot().unwrap();
        assert!(matches!(
            session.snapshot_at(&foreign.revision),
            Err(DocumentError::StaleRevision)
        ));
        assert_eq!(first, session.snapshot().unwrap());
    }

    #[test]
    fn external_changes_invalidate_until_reload() {
        let fixture = TempWorkspace::new("session-change");
        fixture.write_file("example.txt", "before");
        let mut session = open(&fixture);
        let first = session.snapshot().unwrap();
        fixture.write_file("example.txt", "after");
        assert!(matches!(
            session.snapshot_at(&first.revision),
            Err(DocumentError::StaleRevision)
        ));
        fixture.write_file("example.txt", "before");
        assert!(matches!(
            session.snapshot(),
            Err(DocumentError::StaleRevision)
        ));
        let next = session.reload().unwrap();
        assert_ne!(next.revision, first.revision);
        assert!(matches!(
            session.snapshot_at(&first.revision),
            Err(DocumentError::StaleRevision)
        ));
        fixture.write_file("example.txt", "new source");
        let next = session.reload().unwrap();
        assert_eq!(next.text, "new source");
    }

    #[test]
    fn read_only_and_close() {
        let fixture = TempWorkspace::new("session-readonly");
        fixture.write_file("example.txt", "inspect");
        let workspace = Workspace::read_only(fixture.root().to_string_lossy()).unwrap();
        let mut session =
            DocumentSession::open(workspace.clone(), DocumentId::new(2), "example.txt").unwrap();
        assert_eq!(session.snapshot().unwrap().text, "inspect");
        assert!(workspace.save_file("example.txt", "denied").is_err());
        assert_eq!(fixture.read_file("example.txt"), "inspect");
        session.close();
        session.close();
        assert!(matches!(session.snapshot(), Err(DocumentError::Closed)));
        assert!(matches!(session.reload(), Err(DocumentError::Closed)));
    }

    #[test]
    fn missing_invalid_text_and_escape_fail_explicitly() {
        let fixture = TempWorkspace::new("session-errors");
        assert!(matches!(
            DocumentSession::open(fixture.workspace(), DocumentId::new(1), "example.txt"),
            Err(DocumentError::Filesystem(_))
        ));
        fixture.write_bytes("example.txt", &[0xff]);
        assert!(matches!(
            DocumentSession::open(fixture.workspace(), DocumentId::new(1), "example.txt"),
            Err(DocumentError::Filesystem(_))
        ));
        assert!(matches!(
            DocumentSession::open(fixture.workspace(), DocumentId::new(1), "../example.txt"),
            Err(DocumentError::InvalidPath(_))
        ));
        assert!(
            DocumentSession::open(fixture.workspace(), DocumentId::new(1), "/example.txt").is_err()
        );
    }

    #[test]
    fn read_failure_invalidates_previous_token() {
        let fixture = TempWorkspace::new("session-unreadable");
        fixture.write_file("example.txt", "before");
        let mut session = open(&fixture);
        let first = session.snapshot().unwrap();
        fixture.write_bytes("example.txt", &[0xff]);
        assert!(matches!(
            session.snapshot(),
            Err(DocumentError::Filesystem(_))
        ));
        fixture.write_file("example.txt", "before");
        assert!(matches!(
            session.snapshot(),
            Err(DocumentError::StaleRevision)
        ));
        assert_ne!(session.reload().unwrap().revision, first.revision);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_denied() {
        let fixture = TempWorkspace::new("session-confined");
        let outside = TempWorkspace::new("session-outside");
        outside.write_file("example.txt", "outside");
        fixture.symlink(outside.root().join("example.txt"), "example.txt");
        assert!(matches!(
            DocumentSession::open(fixture.workspace(), DocumentId::new(1), "example.txt"),
            Err(DocumentError::Filesystem(_))
        ));
    }
}
