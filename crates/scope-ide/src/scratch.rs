//! In-memory scratch documents with explicit, revision-checked persistence.
//!
//! Destinations must already exist: the current Workspace checked-save API cannot
//! create files. Binding only observes a file; promotion explicitly authorizes
//! replacing its observed contents. Replacement is atomic, but revision checking
//! is best-effort rather than an atomic compare-and-swap.

use scope_language::{EvaluationRequest, RevisionId, SourceId};

use crate::{
    language::{InteractiveSession, SessionError},
    path::ScopePath,
    text::{TextBuffer, TextBufferError, TextEdit},
    workspace::{Workspace, WorkspaceError},
};

#[derive(Debug)]
pub enum ScratchError {
    NoSelection,
    UnknownBuffer(u64),
    NoDestination,
    UnsavedChanges(u64),
    AlreadyOpen(u64),
    WorkspaceMismatch {
        expected: String,
        actual: String,
    },
    LanguageMismatch {
        expected: String,
        actual: Option<String>,
    },
    Text(TextBufferError),
    Session(SessionError),
    Workspace(WorkspaceError),
    /// Includes missing destinations; binding never creates a file.
    Destination(cell_capability_fs::FsError),
    Filesystem(cell_capability_fs::FsError),
}

impl std::fmt::Display for ScratchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSelection => write!(f, "no scratch buffer selected"),
            Self::UnknownBuffer(id) => write!(f, "unknown scratch buffer {id}"),
            Self::NoDestination => write!(f, "scratch buffer has no destination"),
            Self::UnsavedChanges(id) => write!(
                f,
                "buffer {id} has unsaved changes; save or explicitly discard"
            ),
            Self::AlreadyOpen(id) => write!(f, "destination is already open in buffer {id}"),
            Self::WorkspaceMismatch { expected, actual } => write!(
                f,
                "destination belongs to workspace {expected}, not {actual}"
            ),
            Self::LanguageMismatch { expected, actual } => write!(
                f,
                "scratch language {expected} does not match session {actual:?}"
            ),
            Self::Filesystem(error) => error.fmt(f),
            Self::Text(error) => error.fmt(f),
            Self::Session(error) => error.fmt(f),
            Self::Workspace(error) => error.fmt(f),
            Self::Destination(error) => write!(
                f,
                "cannot bind existing destination (new files are unsupported): {error}"
            ),
        }
    }
}

impl std::error::Error for ScratchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Text(error) => Some(error),
            Self::Session(error) => Some(error),
            Self::Workspace(error) => Some(error),
            Self::Destination(error) | Self::Filesystem(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Destination {
    root: String,
    path: ScopePath,
    revision: String,
}

#[derive(Debug)]
pub struct ScratchBuffer {
    id: u64,
    source_id: SourceId,
    language: String,
    buffer: TextBuffer,
    dirty: bool,
    version: u64,
    destination: Option<Destination>,
}

impl ScratchBuffer {
    pub fn id(&self) -> u64 {
        self.id
    }
    /// Stable evaluation identity, independent of the buffer's destination and version.
    pub fn evaluation_source_id(&self) -> &SourceId {
        &self.source_id
    }
    pub fn language_id(&self) -> &str {
        &self.language
    }
    pub fn text(&self) -> &str {
        self.buffer.as_str()
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn destination_path(&self) -> Option<&ScopePath> {
        self.destination
            .as_ref()
            .map(|destination| &destination.path)
    }
    pub fn destination_revision(&self) -> Option<&str> {
        self.destination
            .as_ref()
            .map(|destination| destination.revision.as_str())
    }

    /// Successful edits advance the version, including edits with identical text.
    pub fn apply_edit(&mut self, edit: TextEdit) -> Result<(), ScratchError> {
        self.buffer.apply_edit(edit).map_err(ScratchError::Text)?;
        self.version = self
            .version
            .checked_add(1)
            .expect("scratch version exhausted");
        self.dirty = true;
        Ok(())
    }

    /// Reload is transactional: neither rejected discards nor read failures change state.
    pub fn reload(&mut self, workspace: &Workspace, discard: bool) -> Result<(), ScratchError> {
        if self.dirty && !discard {
            return Err(ScratchError::UnsavedChanges(self.id));
        }
        let destination = self
            .destination
            .as_ref()
            .ok_or(ScratchError::NoDestination)?;
        if destination.root != workspace.root() {
            return Err(ScratchError::WorkspaceMismatch {
                expected: destination.root.clone(),
                actual: workspace.root().to_owned(),
            });
        }
        let file = workspace
            .read_document_source(&destination.path)
            .map_err(ScratchError::Destination)?;
        self.buffer = TextBuffer::new(file.contents);
        self.destination.as_mut().unwrap().revision = file.revision;
        self.version = self
            .version
            .checked_add(1)
            .expect("scratch version exhausted");
        self.dirty = false;
        Ok(())
    }

    /// Evaluates literal source in the caller's session without routing commands,
    /// switching languages, resetting the session, or changing document state.
    pub fn evaluate(
        &self,
        session: &mut InteractiveSession,
    ) -> Result<scope_language::EvaluationResult, ScratchError> {
        if session.language_id() != Some(self.language_id()) {
            return Err(ScratchError::LanguageMismatch {
                expected: self.language.clone(),
                actual: session.language_id().map(str::to_owned),
            });
        }
        session
            .evaluate_request(EvaluationRequest {
                text: self.text(),
                source_id: self.evaluation_source_id().clone(),
                revision_id: RevisionId::from_version(self.version()),
            })
            .map_err(ScratchError::Session)
    }

    /// Observes an existing confined file without writing or changing dirty state.
    /// Explicit rebinding refreshes the baseline, allowing conflict recovery.
    /// A failed bind leaves the previous destination intact.
    pub fn bind_destination(
        &mut self,
        workspace: &Workspace,
        path: &str,
    ) -> Result<(), ScratchError> {
        let path = ScopePath::new(path).map_err(|error| {
            ScratchError::Workspace(WorkspaceError::InvalidPath {
                message: error.to_string(),
            })
        })?;
        let file = workspace
            .read_document_source(&path)
            .map_err(ScratchError::Destination)?;
        self.destination = Some(Destination {
            root: workspace.root().to_owned(),
            path,
            revision: file.revision,
        });
        Ok(())
    }

    /// Explicitly replaces observed existing contents. Repeating promotion to the
    /// same destination retains its baseline so external changes cause conflicts.
    pub fn promote(&mut self, workspace: &Workspace, path: &str) -> Result<(), ScratchError> {
        self.dirty = true;
        let same_destination = self.destination.as_ref().is_some_and(|destination| {
            destination.root == workspace.root() && destination.path.as_str() == path
        });
        if !same_destination {
            self.bind_destination(workspace, path)?;
        }
        self.save(workspace)
    }

    /// Checked replacement only; every failure leaves the document dirty and
    /// preserves the destination and baseline. Persistence does not edit version.
    pub fn save(&mut self, workspace: &Workspace) -> Result<(), ScratchError> {
        self.dirty = true;
        let destination = self
            .destination
            .as_mut()
            .ok_or(ScratchError::NoDestination)?;
        if destination.root != workspace.root() {
            return Err(ScratchError::WorkspaceMismatch {
                expected: destination.root.clone(),
                actual: workspace.root().to_owned(),
            });
        }
        let revision = checked_save(
            workspace,
            &destination.path,
            self.buffer.as_str(),
            &destination.revision,
        )?;
        destination.revision = revision;
        self.dirty = false;
        Ok(())
    }
}

fn checked_save(
    workspace: &Workspace,
    path: &ScopePath,
    text: &str,
    revision: &str,
) -> Result<String, ScratchError> {
    match workspace
        .save_document_source(path, text, revision)
        .map_err(ScratchError::Filesystem)?
    {
        cell_capability_fs::SaveOutcome::Saved(revision) => Ok(revision),
        cell_capability_fs::SaveOutcome::Conflict(current_revision) => {
            Err(ScratchError::Workspace(WorkspaceError::Conflict {
                path: path.clone(),
                current_revision,
            }))
        }
    }
}

/// Scratch IDs are monotonic and never reused. New buffers are dirty at version
/// zero. Discard selects the newest remaining buffer, or clears selection.
#[derive(Debug, Default)]
pub struct ScratchBuffers {
    buffers: Vec<ScratchBuffer>,
    selected: Option<u64>,
    next_id: u64,
}

impl ScratchBuffers {
    /// Opens a clean workspace document, or selects its existing buffer without rereading.
    pub fn open(
        &mut self,
        workspace: &Workspace,
        languages: &crate::language::LanguageRegistry,
        path: &str,
    ) -> Result<u64, ScratchError> {
        let path = ScopePath::new(path).map_err(|error| {
            ScratchError::Workspace(WorkspaceError::InvalidPath {
                message: error.to_string(),
            })
        })?;
        if let Some(id) = self
            .buffers
            .iter()
            .find(|buffer| {
                buffer.destination.as_ref().is_some_and(|destination| {
                    destination.root == workspace.root() && destination.path == path
                })
            })
            .map(|buffer| buffer.id)
        {
            self.selected = Some(id);
            return Ok(id);
        }
        let file = workspace
            .read_document_source(&path)
            .map_err(ScratchError::Destination)?;
        let language = languages
            .language_for_path(&path)
            .map(|language| language.language_id())
            .unwrap_or("text");
        let id = self.create(language, file.contents);
        let buffer = self.selected_mut()?;
        buffer.destination = Some(Destination {
            root: workspace.root().to_owned(),
            path,
            revision: file.revision,
        });
        buffer.dirty = false;
        Ok(id)
    }

    /// Save As observes an existing destination and commits the binding only after success.
    pub fn save_as_selected(
        &mut self,
        workspace: &Workspace,
        path: &str,
    ) -> Result<(), ScratchError> {
        let path = ScopePath::new(path).map_err(|error| {
            ScratchError::Workspace(WorkspaceError::InvalidPath {
                message: error.to_string(),
            })
        })?;
        let id = self.selected()?.id;
        if let Some(other) = self.buffers.iter().find(|buffer| {
            buffer.id != id
                && buffer.destination.as_ref().is_some_and(|destination| {
                    destination.root == workspace.root() && destination.path == path
                })
        }) {
            return Err(ScratchError::AlreadyOpen(other.id));
        }
        let buffer = self.selected_mut()?;
        if buffer.destination.as_ref().is_some_and(|destination| {
            destination.root == workspace.root() && destination.path == path
        }) {
            return buffer.save(workspace);
        }
        let file = workspace
            .read_document_source(&path)
            .map_err(ScratchError::Destination)?;
        let revision = checked_save(workspace, &path, buffer.text(), &file.revision)?;
        buffer.destination = Some(Destination {
            root: workspace.root().to_owned(),
            path,
            revision,
        });
        buffer.dirty = false;
        Ok(())
    }

    pub fn close_selected(&mut self, discard: bool) -> Result<(), ScratchError> {
        let buffer = self.selected()?;
        if buffer.is_dirty() && !discard {
            return Err(ScratchError::UnsavedChanges(buffer.id));
        }
        self.discard_selected()
    }

    pub fn create(&mut self, language: impl Into<String>, source: impl Into<String>) -> u64 {
        let id = self.next_id;
        self.next_id = id.checked_add(1).expect("scratch IDs exhausted");
        self.buffers.push(ScratchBuffer {
            id,
            source_id: SourceId::fresh(),
            language: language.into(),
            buffer: TextBuffer::new(source),
            dirty: true,
            version: 0,
            destination: None,
        });
        self.selected = Some(id);
        id
    }
    pub fn list(&self) -> impl Iterator<Item = &ScratchBuffer> {
        self.buffers.iter()
    }
    /// Resolve a live buffer without changing the user's selection.
    pub fn get(&self, id: u64) -> Result<&ScratchBuffer, ScratchError> {
        self.buffers
            .iter()
            .find(|buffer| buffer.id == id)
            .ok_or(ScratchError::UnknownBuffer(id))
    }
    /// Resolve a live buffer for an explicit host operation without selecting it.
    pub fn get_mut(&mut self, id: u64) -> Result<&mut ScratchBuffer, ScratchError> {
        self.buffers
            .iter_mut()
            .find(|buffer| buffer.id == id)
            .ok_or(ScratchError::UnknownBuffer(id))
    }
    pub fn selected(&self) -> Result<&ScratchBuffer, ScratchError> {
        let id = self.selected.ok_or(ScratchError::NoSelection)?;
        self.buffers
            .iter()
            .find(|buffer| buffer.id == id)
            .ok_or(ScratchError::UnknownBuffer(id))
    }
    pub fn selected_mut(&mut self) -> Result<&mut ScratchBuffer, ScratchError> {
        let id = self.selected.ok_or(ScratchError::NoSelection)?;
        self.buffers
            .iter_mut()
            .find(|buffer| buffer.id == id)
            .ok_or(ScratchError::UnknownBuffer(id))
    }
    pub fn select(&mut self, id: u64) -> Result<(), ScratchError> {
        if !self.buffers.iter().any(|buffer| buffer.id == id) {
            return Err(ScratchError::UnknownBuffer(id));
        }
        self.selected = Some(id);
        Ok(())
    }
    pub fn discard_selected(&mut self) -> Result<(), ScratchError> {
        let id = self.selected()?.id;
        self.buffers.retain(|buffer| buffer.id != id);
        self.selected = self.buffers.last().map(|buffer| buffer.id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        language::{
            EvaluationResult, EvaluationStatus, InteractiveLanguageSession,
            InteractiveSessionProvider, LanguageRegistry,
        },
        test_support::TempWorkspace,
        text::{Position, TextRange},
    };
    use std::sync::{Arc, Mutex};

    #[test]
    fn workspace_lifecycle_and_failed_save_recovery() {
        let root = TempWorkspace::new("workspace_buffers");
        root.write_file("a.scope", "α\r\n雪\n");
        root.write_file("b.txt", "target");
        let workspace = root.workspace();
        let registry = LanguageRegistry::builtins();
        let mut buffers = ScratchBuffers::default();
        let a = buffers.open(&workspace, &registry, "a.scope").unwrap();
        assert_eq!(buffers.selected().unwrap().text(), "α\r\n雪\n");
        assert!(!buffers.selected().unwrap().is_dirty());
        assert_eq!(buffers.selected().unwrap().language_id(), "scope.simple");
        buffers
            .selected_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), "edited"))
            .unwrap();
        let baseline = buffers
            .selected()
            .unwrap()
            .destination_revision()
            .unwrap()
            .to_owned();
        assert_eq!(buffers.open(&workspace, &registry, "a.scope").unwrap(), a);
        assert!(buffers.selected().unwrap().text().starts_with("edited"));
        assert!(buffers.close_selected(false).is_err());
        assert!(
            buffers
                .selected_mut()
                .unwrap()
                .reload(&workspace, false)
                .is_err()
        );
        let b = buffers.open(&workspace, &registry, "b.txt").unwrap();
        assert_ne!(a, b);
        assert_eq!(buffers.list().count(), 2);
        buffers.select(a).unwrap();
        assert!(matches!(
            buffers.save_as_selected(&workspace, "b.txt"),
            Err(ScratchError::AlreadyOpen(_))
        ));
        root.write_file("a.scope", "external");
        assert!(matches!(
            buffers.selected_mut().unwrap().save(&workspace),
            Err(ScratchError::Workspace(WorkspaceError::Conflict { .. }))
        ));
        assert_eq!(
            buffers.selected().unwrap().destination_revision(),
            Some(baseline.as_str())
        );
        assert!(buffers.selected().unwrap().is_dirty());
        let readonly = Workspace::read_only(workspace.root()).unwrap();
        assert!(buffers.save_as_selected(&readonly, "missing.txt").is_err());
        assert!(buffers.selected().unwrap().is_dirty());
        buffers
            .selected_mut()
            .unwrap()
            .reload(&workspace, true)
            .unwrap();
        assert_eq!(buffers.selected().unwrap().text(), "external");
        buffers
            .selected_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), "new "))
            .unwrap();
        assert!(matches!(
            buffers.selected_mut().unwrap().save(&readonly),
            Err(ScratchError::Filesystem(_))
        ));
        assert!(buffers.selected().unwrap().is_dirty());
        buffers.select(b).unwrap();
        buffers.close_selected(false).unwrap();
        buffers.save_as_selected(&workspace, "b.txt").unwrap();
        assert_eq!(root.read_file("b.txt"), "new external");
        assert!(!buffers.selected().unwrap().is_dirty());
        buffers.close_selected(false).unwrap();
        assert!(buffers.selected().is_err());
        assert!(buffers.open(&workspace, &registry, "../escape").is_err());
    }

    fn buffers(source: &str) -> ScratchBuffers {
        let mut buffers = ScratchBuffers::default();
        buffers.create("mock", source);
        buffers
    }

    #[test]
    fn selection_discard_and_ids() {
        let mut buffers = ScratchBuffers::default();
        assert!(matches!(buffers.selected(), Err(ScratchError::NoSelection)));
        assert!(matches!(
            buffers.selected_mut(),
            Err(ScratchError::NoSelection)
        ));
        assert!(buffers.discard_selected().is_err());
        let a = buffers.create("a", "first");
        let b = buffers.create("b", "second");
        assert_eq!(buffers.selected().unwrap().id(), b);
        assert_eq!(
            buffers.list().map(ScratchBuffer::id).collect::<Vec<_>>(),
            vec![a, b]
        );
        assert!(matches!(
            buffers.select(999),
            Err(ScratchError::UnknownBuffer(999))
        ));
        assert_eq!(buffers.selected().unwrap().id(), b);
        buffers.select(a).unwrap();
        buffers.discard_selected().unwrap();
        assert_eq!(buffers.selected().unwrap().id(), b);
        buffers.discard_selected().unwrap();
        assert!(buffers.selected().is_err());
        assert!(buffers.create("mock", "") > b);
    }

    #[test]
    fn unicode_edits_are_transactional() {
        let mut buffers = buffers("é\n世界");
        let buffer = buffers.selected_mut().unwrap();
        assert_eq!(buffer.version(), 0);
        assert!(buffer.is_dirty());
        assert!(matches!(
            buffer.apply_edit(TextEdit::insert(Position::new(0, 1), "x")),
            Err(ScratchError::Text(_))
        ));
        assert_eq!(buffer.text(), "é\n世界");
        assert_eq!(buffer.version(), 0);
        buffer
            .apply_edit(TextEdit::replace(
                TextRange::new(Position::new(0, 2), Position::new(1, 3)),
                "🙂",
            ))
            .unwrap();
        assert_eq!(buffer.text(), "é🙂界");
        assert_eq!(buffer.version(), 1);
        buffer
            .apply_edit(TextEdit::insert(Position::new(0, 9), ""))
            .unwrap();
        assert_eq!(buffer.version(), 2);
    }

    #[test]
    fn binding_is_read_only_and_checked_save_updates_baseline() {
        let fixture = TempWorkspace::new("scratch-bind");
        fixture.write_file("文.txt", "original");
        let workspace = fixture.workspace();
        let mut buffers = buffers("é🙂");
        let buffer = buffers.selected_mut().unwrap();
        buffer.bind_destination(&workspace, "文.txt").unwrap();
        let baseline = buffer.destination_revision().unwrap().to_owned();
        assert_eq!(fixture.read_file("文.txt"), "original");
        assert!(buffer.is_dirty());
        buffer.save(&workspace).unwrap();
        assert!(!buffer.is_dirty());
        assert_ne!(buffer.destination_revision(), Some(baseline.as_str()));
        assert_eq!(fixture.read_file("文.txt"), "é🙂");
        assert_eq!(buffer.version(), 0);
        buffer
            .apply_edit(TextEdit::insert(Position::new(0, 6), "!"))
            .unwrap();
        buffer.save(&workspace).unwrap();
        assert_eq!(fixture.read_file("文.txt"), "é🙂!");
        assert!(!buffer.is_dirty());
    }

    #[test]
    fn same_path_promotion_preserves_conflicts_and_explicit_rebind_recovers() {
        let fixture = TempWorkspace::new("scratch-conflict");
        fixture.write_file("a", "old");
        fixture.write_file("b", "other");
        let workspace = fixture.workspace();
        let mut buffers = buffers("scratch");
        let buffer = buffers.selected_mut().unwrap();
        buffer.promote(&workspace, "a").unwrap();
        let baseline = buffer.destination_revision().unwrap().to_owned();
        fixture.write_file("a", "external");
        assert!(matches!(
            buffer.promote(&workspace, "a"),
            Err(ScratchError::Workspace(WorkspaceError::Conflict { .. }))
        ));
        assert!(buffer.is_dirty());
        assert_eq!(buffer.destination_revision(), Some(baseline.as_str()));
        assert_eq!(fixture.read_file("a"), "external");
        buffer.bind_destination(&workspace, "a").unwrap();
        buffer.save(&workspace).unwrap();
        assert!(!buffer.is_dirty());
        buffer.promote(&workspace, "b").unwrap();
        assert_eq!(fixture.read_file("b"), "scratch");
    }

    #[test]
    fn missing_invalid_and_denied_destinations_preserve_state() {
        let fixture = TempWorkspace::new("scratch-failures");
        fixture.write_file("a", "old");
        let workspace = fixture.workspace();
        let readonly = Workspace::read_only(fixture.root().to_string_lossy()).unwrap();
        let mut buffers = buffers("scratch");
        let buffer = buffers.selected_mut().unwrap();
        assert!(matches!(
            buffer.save(&workspace),
            Err(ScratchError::NoDestination)
        ));
        buffer.promote(&workspace, "a").unwrap();
        assert!(matches!(
            buffer.bind_destination(&workspace, "missing"),
            Err(ScratchError::Destination(_))
        ));
        assert!(!buffer.is_dirty());
        assert_eq!(buffer.destination_path().unwrap().as_str(), "a");
        assert!(matches!(
            buffer.promote(&workspace, "../escape"),
            Err(ScratchError::Workspace(WorkspaceError::InvalidPath { .. }))
        ));
        assert!(buffer.is_dirty());
        buffer.save(&workspace).unwrap();
        assert!(matches!(
            buffer.save(&readonly),
            Err(ScratchError::Filesystem(_))
        ));
        assert!(buffer.is_dirty());
        assert_eq!(fixture.read_file("a"), "scratch");
        buffer.save(&workspace).unwrap();
        assert!(matches!(
            buffer.promote(&workspace, "missing"),
            Err(ScratchError::Destination(_))
        ));
        assert!(buffer.is_dirty());
        assert!(!fixture.host_path_exists("missing"));
        buffer.save(&workspace).unwrap();
        assert!(!buffer.is_dirty());
    }

    #[test]
    fn workspace_mismatch_cannot_write_another_root() {
        let a = TempWorkspace::new("scratch-root-a");
        let b = TempWorkspace::new("scratch-root-b");
        a.write_file("file", "a");
        b.write_file("file", "b");
        let mut buffers = buffers("scratch");
        let buffer = buffers.selected_mut().unwrap();
        buffer.promote(&a.workspace(), "file").unwrap();
        assert!(matches!(
            buffer.save(&b.workspace()),
            Err(ScratchError::WorkspaceMismatch { .. })
        ));
        assert!(buffer.is_dirty());
        assert_eq!(b.read_file("file"), "b");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_destination_cannot_escape() {
        let inside = TempWorkspace::new("scratch-inside");
        let outside = TempWorkspace::new("scratch-outside");
        outside.write_file("file", "untouched");
        inside.symlink(outside.root().join("file"), "escape");
        let mut buffers = buffers("scratch");
        assert!(
            buffers
                .selected_mut()
                .unwrap()
                .promote(&inside.workspace(), "escape")
                .is_err()
        );
        assert_eq!(outside.read_file("file"), "untouched");
    }

    #[derive(Default)]
    struct State {
        sources: Vec<String>,
        requests: Vec<(SourceId, RevisionId)>,
        resets: usize,
        fail_reset: bool,
    }
    struct Provider(Arc<Mutex<State>>);
    struct Mock(Arc<Mutex<State>>);
    impl InteractiveSessionProvider for Provider {
        fn language_id(&self) -> &'static str {
            "mock"
        }
        fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
            Ok(Box::new(Mock(self.0.clone())))
        }
    }
    impl InteractiveLanguageSession for Mock {
        fn language_id(&self) -> &str {
            "mock"
        }
        fn evaluate_request(
            &mut self,
            request: EvaluationRequest<'_>,
        ) -> Result<EvaluationResult, SessionError> {
            self.0
                .lock()
                .unwrap()
                .requests
                .push((request.source_id, request.revision_id));
            self.evaluate(request.text)
        }
        fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
            self.0.lock().unwrap().sources.push(source.to_owned());
            if source == "error" {
                return Err(SessionError::ResetRequired);
            }
            Ok(EvaluationResult {
                output: vec![source.to_owned()],
                diagnostics: vec![],
                status: if source == "poison" {
                    EvaluationStatus::Cancelled {
                        requires_reset: true,
                    }
                } else {
                    EvaluationStatus::Success
                },
            })
        }
        fn reset(&mut self) -> Result<(), SessionError> {
            let mut state = self.0.lock().unwrap();
            state.resets += 1;
            if state.fail_reset {
                Err(SessionError::ResetRequired)
            } else {
                Ok(())
            }
        }
    }
    fn session() -> (InteractiveSession, Arc<Mutex<State>>) {
        let state = Arc::new(Mutex::new(State::default()));
        let provider = Box::leak(Box::new(Provider(state.clone())));
        let registry = LanguageRegistry::empty().with_interactive_provider(provider);
        let mut session = InteractiveSession::new();
        session.select(&registry, "mock").unwrap();
        (session, state)
    }

    #[test]
    fn evaluation_uses_buffer_identity_literal_text_and_actual_version() {
        let (mut session, state) = session();
        let mut buffers = buffers(":scope help\n猫");
        let buffer = buffers.selected_mut().unwrap();
        let identity = buffer.evaluation_source_id().clone();
        buffer.evaluate(&mut session).unwrap();
        buffer.evaluate(&mut session).unwrap();
        buffer
            .apply_edit(TextEdit::insert(Position::new(1, 3), "!"))
            .unwrap();
        buffer.evaluate(&mut session).unwrap();
        session.reset().unwrap();
        buffer.evaluate(&mut session).unwrap();
        assert_eq!(buffer.evaluation_source_id(), &identity);
        let state = state.lock().unwrap();
        assert_eq!(
            state.sources,
            [
                ":scope help\n猫",
                ":scope help\n猫",
                ":scope help\n猫!",
                ":scope help\n猫!"
            ]
        );
        assert_eq!(
            state.requests,
            vec![
                (identity.clone(), RevisionId::from_version(0)),
                (identity.clone(), RevisionId::from_version(0)),
                (identity.clone(), RevisionId::from_version(1)),
                (identity, RevisionId::from_version(buffer.version())),
            ]
        );
    }

    #[test]
    fn source_identity_survives_selection_and_is_distinct_across_buffer_owners() {
        let mut buffers = ScratchBuffers::default();
        let first_id = buffers.create("mock", "same");
        let first = buffers.selected().unwrap().evaluation_source_id().clone();
        buffers.create("mock", "same");
        assert_ne!(buffers.selected().unwrap().evaluation_source_id(), &first);
        buffers.select(first_id).unwrap();
        assert_eq!(buffers.selected().unwrap().evaluation_source_id(), &first);
        let mut other_buffers = ScratchBuffers::default();
        assert_eq!(other_buffers.create("mock", "same"), first_id);
        assert_ne!(
            other_buffers.selected().unwrap().evaluation_source_id(),
            &first
        );
        buffers.discard_selected().unwrap();
        buffers.create("mock", "same");
        assert_ne!(buffers.selected().unwrap().evaluation_source_id(), &first);
    }

    #[test]
    fn source_identity_survives_save_and_reload_but_not_reopening() {
        let fixture = TempWorkspace::new("scratch-source-identity-reload");
        fixture.write_file("a", "old");
        let workspace = fixture.workspace();
        let mut buffers = buffers("new");
        let buffer = buffers.selected_mut().unwrap();
        let identity = buffer.evaluation_source_id().clone();
        buffer.promote(&workspace, "a").unwrap();
        buffer
            .apply_edit(TextEdit::insert(Position::new(0, 3), "!"))
            .unwrap();
        buffer.save(&workspace).unwrap();
        assert_eq!(buffer.evaluation_source_id(), &identity);
        buffer.reload(&workspace, false).unwrap();
        assert_eq!(buffer.evaluation_source_id(), &identity);
        assert_eq!(buffer.version(), 2);
        let (mut session, state) = session();
        buffer.evaluate(&mut session).unwrap();
        assert_eq!(
            state.lock().unwrap().requests,
            vec![(identity.clone(), RevisionId::from_version(2))]
        );
        buffers.close_selected(false).unwrap();
        buffers
            .open(&workspace, &LanguageRegistry::builtins(), "a")
            .unwrap();
        assert_ne!(
            buffers.selected().unwrap().evaluation_source_id(),
            &identity
        );
    }

    #[test]
    fn evaluation_is_literal_and_does_not_change_buffer() {
        let (mut session, state) = session();
        let fixture = TempWorkspace::new("scratch-evaluation");
        fixture.write_file("a", "old");
        let mut buffers = buffers(":scope reset\né🙂");
        let buffer = buffers.selected_mut().unwrap();
        buffer.promote(&fixture.workspace(), "a").unwrap();
        let revision = buffer.destination_revision().unwrap().to_owned();
        for _ in 0..2 {
            let result = buffer.evaluate(&mut session).unwrap();
            assert_eq!(result.output, vec![buffer.text()]);
        }
        assert_eq!(
            state.lock().unwrap().sources,
            vec![buffer.text(), buffer.text()]
        );
        assert_eq!(state.lock().unwrap().resets, 0);
        assert!(!buffer.is_dirty());
        assert_eq!(buffer.version(), 0);
        assert_eq!(buffer.destination_revision(), Some(revision.as_str()));
    }

    #[test]
    fn evaluation_rejects_mismatch_and_missing_session_without_dispatch() {
        let (mut session, state) = session();
        let mut buffers = ScratchBuffers::default();
        buffers.create("other", "source");
        assert!(matches!(
            buffers.selected().unwrap().evaluate(&mut session),
            Err(ScratchError::LanguageMismatch { .. })
        ));
        assert!(state.lock().unwrap().sources.is_empty());
        assert_eq!(session.language_id(), Some("mock"));
        assert!(matches!(
            buffers
                .selected()
                .unwrap()
                .evaluate(&mut InteractiveSession::new()),
            Err(ScratchError::LanguageMismatch { actual: None, .. })
        ));
    }

    #[test]
    fn reset_requirement_and_failed_reset_are_preserved() {
        for source in ["poison", "error"] {
            let (mut session, state) = session();
            let buffers = buffers(source);
            let buffer = buffers.selected().unwrap();
            if source == "poison" {
                assert!(matches!(
                    buffer.evaluate(&mut session).unwrap().status,
                    EvaluationStatus::Cancelled {
                        requires_reset: true
                    }
                ));
            } else {
                assert!(matches!(
                    buffer.evaluate(&mut session),
                    Err(ScratchError::Session(SessionError::ResetRequired))
                ));
            }
            assert!(matches!(
                buffer.evaluate(&mut session),
                Err(ScratchError::Session(SessionError::ResetRequired))
            ));
            assert_eq!(state.lock().unwrap().sources.len(), 1);
            state.lock().unwrap().fail_reset = true;
            assert!(session.reset().is_err());
            assert!(buffer.evaluate(&mut session).is_err());
            state.lock().unwrap().fail_reset = false;
            session.reset().unwrap();
            let fresh = super::tests::buffers("fresh");
            fresh.selected().unwrap().evaluate(&mut session).unwrap();
            assert_eq!(state.lock().unwrap().sources, vec![source, "fresh"]);
            assert!(buffer.is_dirty());
            assert_eq!(buffer.version(), 0);
        }
    }
}
