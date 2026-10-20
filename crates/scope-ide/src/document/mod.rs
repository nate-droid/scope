pub mod session;
pub use session::editing::{EditAuthorization, EditDecision, EditError, EditProposal, EditReview};
pub use session::{DocumentError, DocumentSession, DocumentSnapshot, Revision};

use scope_language::{LanguageDocument, LanguagePath, SourceId};

use crate::path::{ScopePath, ToScopePath};
use crate::text::{TextBuffer, TextBufferError, TextEdit};
use crate::workspace::{RevisionedWorkspaceFile, WorkspaceFile};

/// Stable identity for an open text document.
///
/// `DocumentId` is intentionally independent of the document's path so callers
/// can continue referring to the same open document if it is renamed or moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentId(u64);

impl DocumentId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for DocumentId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// An open text document in the Scope environment.
///
/// `Document` owns editable text, remembers the path it came from, and tracks
/// whether the in-memory text differs from the last saved snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    id: DocumentId,
    source_id: SourceId,
    path: ScopePath,
    buffer: TextBuffer,
    saved_text: String,
    version: u64,
    saved_version: u64,
    disk_revision: Option<String>,
}

impl Document {
    pub fn new(id: DocumentId, path: impl ToScopePath, contents: impl Into<String>) -> Self {
        let contents = contents.into();

        Self {
            id,
            source_id: SourceId::fresh(),
            path: path
                .to_scope_path()
                .expect("document paths must be valid Scope capability paths"),
            buffer: TextBuffer::new(contents.clone()),
            saved_text: contents,
            version: 0,
            saved_version: 0,
            disk_revision: None,
        }
    }

    pub fn from_workspace_file(id: DocumentId, file: WorkspaceFile) -> Self {
        Self::new(id, file.path, file.contents)
    }

    pub fn from_revisioned_workspace_file(id: DocumentId, file: RevisionedWorkspaceFile) -> Self {
        let mut document = Self::new(id, file.path, file.contents);
        document.disk_revision = Some(file.revision);
        document
    }

    pub fn disk_revision(&self) -> Option<&str> {
        self.disk_revision.as_deref()
    }

    pub fn mark_saved_revision(&mut self, revision: impl Into<String>) {
        self.mark_saved();
        self.disk_revision = Some(revision.into());
    }

    pub fn id(&self) -> DocumentId {
        self.id
    }

    /// Evaluation identity belongs to the logical document, not its path or revision.
    pub fn evaluation_source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn path(&self) -> &ScopePath {
        &self.path
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn saved_version(&self) -> u64 {
        self.saved_version
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.as_str() != self.saved_text
    }

    pub fn text(&self) -> &str {
        self.buffer.as_str()
    }

    pub fn as_language_document(&self) -> LanguageDocument<'_> {
        LanguageDocument::new(
            LanguagePath::new(self.path().to_string_lossy()),
            self.text(),
            self.version(),
        )
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    pub fn into_buffer(self) -> TextBuffer {
        self.buffer
    }

    pub fn apply_edit(&mut self, edit: TextEdit) -> Result<(), TextBufferError> {
        self.buffer.apply_edit(edit)?;
        self.version += 1;
        Ok(())
    }

    pub fn apply_edits(
        &mut self,
        edits: impl IntoIterator<Item = TextEdit>,
    ) -> Result<(), TextBufferError> {
        self.buffer.apply_edits(edits)?;
        self.version += 1;
        Ok(())
    }

    pub fn revert(&mut self) {
        self.buffer = TextBuffer::new(self.saved_text.clone());
        self.version += 1;
    }

    pub fn mark_saved(&mut self) {
        self.saved_text = self.buffer.as_str().to_owned();
        self.saved_version = self.version;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{Position, TextRange};

    #[test]
    fn source_identity_survives_edits_save_revert_and_clone() {
        let mut document = Document::new(DocumentId::new(1), "test.scope", "hello");
        let source_id = document.evaluation_source_id().clone();
        assert_eq!(document.clone().evaluation_source_id(), &source_id);
        document
            .apply_edit(TextEdit::insert(Position::new(0, 5), "!"))
            .unwrap();
        document.mark_saved();
        document.revert();
        assert_eq!(document.evaluation_source_id(), &source_id);
        assert_eq!(document.version(), 2);
        let reopened = Document::new(document.id(), "test.scope", document.text());
        assert_ne!(reopened.evaluation_source_id(), &source_id);
    }

    #[test]
    fn document_id_wraps_raw_identity() {
        let id = DocumentId::new(42);

        assert_eq!(id.as_u64(), 42);
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn starts_clean_from_initial_contents() {
        let document = Document::new(DocumentId::new(1), "README.md", "hello");

        assert_eq!(document.id(), DocumentId::new(1));
        assert_eq!(document.path(), &ScopePath::new("README.md").unwrap());
        assert_eq!(document.text(), "hello");
        assert_eq!(document.version(), 0);
        assert_eq!(document.saved_version(), 0);
        assert!(!document.is_dirty());
    }

    #[test]
    fn edit_updates_text_version_and_dirty_state() {
        let mut document = Document::new(DocumentId::new(1), "README.md", "hello world");

        document
            .apply_edit(TextEdit::replace(
                TextRange::new(Position::new(0, 6), Position::new(0, 11)),
                "Scope",
            ))
            .unwrap();

        assert_eq!(document.text(), "hello Scope");
        assert_eq!(document.version(), 1);
        assert_eq!(document.saved_version(), 0);
        assert!(document.is_dirty());
    }

    #[test]
    fn mark_saved_accepts_current_text_as_clean() {
        let mut document = Document::new(DocumentId::new(1), "README.md", "hello");

        document
            .apply_edit(TextEdit::insert(Position::new(0, 5), " Scope"))
            .unwrap();
        document.mark_saved();

        assert_eq!(document.text(), "hello Scope");
        assert_eq!(document.version(), 1);
        assert_eq!(document.saved_version(), 1);
        assert!(!document.is_dirty());
    }

    #[test]
    fn revert_restores_last_saved_text() {
        let mut document = Document::new(DocumentId::new(1), "README.md", "hello");

        document
            .apply_edit(TextEdit::insert(Position::new(0, 5), " Scope"))
            .unwrap();
        document.mark_saved();
        document
            .apply_edit(TextEdit::insert(Position::new(0, 11), "!"))
            .unwrap();

        document.revert();

        assert_eq!(document.text(), "hello Scope");
        assert_eq!(document.version(), 3);
        assert_eq!(document.saved_version(), 1);
        assert!(!document.is_dirty());
    }

    #[test]
    fn can_open_from_workspace_file() {
        let document = Document::from_workspace_file(
            DocumentId::new(7),
            WorkspaceFile {
                path: ScopePath::new("src/main.rs").unwrap(),
                contents: "fn main() {}".to_owned(),
            },
        );

        assert_eq!(document.id(), DocumentId::new(7));
        assert_eq!(document.path(), &ScopePath::new("src/main.rs").unwrap());
        assert_eq!(document.text(), "fn main() {}");
        assert!(!document.is_dirty());
    }
}
