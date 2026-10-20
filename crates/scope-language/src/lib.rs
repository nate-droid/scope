//! Language-neutral contracts for implementing Scope language services.
//!
//! This crate has no dependencies on Scope's app, Cell, or a UI. Services receive
//! read-only document text and path information, not filesystem authority.
//!
//! ```
//! use scope_language::{
//!     LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService,
//! };
//!
//! struct CalcLanguage;
//!
//! impl LanguageService for CalcLanguage {
//!     fn language_id(&self) -> &'static str { "example.calc" }
//!     fn language_name(&self) -> &'static str { "Calc" }
//!     fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
//!         path.extension() == Some("calc")
//!     }
//!     fn analyze(&self, _document: LanguageDocument<'_>) -> LanguageAnalysis {
//!         LanguageAnalysis::default()
//!     }
//! }
//!
//! let document = LanguageDocument::new(LanguagePath::new("main.calc"), "", 0);
//! assert!(CalcLanguage.matches_path(document.path()));
//! assert!(CalcLanguage.diagnostics(document).is_empty());
//! ```

use std::borrow::Cow;

mod interactive;
mod line_index;
mod semantic;
mod source;
pub use source::{
    EvaluationRequest, RevisionId, SourceId, SourceLocation, SourceResolver, SourceSnapshot,
    SourceUnavailable,
};

pub use interactive::{
    CancellationError, CancellationHandle, Completeness, CompletionItem, EvaluationResult,
    EvaluationStatus, InspectionError, InteractiveLanguageSession, InteractiveSessionProvider,
    SessionError, SymbolDescription, SymbolLocation, SymbolOrigin,
};
pub use line_index::LineIndex;
pub use semantic::{
    BlockerCategory, CandidateValidation, CheckObservation, CheckOutcome, ContractValidation,
    ExpectationOrigin, SemanticArgument, SemanticArgumentKind, SemanticBlocker, SemanticEdit,
    SemanticEvaluation, SemanticOperationDescription, SemanticOperationEffect,
    SemanticOperationError, SemanticOperationPurpose, SemanticOperationRequest,
    SemanticOperationResult, SemanticOutcome, SemanticTargetRequirement, SemanticValidationRequest,
    ValidationContract, ValidationStatus, execute_semantic_operation,
};

/// A language-facing path view that avoids granting filesystem access.
///
/// Language services receive path text for classification and diagnostics, but
/// not a host `Path`/`PathBuf` that might encourage ambient filesystem use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePath<'a> {
    text: Cow<'a, str>,
}

impl<'a> LanguagePath<'a> {
    pub fn new(text: impl Into<Cow<'a, str>>) -> Self {
        Self { text: text.into() }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn file_name(&self) -> Option<&str> {
        self.as_str()
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
    }

    pub fn extension(&self) -> Option<&str> {
        self.file_name()?
            .rsplit_once('.')
            .and_then(|(stem, extension)| {
                (!stem.is_empty() && !extension.is_empty()).then_some(extension)
            })
    }
}

impl std::fmt::Display for LanguagePath<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.text.fmt(formatter)
    }
}

impl<'a> From<&'a str> for LanguagePath<'a> {
    fn from(text: &'a str) -> Self {
        Self::new(text)
    }
}

impl From<String> for LanguagePath<'static> {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

/// A zero-based line/column location in a UTF-8 text document.
///
/// `column` is a byte offset within the line, not a Unicode scalar or grapheme
/// index. This keeps positions cheap to convert to Rust string slices while
/// still being line-oriented for UI and diagnostics. Use [`LineIndex`] for
/// checked conversions from parser byte offsets and canonical newline positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// A half-open range in a text document: `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: Position,
    pub end: Position,
}

impl TextRange {
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// A read-only view of a document passed to language services.
///
/// This deliberately contains only the information a language service needs for
/// single-document analysis. Wider project or filesystem access should be added
/// later as explicit capabilities rather than hidden behind this type.
#[derive(Debug, Clone)]
pub struct LanguageDocument<'a> {
    path: LanguagePath<'a>,
    text: &'a str,
    version: u64,
}

impl<'a> LanguageDocument<'a> {
    pub const fn new(path: LanguagePath<'a>, text: &'a str, version: u64) -> Self {
        Self {
            path,
            text,
            version,
        }
    }

    pub const fn path(&self) -> &LanguagePath<'a> {
        &self.path
    }

    pub const fn text(&self) -> &'a str {
        self.text
    }

    /// A document-local revision, not a globally unique content identity.
    ///
    /// Different document instances (including reopened documents) may share a
    /// path and version but contain different text. A `(path, version)` pair is
    /// therefore unsafe as a cache key across document instances; caches must
    /// also account for document identity or validate the text.
    pub const fn version(&self) -> u64 {
        self.version
    }
}

/// Diagnostics and outline symbols produced for a document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanguageAnalysis {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<Symbol>,
}

/// Language-facing semantic service contract.
///
/// Implementations should be adapters from a parser/compiler/analyzer into
/// Scope's language-neutral data structures. The trait intentionally avoids
/// filesystem or process access; those should be modeled as explicit
/// capabilities as the API grows.
pub trait LanguageService: Sync {
    fn language_id(&self) -> &'static str;
    fn language_name(&self) -> &'static str;
    fn matches_path(&self, path: &LanguagePath<'_>) -> bool;
    /// Produces diagnostics and outline symbols for the same document.
    ///
    /// Implement this directly, sharing a parser pass when both outputs need it.
    /// Do not call the default `diagnostics` or `outline` methods here: they
    /// delegate to this method. Combined consumers call this method directly,
    /// independently of any single-output overrides. Results are not cached.
    fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis;

    /// Projects diagnostics from combined analysis by default.
    ///
    /// May be overridden for a cheaper diagnostics-only path.
    fn diagnostics(&self, document: LanguageDocument<'_>) -> Vec<Diagnostic> {
        self.analyze(document).diagnostics
    }

    /// Projects outline symbols from combined analysis by default.
    ///
    /// May be overridden for a cheaper outline-only path.
    fn outline(&self, document: LanguageDocument<'_>) -> Vec<Symbol> {
        self.analyze(document).symbols
    }

    /// Descriptions use stable, language-local operation IDs.
    fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
        Vec::new()
    }

    /// Advertise optional evaluation outcomes in the existing operation catalog.
    /// Descriptive only: this grants no authority and does not require evaluation.
    fn semantic_operation_produces_evaluation(&self, _operation_id: &str) -> bool {
        false
    }

    /// Validates the advertised arguments and target before dispatching.
    /// Implementors should override `perform_semantic_operation`, not this method.
    fn execute_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        execute_semantic_operation(self, request)
    }

    /// Implementation hook; callers should use the validated execution entry point.
    /// Changes are proposals only: this method must not mutate the document.
    fn perform_semantic_operation(
        &self,
        request: SemanticOperationRequest<'_>,
    ) -> Result<SemanticOperationResult, SemanticOperationError> {
        Err(SemanticOperationError::UnsupportedOperation(
            request.operation_id.into(),
        ))
    }

    /// Statically validates inert candidate source without executing it, applying
    /// edits, accessing the filesystem, spawning processes, or loading dependencies.
    /// Diagnostic ranges refer to candidate source, not the original document.
    /// The default performs no evaluation and returns `NotEvaluated`.
    fn validate_semantic_candidate(
        &self,
        _request: SemanticValidationRequest<'_>,
    ) -> Result<CandidateValidation, SemanticOperationError> {
        Ok(CandidateValidation::default())
    }

    /// Evaluate explicit expectations against the actual final proposal preview.
    /// This grants no additional capabilities: execution, if supported, must use
    /// the language's existing bounded evaluator and host authority. Defaults to
    /// absent evidence; static acceptance cannot satisfy a behavioral contract.
    fn validate_semantic_contract(
        &self,
        _request: SemanticValidationRequest<'_>,
        _contract: &ValidationContract,
    ) -> Result<ContractValidation, SemanticOperationError> {
        Ok(ContractValidation::default())
    }

    fn actions(&self, _document: LanguageDocument<'_>) -> Vec<LanguageAction> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageAction {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Explicit original-source locations, primary first. Empty means legacy
    /// text-relative coordinates; never infer historical provenance from `source`.
    pub locations: Vec<SourceLocation>,
    pub range: TextRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
}

impl Diagnostic {
    pub fn error(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            locations: Vec::new(),
            range,
            severity: DiagnosticSeverity::Error,
            message: message.into(),
            source: None,
            code: None,
        }
    }

    pub fn warning(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            locations: Vec::new(),
            range,
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            source: None,
            code: None,
        }
    }

    pub fn information(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            locations: Vec::new(),
            range,
            severity: DiagnosticSeverity::Information,
            message: message.into(),
            source: None,
            code: None,
        }
    }

    pub fn hint(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            locations: Vec::new(),
            range,
            severity: DiagnosticSeverity::Hint,
            message: message.into(),
            source: None,
            code: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    File,
    Module,
    Namespace,
    Package,
    Class,
    Method,
    Property,
    Field,
    Constructor,
    Enum,
    Interface,
    Function,
    Variable,
    Constant,
    String,
    Number,
    Boolean,
    Array,
    Object,
    Key,
    Null,
    EnumMember,
    Struct,
    Event,
    Operator,
    TypeParameter,
}

impl std::fmt::Display for DiagnosticSeverity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => write!(formatter, "error"),
            Self::Warning => write!(formatter, "warning"),
            Self::Information => write!(formatter, "information"),
            Self::Hint => write!(formatter, "hint"),
        }
    }
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File => write!(formatter, "file"),
            Self::Module => write!(formatter, "module"),
            Self::Namespace => write!(formatter, "namespace"),
            Self::Package => write!(formatter, "package"),
            Self::Class => write!(formatter, "class"),
            Self::Method => write!(formatter, "method"),
            Self::Property => write!(formatter, "property"),
            Self::Field => write!(formatter, "field"),
            Self::Constructor => write!(formatter, "constructor"),
            Self::Enum => write!(formatter, "enum"),
            Self::Interface => write!(formatter, "interface"),
            Self::Function => write!(formatter, "function"),
            Self::Variable => write!(formatter, "variable"),
            Self::Constant => write!(formatter, "constant"),
            Self::String => write!(formatter, "string"),
            Self::Number => write!(formatter, "number"),
            Self::Boolean => write!(formatter, "boolean"),
            Self::Array => write!(formatter, "array"),
            Self::Object => write!(formatter, "object"),
            Self::Key => write!(formatter, "key"),
            Self::Null => write!(formatter, "null"),
            Self::EnumMember => write!(formatter, "enum member"),
            Self::Struct => write!(formatter, "struct"),
            Self::Event => write!(formatter, "event"),
            Self::Operator => write!(formatter, "operator"),
            Self::TypeParameter => write!(formatter, "type parameter"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ProjectionService;

    fn assert_document(document: &LanguageDocument<'_>) {
        assert_eq!(document.path().as_str(), "main.scope");
        assert_eq!(document.text(), "example");
        assert_eq!(document.version(), 7);
    }

    fn sample_analysis() -> LanguageAnalysis {
        let range = TextRange::new(Position::new(0, 0), Position::new(0, 7));
        LanguageAnalysis {
            diagnostics: vec![Diagnostic::warning(range, "example diagnostic")],
            symbols: vec![Symbol {
                name: "example".into(),
                kind: SymbolKind::Function,
                range,
            }],
        }
    }

    impl LanguageService for ProjectionService {
        fn language_id(&self) -> &'static str {
            "example"
        }

        fn language_name(&self) -> &'static str {
            "Example"
        }

        fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
            path.extension() == Some("scope")
        }

        fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
            assert_document(&document);
            sample_analysis()
        }
    }

    struct CombinedService;

    impl LanguageService for CombinedService {
        fn language_id(&self) -> &'static str {
            "example"
        }

        fn language_name(&self) -> &'static str {
            "Example"
        }

        fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
            path.extension() == Some("scope")
        }

        fn diagnostics(&self, _document: LanguageDocument<'_>) -> Vec<Diagnostic> {
            panic!("analyze override must not call diagnostics")
        }

        fn outline(&self, _document: LanguageDocument<'_>) -> Vec<Symbol> {
            panic!("analyze override must not call outline")
        }

        fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
            assert_document(&document);
            sample_analysis()
        }
    }

    #[test]
    fn default_projections_delegate_through_dyn_service() {
        let service: &dyn LanguageService = &ProjectionService;
        let document = LanguageDocument::new(LanguagePath::new("main.scope"), "example", 7);

        assert_eq!(
            service.diagnostics(document.clone()),
            sample_analysis().diagnostics
        );
        assert_eq!(service.outline(document.clone()), sample_analysis().symbols);
        assert_eq!(service.analyze(document), sample_analysis());
    }

    #[test]
    fn analysis_override_dispatches_through_dyn_service() {
        let service: &dyn LanguageService = &CombinedService;
        let document = LanguageDocument::new(LanguagePath::new("main.scope"), "example", 7);

        assert_eq!(service.analyze(document), sample_analysis());
    }

    #[test]
    fn single_output_overrides_dispatch_independently() {
        struct Overrides;

        impl LanguageService for Overrides {
            fn language_id(&self) -> &'static str {
                "example.overrides"
            }
            fn language_name(&self) -> &'static str {
                "Overrides"
            }
            fn matches_path(&self, _path: &LanguagePath<'_>) -> bool {
                true
            }
            fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
                assert_document(&document);
                sample_analysis()
            }
            fn diagnostics(&self, document: LanguageDocument<'_>) -> Vec<Diagnostic> {
                assert_document(&document);
                Vec::new()
            }
            fn outline(&self, document: LanguageDocument<'_>) -> Vec<Symbol> {
                assert_document(&document);
                Vec::new()
            }
        }

        let service: &dyn LanguageService = &Overrides;
        let document = LanguageDocument::new(LanguagePath::new("main.scope"), "example", 7);
        assert!(service.diagnostics(document.clone()).is_empty());
        assert!(service.outline(document.clone()).is_empty());
        assert_eq!(service.analyze(document), sample_analysis());
    }

    #[test]
    fn language_path_exposes_classification_without_filesystem_access() {
        let path = LanguagePath::new("src/main.scope");

        assert_eq!(path.file_name(), Some("main.scope"));
        assert_eq!(path.extension(), Some("scope"));
        assert_eq!(path.as_str(), "src/main.scope");
    }
}
