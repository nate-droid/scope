use crate::document::{Document, DocumentId};
use crate::language::{
    Diagnostic, LanguageAction, LanguageAnalysis, LanguageRegistry, LanguageService, Symbol,
};
use crate::path::ToScopePath;
use crate::workspace::{Workspace, WorkspaceEntry, WorkspaceError};

#[derive(Debug, Clone)]
pub struct App {
    workspace: Workspace,
    languages: LanguageRegistry,
    files: Vec<WorkspaceEntry>,
    selected: usize,
    open_document: Option<Document>,
    next_document_id: u64,
}

impl App {
    pub fn new(workspace: Workspace) -> Result<Self, WorkspaceError> {
        Self::builder(workspace).build()
    }

    pub fn builder(workspace: Workspace) -> AppBuilder {
        AppBuilder::new(workspace)
    }

    /// Create an isolated disk snapshot without touching the active buffer.
    pub fn proposal(
        &self,
        paths: &[crate::path::ScopePath],
        limits: crate::workspace::ProposalLimits,
    ) -> Result<crate::workspace::Proposal, crate::workspace::ProposalError> {
        crate::workspace::Proposal::new(&self.workspace, paths, limits)
    }

    /// Human-only acceptance. Refuse while any active buffer is dirty.
    pub fn accept_proposal(
        &mut self,
        proposal: &mut crate::workspace::Proposal,
    ) -> crate::workspace::Acceptance {
        if self.open_document.as_ref().is_some_and(Document::is_dirty) {
            return crate::workspace::Acceptance {
                applied: Vec::new(),
                error: Some("save or revert the dirty buffer before acceptance".into()),
            };
        }
        let result = proposal.accept();
        // Do not silently replace the user's open buffer, even when clean.
        result
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn languages(&self) -> &LanguageRegistry {
        &self.languages
    }

    pub fn files(&self) -> &[WorkspaceEntry] {
        &self.files
    }

    pub fn selected_index(&self) -> Option<usize> {
        (!self.files.is_empty()).then_some(self.selected)
    }

    pub fn selected_file(&self) -> Option<&WorkspaceEntry> {
        self.files.get(self.selected)
    }

    pub fn open_document(&self) -> Option<&Document> {
        self.open_document.as_ref()
    }

    pub fn open_document_mut(&mut self) -> Option<&mut Document> {
        self.open_document.as_mut()
    }

    fn allocate_document_id(&mut self) -> DocumentId {
        let id = DocumentId::new(self.next_document_id);
        self.next_document_id += 1;
        id
    }

    pub fn open_language(&self) -> Option<&'static dyn LanguageService> {
        self.open_document
            .as_ref()
            .and_then(|document| self.languages.language_for_document(document))
    }

    pub fn semantic_service_for_document(
        &self,
        document: &Document,
    ) -> &'static dyn LanguageService {
        self.languages.semantic_service_for_document(document)
    }

    /// Analyze diagnostics and outline together, without caching across calls.
    pub fn open_analysis(&self) -> Result<LanguageAnalysis, AppError> {
        let document = self
            .open_document
            .as_ref()
            .ok_or(AppError::NoOpenDocument)?;
        let language = self.semantic_service_for_document(document);

        Ok(language.analyze(document.as_language_document()))
    }

    pub fn open_diagnostics(&self) -> Result<Vec<Diagnostic>, AppError> {
        let document = self
            .open_document
            .as_ref()
            .ok_or(AppError::NoOpenDocument)?;
        let language = self.semantic_service_for_document(document);

        Ok(language.diagnostics(document.as_language_document()))
    }

    pub fn open_outline(&self) -> Result<Vec<Symbol>, AppError> {
        let document = self
            .open_document
            .as_ref()
            .ok_or(AppError::NoOpenDocument)?;
        let language = self.semantic_service_for_document(document);

        Ok(language.outline(document.as_language_document()))
    }

    pub fn open_actions(&self) -> Result<Vec<LanguageAction>, AppError> {
        let document = self
            .open_document
            .as_ref()
            .ok_or(AppError::NoOpenDocument)?;
        let language = self.semantic_service_for_document(document);

        Ok(language.actions(document.as_language_document()))
    }

    pub fn move_selection_up(&mut self) {
        if self.files.is_empty() {
            return;
        }

        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_selection_down(&mut self) {
        if self.files.is_empty() {
            return;
        }

        self.selected = (self.selected + 1).min(self.files.len() - 1);
    }

    pub fn open_selected(&mut self) -> Result<&Document, AppError> {
        let path = self.selected_file().ok_or(AppError::NoFiles)?.path.clone();

        self.open_path(path)
    }

    pub fn open_path(&mut self, path: impl ToScopePath) -> Result<&Document, AppError> {
        let file = self
            .workspace
            .load_file_revision(path)
            .map_err(AppError::Workspace)?;
        let document_id = self.allocate_document_id();
        self.open_document = Some(Document::from_revisioned_workspace_file(document_id, file));
        Ok(self
            .open_document
            .as_ref()
            .expect("document was just opened"))
    }

    pub fn save_open_document(&mut self) -> Result<(), AppError> {
        let document = self
            .open_document
            .as_ref()
            .ok_or(AppError::NoOpenDocument)?;
        let path = document.path().clone();
        let contents = document.text().to_owned();

        let expected_revision = document
            .disk_revision()
            .ok_or(AppError::MissingDiskRevision)?;
        let revision = self
            .workspace
            .save_file_checked(path, contents, expected_revision)?;
        self.open_document
            .as_mut()
            .expect("document was checked above")
            .mark_saved_revision(revision);
        Ok(())
    }

    pub fn revert_open_document(&mut self) -> Result<(), AppError> {
        let document = self
            .open_document
            .as_mut()
            .ok_or(AppError::NoOpenDocument)?;
        document.revert();
        Ok(())
    }

    pub fn refresh_files(&mut self) -> Result<(), WorkspaceError> {
        self.files = self.workspace.list_files()?;
        if self.files.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.files.len() - 1);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AppBuilder {
    workspace: Workspace,
    languages: LanguageRegistry,
}

impl AppBuilder {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            languages: LanguageRegistry::builtins(),
        }
    }

    pub fn language(mut self, language: &'static dyn LanguageService) -> Self {
        self.languages = self.languages.with_language(language);
        self
    }

    pub fn languages(
        mut self,
        languages: impl IntoIterator<Item = &'static dyn LanguageService>,
    ) -> Self {
        self.languages = self.languages.with_languages(languages);
        self
    }

    pub fn language_registry(mut self, languages: LanguageRegistry) -> Self {
        self.languages = languages;
        self
    }

    pub fn build(self) -> Result<App, WorkspaceError> {
        let files = self.workspace.list_files()?;

        Ok(App {
            workspace: self.workspace,
            languages: self.languages,
            files,
            selected: 0,
            open_document: None,
            next_document_id: 1,
        })
    }
}

#[derive(Debug)]
pub enum AppError {
    NoFiles,
    NoOpenDocument,
    MissingDiskRevision,
    Workspace(WorkspaceError),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFiles => write!(formatter, "workspace has no files"),
            Self::NoOpenDocument => write!(formatter, "no document is open"),
            Self::MissingDiskRevision => write!(formatter, "open document has no disk revision"),
            Self::Workspace(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoFiles | Self::NoOpenDocument | Self::MissingDiskRevision => None,
            Self::Workspace(source) => Some(source),
        }
    }
}

impl From<WorkspaceError> for AppError {
    fn from(error: WorkspaceError) -> Self {
        Self::Workspace(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::ScopePath;
    use crate::test_support::TempWorkspace;

    #[test]
    fn starts_with_workspace_files_and_no_open_document() {
        let root = TempWorkspace::new("starts_with_workspace_files_and_no_open_document");
        root.create_dir("src");
        root.write_file("README.md", "hello");
        root.write_file("src/main.rs", "fn main() {}");

        let app = App::new(root.workspace()).unwrap();

        assert_eq!(app.files().len(), 2);
        assert_eq!(app.selected_index(), Some(0));
        assert_eq!(
            app.selected_file().unwrap().path,
            ScopePath::new("README.md").unwrap()
        );
        assert!(app.open_document().is_none());
    }

    #[test]
    fn moves_selection_within_file_bounds() {
        let root = TempWorkspace::new("moves_selection_within_file_bounds");
        root.write_file("a.txt", "a");
        root.write_file("b.txt", "b");

        let mut app = App::new(root.workspace()).unwrap();

        app.move_selection_down();
        app.move_selection_down();
        assert_eq!(
            app.selected_file().unwrap().path,
            ScopePath::new("b.txt").unwrap()
        );

        app.move_selection_up();
        app.move_selection_up();
        assert_eq!(
            app.selected_file().unwrap().path,
            ScopePath::new("a.txt").unwrap()
        );
    }

    #[test]
    fn opens_selected_file_as_document() {
        let root = TempWorkspace::new("opens_selected_file_as_document");
        root.write_file("README.md", "hello");

        let mut app = App::new(root.workspace()).unwrap();
        let document = app.open_selected().unwrap();

        assert_eq!(document.id(), DocumentId::new(1));
        assert_eq!(document.path(), &ScopePath::new("README.md").unwrap());
        assert_eq!(document.text(), "hello");
        assert!(!document.is_dirty());
    }

    #[test]
    fn opens_file_by_workspace_relative_path() {
        let root = TempWorkspace::new("opens_file_by_workspace_relative_path");
        root.write_file("src/main.rs", "fn main() {}");

        let mut app = App::new(root.workspace()).unwrap();
        let document = app.open_path("src/main.rs").unwrap();

        assert_eq!(document.id(), DocumentId::new(1));
        assert_eq!(document.path(), &ScopePath::new("src/main.rs").unwrap());
        assert_eq!(document.text(), "fn main() {}");
    }

    #[test]
    fn allocates_a_new_identity_each_time_a_document_opens() {
        let root = TempWorkspace::new("allocates_a_new_identity_each_time_a_document_opens");
        root.write_file("README.md", "hello");

        let mut app = App::new(root.workspace()).unwrap();
        let first_id = app.open_path("README.md").unwrap().id();
        let second_id = app.open_path("README.md").unwrap().id();

        assert_eq!(first_id, DocumentId::new(1));
        assert_eq!(second_id, DocumentId::new(2));
        assert_ne!(first_id, second_id);
    }

    #[test]
    fn exposes_language_features_for_open_document() {
        let root = TempWorkspace::new("exposes_language_features_for_open_document");
        root.write_file("main.scope", "fn main() {\n  let answer = 42\n}\n");

        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("main.scope").unwrap();

        assert_eq!(app.open_language().unwrap().language_id(), "scope.simple");
        assert_eq!(app.open_diagnostics().unwrap(), Vec::new());
        assert_eq!(app.open_outline().unwrap().len(), 2);
        let analysis = app.open_analysis().unwrap();
        assert_eq!(analysis.diagnostics, app.open_diagnostics().unwrap());
        assert_eq!(analysis.symbols, app.open_outline().unwrap());
        assert_eq!(app.open_actions().unwrap(), Vec::new());
    }

    #[test]
    fn exposes_neutral_semantic_features_without_detected_language() {
        let root =
            TempWorkspace::new("exposes_neutral_semantic_features_without_detected_language");
        root.write_file(
            "README.md",
            "<<<<<<< ours\nhello\n=======\nworld\n>>>>>>> theirs\n",
        );

        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("README.md").unwrap();

        assert!(app.open_language().is_none());
        assert_eq!(app.open_outline().unwrap(), Vec::new());
        assert_eq!(app.open_diagnostics().unwrap().len(), 3);
        let analysis = app.open_analysis().unwrap();
        assert_eq!(analysis.diagnostics, app.open_diagnostics().unwrap());
        assert_eq!(analysis.symbols, app.open_outline().unwrap());
        assert_eq!(app.open_actions().unwrap(), Vec::new());
    }

    #[test]
    fn external_write_conflict_preserves_document() {
        let root = TempWorkspace::new("external_write_conflict_preserves_document");
        root.write_file("README.md", "hello");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("README.md").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(crate::text::TextEdit::insert(
                crate::text::Position::new(0, 5),
                " Scope",
            ))
            .unwrap();
        let before = app.open_document().unwrap().clone();
        root.write_file("README.md", "external");
        let current = app.workspace().load_file_revision("README.md").unwrap();
        match app.save_open_document().unwrap_err() {
            AppError::Workspace(WorkspaceError::Conflict {
                path,
                current_revision,
            }) => {
                assert_eq!(path, current.path);
                assert_eq!(current_revision, current.revision);
            }
            error => panic!("expected conflict, got {error:?}"),
        }
        assert_eq!(app.open_document().unwrap(), &before);
        assert!(app.open_document().unwrap().is_dirty());
        assert_eq!(root.read_file("README.md"), "external");
    }

    #[test]
    fn successive_saves_update_disk_revision() {
        let root = TempWorkspace::new("successive_saves_update_disk_revision");
        root.write_file("README.md", "hello");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("README.md").unwrap();
        for suffix in [" Scope", "!"] {
            let previous = app
                .open_document()
                .unwrap()
                .disk_revision()
                .unwrap()
                .to_owned();
            let column = app.open_document().unwrap().text().len();
            app.open_document_mut()
                .unwrap()
                .apply_edit(crate::text::TextEdit::insert(
                    crate::text::Position::new(0, column),
                    suffix,
                ))
                .unwrap();
            app.save_open_document().unwrap();
            let document = app.open_document().unwrap();
            let disk = app.workspace().load_file_revision("README.md").unwrap();
            assert_ne!(document.disk_revision(), Some(previous.as_str()));
            assert_eq!(document.disk_revision(), Some(disk.revision.as_str()));
            assert_eq!(document.text(), disk.contents);
            assert_eq!(document.saved_version(), document.version());
            assert!(!document.is_dirty());
        }
        assert_eq!(root.read_file("README.md"), "hello Scope!");
    }

    #[test]
    fn refuses_save_without_disk_revision() {
        let root = TempWorkspace::new("refuses_save_without_disk_revision");
        root.write_file("README.md", "hello");
        let mut app = App::new(root.workspace()).unwrap();
        let mut document = Document::new(DocumentId::new(1), "README.md", "hello");
        document
            .apply_edit(crate::text::TextEdit::insert(
                crate::text::Position::new(0, 5),
                "!",
            ))
            .unwrap();
        app.open_document = Some(document.clone());
        assert!(matches!(
            app.save_open_document(),
            Err(AppError::MissingDiskRevision)
        ));
        assert_eq!(app.open_document().unwrap(), &document);
        assert_eq!(root.read_file("README.md"), "hello");
    }

    #[test]
    fn saves_open_document_to_workspace() {
        let root = TempWorkspace::new("saves_open_document_to_workspace");
        root.write_file("README.md", "hello");

        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("README.md").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(crate::text::TextEdit::insert(
                crate::text::Position::new(0, 5),
                " Scope",
            ))
            .unwrap();

        assert!(app.open_document().unwrap().is_dirty());
        app.save_open_document().unwrap();

        let document = app.open_document().unwrap();
        assert_eq!(root.read_file("README.md"), "hello Scope");
        assert_eq!(document.text(), "hello Scope");
        assert!(!document.is_dirty());
    }
}
