use crate::document::{Document, DocumentId};
use crate::language::{
    Diagnostic, LanguageAction, LanguageAnalysis, LanguageRegistry, LanguageService, Symbol,
};
use crate::path::ToScopePath;
#[cfg(feature = "experimental-sprout-closure")]
use crate::registered_document_closure::{
    InertProvider, RegisteredDocuments, TrustedClosureProvider,
};
#[cfg(not(feature = "experimental-sprout-closure"))]
#[doc(hidden)]
pub trait TrustedClosureProvider {}
#[cfg(not(feature = "experimental-sprout-closure"))]
impl TrustedClosureProvider for () {}

#[cfg(feature = "experimental-sprout-closure")]
pub type App<P = InertProvider> = AppState<P>;
#[cfg(not(feature = "experimental-sprout-closure"))]
pub type App = AppState<()>;
use crate::workspace::{Workspace, WorkspaceEntry, WorkspaceError};

/// App's experimental registered session has exclusive document ownership.
/// With `experimental-sprout-closure`, App is deliberately not Clone: duplicating
/// an owner would duplicate live evaluation identities and authorization state.
#[cfg_attr(not(feature = "experimental-sprout-closure"), derive(Clone))]
pub struct AppState<P> {
    #[cfg(not(feature = "experimental-sprout-closure"))]
    provider: std::marker::PhantomData<P>,
    workspace: Workspace,
    languages: LanguageRegistry,
    files: Vec<WorkspaceEntry>,
    selected: usize,
    open_document: Option<Document>,
    next_document_id: u64,
    #[cfg(feature = "experimental-sprout-closure")]
    registered_session: Option<RegisteredSession<P>>,
}

#[cfg(feature = "experimental-sprout-closure")]
struct RegisteredSession<P> {
    documents: RegisteredDocuments<P>,
    selected: Option<String>,
}

impl<P: TrustedClosureProvider> std::fmt::Debug for AppState<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("workspace", &self.workspace)
            .field("open_document", &self.open_document())
            .finish_non_exhaustive()
    }
}

impl App {
    pub fn new(workspace: Workspace) -> Result<Self, WorkspaceError> {
        Self::builder(workspace).build()
    }

    pub fn builder(workspace: Workspace) -> AppBuilder {
        AppBuilder::new(workspace)
    }
}

impl<P: TrustedClosureProvider> AppState<P> {
    /// Create an isolated disk snapshot without touching the active buffer.
    pub fn proposal(
        &self,
        paths: &[crate::path::ScopePath],
        limits: crate::workspace::ProposalLimits,
    ) -> Result<crate::workspace::Proposal, crate::workspace::ProposalError> {
        crate::workspace::Proposal::new(&self.workspace, paths, limits)
    }

    /// Whether any active or parked enrolled document has unsaved edits.
    pub fn has_unsaved_documents(&self) -> bool {
        if self.open_document.as_ref().is_some_and(Document::is_dirty) {
            return true;
        }
        #[cfg(feature = "experimental-sprout-closure")]
        if self.registered_documents().is_some_and(|documents| {
            documents
                .documents()
                .any(|(_, document)| document.is_dirty())
        }) {
            return true;
        }
        false
    }

    /// Human-only acceptance. Refuse while any owned buffer is dirty.
    pub fn accept_proposal(
        &mut self,
        proposal: &mut crate::workspace::Proposal,
    ) -> crate::workspace::Acceptance {
        if self.has_unsaved_documents() {
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
        #[cfg(feature = "experimental-sprout-closure")]
        match &self.registered_session {
            Some(session) if session.selected.is_some() => session
                .documents
                .document(session.selected.as_deref().unwrap()),
            _ => self.open_document.as_ref(),
        }
        #[cfg(not(feature = "experimental-sprout-closure"))]
        self.open_document.as_ref()
    }

    pub fn open_document_mut(&mut self) -> Option<&mut Document> {
        #[cfg(feature = "experimental-sprout-closure")]
        match &mut self.registered_session {
            Some(session) if session.selected.is_some() => session
                .documents
                .document_mut(session.selected.as_deref().unwrap()),
            _ => self.open_document.as_mut(),
        }
        #[cfg(not(feature = "experimental-sprout-closure"))]
        self.open_document.as_mut()
    }

    /// A configured owner must be empty. Failure returns the unchanged owner.
    #[cfg(feature = "experimental-sprout-closure")]
    pub fn enable_registered_session(
        &mut self,
        documents: RegisteredDocuments<P>,
    ) -> Result<(), Box<RegisteredDocuments<P>>> {
        if self.registered_session.is_some() || !documents.is_empty() {
            return Err(Box::new(documents));
        }
        self.registered_session = Some(RegisteredSession {
            documents,
            selected: None,
        });
        Ok(())
    }

    #[cfg(feature = "experimental-sprout-closure")]
    pub fn registered_documents(&self) -> Option<&RegisteredDocuments<P>> {
        self.registered_session
            .as_ref()
            .map(|session| &session.documents)
    }

    /// Capture against the authoritative session. `None` means it is disabled.
    #[cfg(feature = "experimental-sprout-closure")]
    pub fn capture_registered_execution(
        &self,
        root: &str,
        operation: &str,
    ) -> Option<
        Result<
            crate::registered_document_closure::RegisteredClosureBinding,
            crate::closure_inspection::ClosureInspectionError,
        >,
    > {
        self.registered_documents()
            .map(|documents| documents.capture(root, operation))
    }

    /// Parameterized capture against the authoritative session; disabled is None.
    #[cfg(feature = "experimental-sprout-closure")]
    pub fn capture_registered_invocation(
        &self,
        root: &str,
        operation: &str,
        arguments: serde_json::Value,
        contract: crate::registered_document_closure::InvocationContract,
    ) -> Option<
        Result<
            crate::registered_document_closure::RegisteredClosureBinding,
            crate::registered_document_closure::RegisteredCaptureRejection,
        >,
    > {
        self.registered_documents()
            .map(|documents| documents.capture_invocation(root, operation, arguments, contract))
    }

    /// Consume a selection using the registry's freshness and policy checks.
    /// `None` means the session is disabled; no authorization is requested then.
    #[cfg(feature = "experimental-sprout-closure")]
    pub fn invoke_registered_execution(
        &mut self,
        selection: crate::registered_document_closure::RegisteredClosureBinding,
        policy: &mut impl crate::tool::semantic::HostSemanticPolicy,
    ) -> Option<
        Result<
            crate::registered_document_closure::RegisteredClosureResult<P::Output>,
            crate::closure_inspection::ClosureRejection,
        >,
    > {
        self.registered_documents_mut()
            .map(|documents| documents.invoke(selection, policy))
    }

    /// Capture and invoke through the authoritative owner. Invocation consumes its
    /// binding and retains the registry's freshness and authorization checks.
    #[cfg(feature = "experimental-sprout-closure")]
    pub fn registered_documents_mut(&mut self) -> Option<&mut RegisteredDocuments<P>> {
        self.registered_session
            .as_mut()
            .map(|session| &mut session.documents)
    }

    #[cfg(feature = "experimental-sprout-closure")]
    pub fn selected_registered_module(&self) -> Option<&str> {
        self.registered_session.as_ref()?.selected.as_deref()
    }

    #[cfg(feature = "experimental-sprout-closure")]
    pub fn set_registered_policy(&mut self, policy: serde_json::Value) -> Result<(), AppError> {
        self.registered_session
            .as_mut()
            .ok_or(AppError::RegisteredSession("session is disabled"))?
            .documents
            .set_policy(policy);
        Ok(())
    }

    #[cfg(feature = "experimental-sprout-closure")]
    pub fn disable_registered_session(
        &mut self,
    ) -> Result<Option<RegisteredDocuments<P>>, AppError> {
        if self
            .registered_documents()
            .is_some_and(|documents| !documents.is_empty())
        {
            return Err(AppError::RegisteredSession(
                "remove enrolled documents before disabling",
            ));
        }
        Ok(self
            .registered_session
            .take()
            .map(|session| session.documents))
    }

    #[cfg(feature = "experimental-sprout-closure")]
    pub fn enroll_open_document(
        &mut self,
        module: String,
        language: String,
    ) -> Result<(), AppError> {
        let session = self
            .registered_session
            .as_mut()
            .ok_or(AppError::RegisteredSession("session is disabled"))?;
        let document = self.open_document.take().ok_or(AppError::NoOpenDocument)?;
        match session
            .documents
            .try_register_with_language(module.clone(), document, language)
        {
            Ok(()) => {
                session.selected = Some(module);
                Ok(())
            }
            Err(rejected) => {
                let (error, document) = *rejected;
                self.open_document = Some(document);
                Err(AppError::Registration(error))
            }
        }
    }

    #[cfg(feature = "experimental-sprout-closure")]
    pub fn select_registered_document(&mut self, module: &str) -> Result<&Document, AppError> {
        let session = self
            .registered_session
            .as_mut()
            .ok_or(AppError::RegisteredSession("session is disabled"))?;
        if session.documents.document(module).is_none() {
            return Err(AppError::RegisteredSession("unknown enrolled module"));
        }
        if self.open_document.as_ref().is_some_and(Document::is_dirty) {
            return Err(AppError::RegisteredSession(
                "save or close the dirty unenrolled document first",
            ));
        }
        self.open_document = None;
        session.selected = Some(module.to_owned());
        Ok(self.open_document().expect("selected enrolled document"))
    }

    /// Transfers ownership, including unsaved edits, to the caller.
    #[cfg(feature = "experimental-sprout-closure")]
    pub fn remove_registered_document(&mut self, module: &str) -> Result<Document, AppError> {
        let session = self
            .registered_session
            .as_mut()
            .ok_or(AppError::RegisteredSession("session is disabled"))?;
        let document = session
            .documents
            .remove(module)
            .ok_or(AppError::RegisteredSession("unknown enrolled module"))?;
        if session.selected.as_deref() == Some(module) {
            session.selected = None;
        }
        Ok(document)
    }

    pub fn close_open_document(&mut self) -> Result<(), AppError> {
        if self.open_document().is_some_and(Document::is_dirty) {
            return Err(AppError::RegisteredSession(
                "save or revert the dirty document before closing",
            ));
        }
        #[cfg(feature = "experimental-sprout-closure")]
        if let Some(module) = self.selected_registered_module().map(str::to_owned) {
            self.remove_registered_document(&module)?;
        }
        self.open_document = None;
        Ok(())
    }

    fn allocate_document_id(&mut self) -> DocumentId {
        let id = DocumentId::new(self.next_document_id);
        self.next_document_id += 1;
        id
    }

    pub fn open_language(&self) -> Option<&'static dyn LanguageService> {
        self.open_document()
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
        let document = self.open_document().ok_or(AppError::NoOpenDocument)?;
        let language = self.semantic_service_for_document(document);

        Ok(language.analyze(document.as_language_document()))
    }

    pub fn open_diagnostics(&self) -> Result<Vec<Diagnostic>, AppError> {
        let document = self.open_document().ok_or(AppError::NoOpenDocument)?;
        let language = self.semantic_service_for_document(document);

        Ok(language.diagnostics(document.as_language_document()))
    }

    pub fn open_outline(&self) -> Result<Vec<Symbol>, AppError> {
        let document = self.open_document().ok_or(AppError::NoOpenDocument)?;
        let language = self.semantic_service_for_document(document);

        Ok(language.outline(document.as_language_document()))
    }

    pub fn open_actions(&self) -> Result<Vec<LanguageAction>, AppError> {
        let document = self.open_document().ok_or(AppError::NoOpenDocument)?;
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
        #[cfg(feature = "experimental-sprout-closure")]
        if let Some(session) = &self.registered_session {
            if session
                .documents
                .documents()
                .any(|(_, document)| document.path() == &file.path)
            {
                return Err(AppError::RegisteredSession("path is already enrolled"));
            }
            if self.open_document.as_ref().is_some_and(Document::is_dirty) {
                return Err(AppError::RegisteredSession(
                    "save or close the dirty unenrolled document first",
                ));
            }
        }
        let document_id = self.allocate_document_id();
        #[cfg(feature = "experimental-sprout-closure")]
        if let Some(session) = &mut self.registered_session {
            session.selected = None;
        }
        self.open_document = Some(Document::from_revisioned_workspace_file(document_id, file));
        Ok(self.open_document().expect("document was just opened"))
    }

    pub fn save_open_document(&mut self) -> Result<(), AppError> {
        let document = self.open_document().ok_or(AppError::NoOpenDocument)?;
        let path = document.path().clone();
        let contents = document.text().to_owned();

        let expected_revision = document
            .disk_revision()
            .ok_or(AppError::MissingDiskRevision)?;
        let revision = self
            .workspace
            .save_file_checked(path, contents, expected_revision)?;
        self.open_document_mut()
            .expect("document was checked above")
            .mark_saved_revision(revision);
        Ok(())
    }

    pub fn revert_open_document(&mut self) -> Result<(), AppError> {
        let document = self.open_document_mut().ok_or(AppError::NoOpenDocument)?;
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

#[cfg(feature = "experimental-sprout-closure")]
impl App {
    /// Attach a prepared implementation while preserving the existing editing
    /// session. Failure returns the original App; `None` means no session exists.
    pub fn try_with_prepared_provider<T: Send + Sync + 'static, O>(
        self,
        descriptor: crate::registered_document_closure::PreparedImplementationDescriptor,
        prepared: T,
        dispatch: fn(
            &T,
            crate::registered_document_closure::ProviderRequest<'_>,
        ) -> Result<O, crate::registered_document_closure::ProviderFailure>,
    ) -> Result<
        App<crate::registered_document_closure::PreparedProvider<T, O>>,
        (
            Option<crate::closure_inspection::ClosureInspectionError>,
            Self,
        ),
    > {
        let Self {
            workspace,
            languages,
            files,
            selected,
            open_document,
            next_document_id,
            registered_session,
        } = self;
        let Some(session) = registered_session else {
            return Err((
                None,
                Self {
                    workspace,
                    languages,
                    files,
                    selected,
                    open_document,
                    next_document_id,
                    registered_session: None,
                },
            ));
        };
        match RegisteredDocuments::try_with_prepared_provider(
            session.documents,
            descriptor,
            prepared,
            dispatch,
        ) {
            Ok(documents) => Ok(AppState {
                workspace,
                languages,
                files,
                selected,
                open_document,
                next_document_id,
                registered_session: Some(RegisteredSession {
                    documents,
                    selected: session.selected,
                }),
            }),
            Err((error, documents)) => Err((
                Some(error),
                Self {
                    workspace,
                    languages,
                    files,
                    selected,
                    open_document,
                    next_document_id,
                    registered_session: Some(RegisteredSession {
                        documents,
                        selected: session.selected,
                    }),
                },
            )),
        }
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

        Ok(AppState {
            #[cfg(not(feature = "experimental-sprout-closure"))]
            provider: std::marker::PhantomData,
            workspace: self.workspace,
            languages: self.languages,
            files,
            selected: 0,
            open_document: None,
            next_document_id: 1,
            #[cfg(feature = "experimental-sprout-closure")]
            registered_session: None,
        })
    }
}

#[derive(Debug)]
pub enum AppError {
    NoFiles,
    NoOpenDocument,
    MissingDiskRevision,
    RegisteredSession(&'static str),
    #[cfg(feature = "experimental-sprout-closure")]
    Registration(crate::closure_inspection::ClosureInspectionError),
    Workspace(WorkspaceError),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFiles => write!(formatter, "workspace has no files"),
            Self::NoOpenDocument => write!(formatter, "no document is open"),
            Self::MissingDiskRevision => write!(formatter, "open document has no disk revision"),
            Self::RegisteredSession(message) => formatter.write_str(message),
            #[cfg(feature = "experimental-sprout-closure")]
            Self::Registration(error) => write!(formatter, "registration failed: {error:?}"),
            Self::Workspace(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoFiles
            | Self::NoOpenDocument
            | Self::MissingDiskRevision
            | Self::RegisteredSession(_) => None,
            #[cfg(feature = "experimental-sprout-closure")]
            Self::Registration(_) => None,
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

    #[cfg(feature = "experimental-sprout-closure")]
    #[test]
    fn prepared_transfer_preserves_live_session_and_recovers_invalid_descriptor() {
        use crate::registered_document_closure::*;
        let root = TempWorkspace::new("prepared-app-transfer");
        root.write_file("main.sprout", "original");
        let mut app = session_app(&root);
        app.open_path("main.sprout").unwrap();
        app.enroll_open_document("main".into(), "sprout".into())
            .unwrap();
        let id = app.open_document().unwrap().evaluation_source_id().clone();
        let selected = app.selected_index();
        let mut descriptor = PreparedImplementationDescriptor {
            sha256: "invalid".into(),
            abi: "test".into(),
            execution_policy: serde_json::json!({}),
        };
        let (_, app) = app
            .try_with_prepared_provider(descriptor.clone(), (), |_, _| Ok(42u32))
            .unwrap_err();
        assert_eq!(app.open_document().unwrap().evaluation_source_id(), &id);
        assert_eq!(app.selected_registered_module(), Some("main"));
        descriptor.sha256 = "ab".repeat(32);
        let mut app = app
            .try_with_prepared_provider(descriptor, (), |_, _| Ok(42u32))
            .unwrap();
        assert_eq!(app.open_document().unwrap().evaluation_source_id(), &id);
        assert_eq!(app.selected_index(), selected);
        assert_eq!(app.selected_registered_module(), Some("main"));
        app.open_document_mut()
            .unwrap()
            .apply_edit(crate::text::TextEdit::insert(
                crate::text::Position::new(0, 0),
                "edited",
            ))
            .unwrap();
        assert!(app.has_unsaved_documents());
        assert!(app.disable_registered_session().is_err());
        let document = app.remove_registered_document("main").unwrap();
        assert_eq!(document.text(), "editedoriginal");
        assert_eq!(document.evaluation_source_id(), &id);
        assert!(app.disable_registered_session().unwrap().is_some());
    }

    #[cfg(feature = "experimental-sprout-closure")]
    fn session_app(root: &TempWorkspace) -> App {
        use crate::registered_document_closure::*;
        struct NoDiscovery;
        impl Discovery for NoDiscovery {
            fn capture(
                &self,
                _: &str,
                _: Vec<RegisteredSource>,
                _: Limits,
            ) -> Result<DiscoveredClosure, DiscoveryError> {
                Err(DiscoveryError {
                    phase: DiscoveryPhase::Acquisition,
                    category: DiscoveryCategory::MissingSource,
                })
            }
        }
        let mut app = App::new(root.workspace()).unwrap();
        assert!(
            app.enable_registered_session(RegisteredDocuments::new(
                NoDiscovery,
                TrustedLoadedArtifact::default(),
                vec![],
                serde_json::Value::Null,
                Limits::default(),
                10,
            ))
            .is_ok()
        );
        app
    }

    #[cfg(feature = "experimental-sprout-closure")]
    #[test]
    fn registered_session_identity_revision_save_and_close_lifecycle() {
        let root = TempWorkspace::new("registered_session_lifecycle");
        root.write_file("a.sprout", "same");
        root.write_file("b.sprout", "same");
        let mut app = session_app(&root);
        app.open_path("a.sprout").unwrap();
        let identity = app.open_document().unwrap().evaluation_source_id().clone();
        app.enroll_open_document("a".into(), "sprout".into())
            .unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(crate::text::TextEdit::insert(
                crate::text::Position::new(0, 4),
                "!",
            ))
            .unwrap();
        let version = app.open_document().unwrap().version();
        assert!(app.close_open_document().is_err());
        app.open_path("b.sprout").unwrap();
        assert!(app.open_path("a.sprout").is_err());
        assert!(app.open_path("missing").is_err());
        app.enroll_open_document("b".into(), "sprout".into())
            .unwrap();
        app.select_registered_document("a").unwrap();
        assert_eq!(
            app.open_document().unwrap().evaluation_source_id(),
            &identity
        );
        assert_eq!(app.open_document().unwrap().version(), version);
        assert_eq!(app.open_document().unwrap().text(), "same!");
        app.save_open_document().unwrap();
        assert_eq!(root.read_file("a.sprout"), "same!");
        assert_eq!(app.open_document().unwrap().version(), version);
        assert!(app.disable_registered_session().is_err());
        app.close_open_document().unwrap();
        assert!(app.registered_documents().unwrap().document("a").is_none());
        let b = app.remove_registered_document("b").unwrap();
        assert_eq!(b.text(), "same");
        assert!(app.disable_registered_session().unwrap().is_some());
    }

    #[cfg(feature = "experimental-sprout-closure")]
    #[test]
    fn registered_session_failed_enrollment_and_dirty_selection_preserve_document() {
        let root = TempWorkspace::new("registered_session_failures");
        root.write_file("a.sprout", "same");
        root.write_file("b.sprout", "same");
        let mut app = session_app(&root);
        app.open_path("a.sprout").unwrap();
        app.enroll_open_document("a".into(), "sprout".into())
            .unwrap();
        app.open_path("b.sprout").unwrap();
        let identity = app.open_document().unwrap().evaluation_source_id().clone();
        assert_ne!(
            app.registered_documents()
                .unwrap()
                .document("a")
                .unwrap()
                .evaluation_source_id(),
            &identity
        );
        for (module, language) in [("a", "sprout"), ("b", ""), ("", "sprout")] {
            assert!(
                app.enroll_open_document(module.into(), language.into())
                    .is_err()
            );
            assert_eq!(
                app.open_document().unwrap().evaluation_source_id(),
                &identity
            );
        }
        app.open_document_mut()
            .unwrap()
            .apply_edit(crate::text::TextEdit::insert(
                crate::text::Position::new(0, 4),
                "!",
            ))
            .unwrap();
        assert!(app.select_registered_document("a").is_err());
        assert!(app.open_path("a.sprout").is_err());
        assert_eq!(
            app.open_document().unwrap().evaluation_source_id(),
            &identity
        );
        app.enroll_open_document("b".into(), "sprout".into())
            .unwrap();
        assert!(app.open_path("missing").is_err());
        assert_eq!(app.selected_registered_module(), Some("b"));
        let document = app.remove_registered_document("b").unwrap();
        assert!(document.is_dirty());
        assert_eq!(document.evaluation_source_id(), &identity);
        assert!(app.open_document().is_none());
    }

    #[cfg(feature = "experimental-sprout-closure")]
    #[test]
    fn unsaved_documents_include_inactive_enrolled_buffers_and_block_proposals() {
        let root = TempWorkspace::new("inactive_dirty_proposal_guard");
        root.write_file("a.sprout", "same");
        root.write_file("b.sprout", "same");
        let mut app = session_app(&root);
        assert!(!app.has_unsaved_documents());
        app.open_path("a.sprout").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(crate::text::TextEdit::insert(
                crate::text::Position::new(0, 4),
                "!",
            ))
            .unwrap();
        assert!(app.has_unsaved_documents());
        app.enroll_open_document("a".into(), "sprout".into())
            .unwrap();
        app.open_path("b.sprout").unwrap();
        assert!(!app.open_document().unwrap().is_dirty());
        assert!(app.has_unsaved_documents());
        let mut proposal = app
            .proposal(&[ScopePath::new("a.sprout").unwrap()], Default::default())
            .unwrap();
        let acceptance = app.accept_proposal(&mut proposal);
        assert!(acceptance.applied.is_empty());
        assert!(acceptance.error.is_some());
        assert_eq!(root.read_file("a.sprout"), "same");
        app.select_registered_document("a").unwrap();
        app.revert_open_document().unwrap();
        assert!(!app.has_unsaved_documents());
        app.open_document_mut()
            .unwrap()
            .apply_edit(crate::text::TextEdit::insert(
                crate::text::Position::new(0, 4),
                "!",
            ))
            .unwrap();
        assert!(app.has_unsaved_documents());
        let removed = app.remove_registered_document("a").unwrap();
        assert!(removed.is_dirty());
        assert!(!app.has_unsaved_documents());
    }

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
        let first_source = app.open_document().unwrap().evaluation_source_id().clone();
        #[cfg(not(feature = "experimental-sprout-closure"))]
        assert_eq!(
            app.clone().open_document().unwrap().evaluation_source_id(),
            &first_source
        );
        let second_id = app.open_path("README.md").unwrap().id();
        assert_ne!(
            app.open_document().unwrap().evaluation_source_id(),
            &first_source
        );

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
