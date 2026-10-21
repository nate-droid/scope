//! Read-only, derived source selection for interactive hosts.
//!
//! This is not a document store, an edit target, or semantic authorization. Derive
//! it again after host transitions; identities and versions belong to the owners.
use crate::{
    app::App,
    document::Document,
    language::{InteractiveSession, LanguageRegistry, LanguageService},
    path::ScopePath,
    scratch::{ScratchBuffer, ScratchBuffers},
    source_edit::SourceId,
};

/// An explicitly requested source owner; no variant takes precedence over another.
/// A buffer with the App document's destination is still a different owner.
#[derive(Clone, Copy)]
pub enum SourceContext<'a> {
    None,
    App(&'a App),
    Buffer {
        buffer: &'a ScratchBuffer,
        languages: &'a LanguageRegistry,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    None,
    App,
    Buffer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceContextError {
    NoAppDocument,
    NoSelectedBuffer,
}

impl std::fmt::Display for SourceContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoAppDocument => "no App document is open",
            Self::NoSelectedBuffer => "no buffer is selected",
        })
    }
}

impl std::error::Error for SourceContextError {}

/// Both available source owners and the independent interactive session.
/// This descriptor does not select a command target or grant authority.
#[derive(Clone, Copy)]
pub struct HostSourceContext<'a> {
    pub app: Option<SourceContext<'a>>,
    pub buffer: Option<SourceContext<'a>>,
    pub active_session: &'a InteractiveSession,
}

impl<'a> HostSourceContext<'a> {
    pub fn derive(
        app: &'a App,
        buffers: &'a ScratchBuffers,
        session: &'a InteractiveSession,
    ) -> Self {
        Self {
            app: SourceContext::resolve(app, buffers, SourceKind::App).ok(),
            buffer: SourceContext::resolve(app, buffers, SourceKind::Buffer).ok(),
            active_session: session,
        }
    }

    /// Both owners exist and are distinct, regardless of path, text or version.
    /// With either owner absent there are not two targets to compare.
    pub fn targets_differ(self) -> bool {
        self.app.is_some() && self.buffer.is_some()
    }
}

impl<'a> SourceContext<'a> {
    /// Resolve only the requested owner. Missing sources never fall back to the
    /// other owner; explicit None succeeds even when sources are available.
    pub fn resolve(
        app: &'a App,
        buffers: &'a ScratchBuffers,
        kind: SourceKind,
    ) -> Result<Self, SourceContextError> {
        match kind {
            SourceKind::None => Ok(Self::None),
            SourceKind::App if app.open_document().is_some() => Ok(Self::App(app)),
            SourceKind::App => Err(SourceContextError::NoAppDocument),
            SourceKind::Buffer => buffers
                .selected()
                .map(|buffer| Self::Buffer {
                    buffer,
                    languages: app.languages(),
                })
                .map_err(|_| SourceContextError::NoSelectedBuffer),
        }
    }

    pub fn kind(self) -> SourceKind {
        match self {
            Self::None => SourceKind::None,
            Self::App(_) => SourceKind::App,
            Self::Buffer { .. } => SourceKind::Buffer,
        }
    }

    pub fn document(self) -> Option<&'a Document> {
        match self {
            Self::App(app) => app.open_document(),
            _ => None,
        }
    }

    pub fn buffer(self) -> Option<&'a ScratchBuffer> {
        match self {
            Self::Buffer { buffer, .. } => Some(buffer),
            _ => None,
        }
    }

    /// Existing edit-target identity, not a path or an evaluation source ID.
    /// As with SourceTarget, IDs are local to their owning App/buffer collection.
    pub fn source_id(self) -> Option<SourceId> {
        match self {
            Self::Buffer { buffer, .. } => Some(SourceId::Scratch(buffer.id())),
            _ => self
                .document()
                .map(|document| SourceId::Document(document.id())),
        }
    }

    pub fn version(self) -> Option<u64> {
        match self {
            Self::Buffer { buffer, .. } => Some(buffer.version()),
            _ => self.document().map(Document::version),
        }
    }

    pub fn text(self) -> Option<&'a str> {
        match self {
            Self::Buffer { buffer, .. } => Some(buffer.text()),
            _ => self.document().map(Document::text),
        }
    }

    /// Display/language context only; this grants no filesystem authority.
    pub fn path(self) -> Option<&'a ScopePath> {
        match self {
            Self::Buffer { buffer, .. } => buffer.destination_path(),
            _ => self.document().map(Document::path),
        }
    }

    pub fn is_dirty(self) -> Option<bool> {
        match self {
            Self::Buffer { buffer, .. } => Some(buffer.is_dirty()),
            _ => self.document().map(Document::is_dirty),
        }
    }

    /// Existing path-based semantic selection, including the neutral fallback.
    /// Pathless scratch language IDs do not imply semantic service registration.
    /// Returning a service does not authorize inspection or change its target.
    pub fn semantic_service(self) -> Option<&'static dyn LanguageService> {
        match self {
            Self::App(app) => app
                .open_document()
                .map(|document| app.semantic_service_for_document(document)),
            Self::Buffer { buffer, languages } => buffer
                .destination_path()
                .map(|path| languages.semantic_service_for_path(path)),
            Self::None => None,
        }
    }

    /// Source language for interactive routing, not the active session language
    /// and not a promise that an interactive provider is registered.
    pub fn interactive_language(self) -> Option<&'a str> {
        match self {
            Self::App(app) => app.open_language().map(|language| language.language_id()),
            Self::Buffer { buffer, .. } => Some(buffer.language_id()),
            Self::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_support::TempWorkspace,
        text::{Position, TextEdit, TextRange},
    };

    fn insert(text: &str) -> TextEdit {
        TextEdit::replace(
            TextRange::new(Position::new(0, 0), Position::new(0, 0)),
            text,
        )
    }

    struct Session;

    impl crate::language::InteractiveLanguageSession for Session {
        fn language_id(&self) -> &str {
            "session-only"
        }
        fn evaluate(
            &mut self,
            _: &str,
        ) -> Result<crate::language::EvaluationResult, crate::language::SessionError> {
            panic!("source context must not evaluate")
        }
        fn reset(&mut self) -> Result<(), crate::language::SessionError> {
            panic!("source context must not reset")
        }
    }

    #[test]
    fn explicit_resolution_and_aggregate_do_not_choose_authority() {
        let root = TempWorkspace::new("source_context_explicit");
        root.write_file("main.scope", "disk");
        let mut app = App::new(root.workspace()).unwrap();
        let mut buffers = ScratchBuffers::default();
        let mut session = InteractiveSession::new();
        assert_eq!(
            SourceContext::resolve(&app, &buffers, SourceKind::App).err(),
            Some(SourceContextError::NoAppDocument)
        );
        assert_eq!(
            SourceContext::resolve(&app, &buffers, SourceKind::Buffer).err(),
            Some(SourceContextError::NoSelectedBuffer)
        );
        let host = HostSourceContext::derive(&app, &buffers, &session);
        assert!(host.app.is_none() && host.buffer.is_none());
        assert!(!host.targets_differ());
        assert_eq!(host.active_session.language_id(), None);

        buffers.create("buffer-only", "scratch");
        assert_eq!(
            SourceContext::resolve(&app, &buffers, SourceKind::App).err(),
            Some(SourceContextError::NoAppDocument)
        );
        let host = HostSourceContext::derive(&app, &buffers, &session);
        assert!(host.app.is_none() && host.buffer.is_some());
        assert!(!host.targets_differ());
        buffers.discard_selected().unwrap();
        app.open_path("main.scope").unwrap();
        assert_eq!(
            SourceContext::resolve(&app, &buffers, SourceKind::Buffer).err(),
            Some(SourceContextError::NoSelectedBuffer)
        );
        assert!(!HostSourceContext::derive(&app, &buffers, &session).targets_differ());

        buffers
            .open(app.workspace(), app.languages(), "main.scope")
            .unwrap();
        session.install("session-only", Box::new(Session)).unwrap();
        let host = HostSourceContext::derive(&app, &buffers, &session);
        let app_context = host.app.unwrap();
        let buffer_context = host.buffer.unwrap();
        assert_eq!(app_context.path(), buffer_context.path());
        assert_eq!(app_context.text(), buffer_context.text());
        assert_eq!(app_context.version(), buffer_context.version());
        assert!(host.targets_differ());
        assert!(std::ptr::eq(host.active_session, &session));
        assert_eq!(host.active_session.language_id(), Some("session-only"));
        assert_eq!(app_context.interactive_language(), Some("scope.simple"));
        assert_eq!(buffer_context.interactive_language(), Some("scope.simple"));
        assert_eq!(
            SourceContext::resolve(&app, &buffers, SourceKind::App)
                .unwrap()
                .source_id(),
            app_context.source_id()
        );
        assert_eq!(
            SourceContext::resolve(&app, &buffers, SourceKind::Buffer)
                .unwrap()
                .source_id(),
            buffer_context.source_id()
        );
        assert_eq!(
            SourceContext::resolve(&app, &buffers, SourceKind::None)
                .unwrap()
                .kind(),
            SourceKind::None
        );
        assert!(!app.open_document().unwrap().is_dirty());
        assert!(!buffers.selected().unwrap().is_dirty());
        assert_eq!(root.read_file("main.scope"), "disk");
    }

    #[test]
    fn transitions_require_explicit_owner_selection() {
        let root = TempWorkspace::new("source_context_transitions");
        root.write_file("main.scope", "disk");
        let mut app = App::new(root.workspace()).unwrap();
        let mut buffers = ScratchBuffers::default();
        let context = SourceContext::resolve(&app, &buffers, SourceKind::None).unwrap();
        assert_eq!(context.kind(), SourceKind::None);
        assert_eq!(context.source_id(), None);
        assert_eq!(context.version(), None);
        assert_eq!(context.text(), None);
        assert_eq!(context.path(), None);
        assert_eq!(context.is_dirty(), None);
        assert!(context.semantic_service().is_none());
        assert_eq!(context.interactive_language(), None);

        app.open_path("main.scope").unwrap();
        let app_id = SourceContext::resolve(&app, &buffers, SourceKind::App)
            .unwrap()
            .source_id();
        let first = buffers.create("interactive-only", "first");
        let second = buffers.create("other", "second");
        assert_eq!(
            SourceContext::resolve(&app, &buffers, SourceKind::Buffer)
                .unwrap()
                .text(),
            Some("second")
        );
        buffers.select(first).unwrap();
        let context = SourceContext::resolve(&app, &buffers, SourceKind::Buffer).unwrap();
        assert_eq!(context.kind(), SourceKind::Buffer);
        assert_eq!(context.source_id(), Some(SourceId::Scratch(first)));
        assert_eq!(context.interactive_language(), Some("interactive-only"));
        assert!(context.semantic_service().is_none());
        assert!(std::ptr::eq(
            context.text().unwrap().as_ptr(),
            buffers.selected().unwrap().text().as_ptr()
        ));
        buffers.discard_selected().unwrap();
        buffers.select(second).unwrap();
        buffers.discard_selected().unwrap();
        let context = SourceContext::resolve(&app, &buffers, SourceKind::App).unwrap();
        assert_eq!(context.kind(), SourceKind::App);
        assert_eq!(context.source_id(), app_id);
        assert_eq!(context.interactive_language(), Some("scope.simple"));
        assert_eq!(
            context.semantic_service().unwrap().language_id(),
            "scope.simple"
        );
    }

    #[test]
    fn same_path_has_distinct_identity_text_and_versions_without_mutation() {
        let root = TempWorkspace::new("source_context_same_path");
        root.write_file("main.scope", "disk");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("main.scope").unwrap();
        let mut buffers = ScratchBuffers::default();
        let id = buffers
            .open(app.workspace(), app.languages(), "main.scope")
            .unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(insert("app "))
            .unwrap();
        buffers
            .selected_mut()
            .unwrap()
            .apply_edit(insert("buffer "))
            .unwrap();
        buffers
            .selected_mut()
            .unwrap()
            .apply_edit(insert("new "))
            .unwrap();
        let document = app.open_document().unwrap();
        let before = (
            document.id(),
            document.version(),
            document.text().to_owned(),
            document.is_dirty(),
        );
        let buffer = buffers.selected().unwrap();
        let buffer_before = (
            buffer.version(),
            buffer.text().to_owned(),
            buffer.is_dirty(),
            buffer.destination_revision().map(str::to_owned),
            buffer.evaluation_source_id().clone(),
        );
        for _ in 0..3 {
            let context = SourceContext::resolve(&app, &buffers, SourceKind::Buffer).unwrap();
            assert_eq!(context.kind(), SourceKind::Buffer);
            assert_eq!(context.source_id(), Some(SourceId::Scratch(id)));
            assert_ne!(context.source_id(), Some(SourceId::Document(document.id())));
            assert_eq!(context.path(), Some(document.path()));
            assert_eq!(context.version(), Some(2));
            assert_eq!(document.version(), 1);
            assert_eq!(context.text(), Some("new buffer disk"));
            assert_eq!(document.text(), "app disk");
            assert_eq!(context.is_dirty(), Some(true));
            assert_eq!(
                context.semantic_service().unwrap().language_id(),
                "scope.simple"
            );
        }
        assert_eq!(
            before,
            (
                document.id(),
                document.version(),
                document.text().to_owned(),
                document.is_dirty()
            )
        );
        let buffer = buffers.selected().unwrap();
        assert_eq!(
            buffer_before,
            (
                buffer.version(),
                buffer.text().to_owned(),
                buffer.is_dirty(),
                buffer.destination_revision().map(str::to_owned),
                buffer.evaluation_source_id().clone()
            )
        );
        assert_eq!(root.read_file("main.scope"), "disk");
        let old_id = SourceContext::App(&app).source_id();
        app.open_path("main.scope").unwrap();
        assert_ne!(SourceContext::App(&app).source_id(), old_id);
        assert_eq!(SourceContext::App(&app).version(), Some(0));
    }

    #[test]
    fn neutral_semantics_are_not_an_interactive_language() {
        let root = TempWorkspace::new("source_context_neutral");
        root.write_file("plain.unknown", "text");
        let mut app = App::new(root.workspace()).unwrap();
        app.open_path("plain.unknown").unwrap();
        let mut buffers = ScratchBuffers::default();
        let context = SourceContext::resolve(&app, &buffers, SourceKind::App).unwrap();
        assert!(context.semantic_service().is_some());
        assert_eq!(context.interactive_language(), None);
        buffers
            .open(app.workspace(), app.languages(), "plain.unknown")
            .unwrap();
        let context = SourceContext::resolve(&app, &buffers, SourceKind::Buffer).unwrap();
        assert!(context.semantic_service().is_some());
        assert_eq!(context.interactive_language(), Some("text"));
    }
}
