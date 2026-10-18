use crate::document::Document;
use crate::path::ScopePath;
pub use scope_language::{
    CancellationHandle, Completeness, CompletionItem, Diagnostic, DiagnosticSeverity,
    EvaluationResult, EvaluationStatus, InspectionError, InteractiveLanguageSession,
    InteractiveSessionProvider, LanguageAction, LanguageAnalysis, LanguageDocument, LanguagePath,
    LanguageService, Position, SessionError, Symbol, SymbolDescription, SymbolKind, SymbolLocation,
    SymbolOrigin, TextRange,
};

pub mod interactive;
pub use interactive::{InputRoute, InteractiveSession};
pub mod meta;
mod neutral;

pub use meta::LanguageMeta;
mod simple;

pub use neutral::NeutralLanguage;
pub use simple::SimpleLanguage;

pub static NEUTRAL_LANGUAGE: NeutralLanguage = NeutralLanguage;
pub static SIMPLE_LANGUAGE: SimpleLanguage = SimpleLanguage;

#[derive(Clone)]
pub struct LanguageRegistry {
    services: Vec<&'static dyn LanguageService>,
    providers: Vec<&'static dyn InteractiveSessionProvider>,
}

impl LanguageRegistry {
    pub fn empty() -> Self {
        Self {
            services: Vec::new(),
            providers: Vec::new(),
        }
    }

    pub fn builtins() -> Self {
        Self::empty().with_language(&SIMPLE_LANGUAGE)
    }

    pub fn with_language(mut self, language: &'static dyn LanguageService) -> Self {
        self.services.push(language);
        self
    }

    pub fn with_languages(
        mut self,
        languages: impl IntoIterator<Item = &'static dyn LanguageService>,
    ) -> Self {
        self.services.extend(languages);
        self
    }

    pub fn with_interactive_provider(
        mut self,
        provider: &'static dyn InteractiveSessionProvider,
    ) -> Self {
        self.providers.push(provider);
        self
    }

    pub fn interactive_provider(
        &self,
        id: &str,
    ) -> Option<&'static dyn InteractiveSessionProvider> {
        self.providers
            .iter()
            .copied()
            .find(|provider| provider.language_id() == id)
    }

    pub fn interactive_providers(&self) -> &[&'static dyn InteractiveSessionProvider] {
        &self.providers
    }

    pub fn services(&self) -> &[&'static dyn LanguageService] {
        &self.services
    }

    pub fn language_for_document(
        &self,
        document: &Document,
    ) -> Option<&'static dyn LanguageService> {
        self.language_for_path(document.path())
    }

    pub fn language_for_path(&self, path: &ScopePath) -> Option<&'static dyn LanguageService> {
        let path = LanguagePath::new(path.to_string_lossy());
        self.services
            .iter()
            .copied()
            .find(|language| language.matches_path(&path))
    }

    pub fn semantic_service_for_document(
        &self,
        document: &Document,
    ) -> &'static dyn LanguageService {
        self.language_for_document(document)
            .unwrap_or(&NEUTRAL_LANGUAGE)
    }

    pub fn semantic_service_for_path(&self, path: &ScopePath) -> &'static dyn LanguageService {
        self.language_for_path(path).unwrap_or(&NEUTRAL_LANGUAGE)
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::builtins()
    }
}

impl std::fmt::Debug for LanguageRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LanguageRegistry")
            .field(
                "services",
                &self
                    .services
                    .iter()
                    .map(|service| service.language_id())
                    .collect::<Vec<_>>(),
            )
            .field(
                "providers",
                &self
                    .providers
                    .iter()
                    .map(|provider| provider.language_id())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

pub fn language_for_document(document: &Document) -> Option<&'static dyn LanguageService> {
    LanguageRegistry::builtins().language_for_document(document)
}

pub fn language_for_path(path: &ScopePath) -> Option<&'static dyn LanguageService> {
    LanguageRegistry::builtins().language_for_path(path)
}

pub fn semantic_service_for_document(document: &Document) -> &'static dyn LanguageService {
    LanguageRegistry::builtins().semantic_service_for_document(document)
}

pub fn semantic_service_for_path(path: &ScopePath) -> &'static dyn LanguageService {
    LanguageRegistry::builtins().semantic_service_for_path(path)
}

pub fn builtins() -> [&'static dyn LanguageService; 1] {
    [&SIMPLE_LANGUAGE]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_simple_language_by_extension() {
        assert_eq!(
            language_for_path(&ScopePath::new("example.scope").unwrap())
                .unwrap()
                .language_id(),
            "scope.simple"
        );
        assert!(language_for_path(&ScopePath::new("README.md").unwrap()).is_none());
        assert_eq!(
            semantic_service_for_path(&ScopePath::new("README.md").unwrap()).language_id(),
            "scope.neutral"
        );
    }

    #[test]
    fn registry_accepts_external_languages_without_changing_builtins() {
        struct ExternalLanguage;

        impl LanguageService for ExternalLanguage {
            fn language_id(&self) -> &'static str {
                "example.external"
            }

            fn language_name(&self) -> &'static str {
                "External"
            }

            fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
                path.extension() == Some("external")
            }

            fn analyze(&self, _document: LanguageDocument<'_>) -> LanguageAnalysis {
                LanguageAnalysis::default()
            }
        }

        static EXTERNAL_LANGUAGE: ExternalLanguage = ExternalLanguage;

        let registry = LanguageRegistry::builtins().with_language(&EXTERNAL_LANGUAGE);

        assert_eq!(
            registry
                .language_for_path(&ScopePath::new("main.external").unwrap())
                .unwrap()
                .language_id(),
            "example.external"
        );
        assert!(language_for_path(&ScopePath::new("main.external").unwrap()).is_none());
    }

    #[test]
    fn language_actions_default_to_empty() {
        let document = Document::new(
            crate::document::DocumentId::new(1),
            "README.md",
            "# Hello\n",
        );

        assert_eq!(
            NEUTRAL_LANGUAGE.actions(document.as_language_document()),
            Vec::new()
        );
        assert_eq!(
            SIMPLE_LANGUAGE.actions(document.as_language_document()),
            Vec::new()
        );
    }
}
