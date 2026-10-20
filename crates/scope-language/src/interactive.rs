//! Optional synchronous interactive contracts, independent of language analysis.
//!
//! These contracts provide no runtime, authorization policy, or filesystem/process
//! capabilities. Implementations and their hosts are responsible for those concerns.

use std::{fmt, sync::Arc};

use crate::Diagnostic;

/// Optional factory for interactive sessions; not required by `LanguageService`.
pub trait InteractiveSessionProvider: Sync {
    fn language_id(&self) -> &'static str;

    /// Creates an independent session whose language ID matches this provider.
    fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError>;
}

/// A stateful, synchronous language session.
///
/// Language-level failures belong in an `EvaluationResult`; authorization and
/// infrastructure failures belong in `SessionError`. The language ID must remain
/// stable and match the provider. Neither `Send` nor `Sync` is imposed on session
/// state; only the independent cancellation handle must be thread-safe.
pub trait InteractiveLanguageSession {
    fn language_id(&self) -> &str;

    /// Evaluates source in the current session, preserving output order.
    ///
    /// After `Cancelled { requires_reset: true }`, evaluation must return
    /// `SessionError::ResetRequired` until a successful reset. An unsuccessful
    /// reset must not clear that requirement.
    fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError>;

    /// Evaluates exact in-memory text with descriptive source context. Adapters
    /// may retain cloned identities and text for definitions and historical errors.
    /// The default preserves text-only implementations and does not invent provenance.
    fn evaluate_request(
        &mut self,
        request: crate::EvaluationRequest<'_>,
    ) -> Result<EvaluationResult, SessionError> {
        self.evaluate(request.text)
    }

    /// Restores a fresh evaluation state, including clearing any reset requirement.
    fn reset(&mut self) -> Result<(), SessionError>;

    /// Releases resources. The default is suitable for resource-free sessions.
    fn close(&mut self) -> Result<(), SessionError> {
        Ok(())
    }

    /// Inspects source without evaluating it or changing any session state,
    /// including interpreter dictionaries, stacks, and reset requirements.
    /// Must not request or acquire capabilities, widen grants, or perform host
    /// side effects such as filesystem or network access.
    ///
    /// The shared borrow prevents ordinary mutation, but cannot prevent interior
    /// mutability or ambient host access; providers must uphold this contract.
    /// Use syntax-only inspection, never speculative evaluation followed by rollback.
    ///
    /// This is advisory: complete input may still fail evaluation. Implementations
    /// without completeness support treat all input as complete.
    fn completeness(&self, _source: &str) -> Completeness {
        Completeness::Complete
    }

    /// Suggests edits for source without evaluating it or changing session state,
    /// including dictionaries, stacks, and reset requirements. Must not request
    /// or acquire capabilities, widen grants, or perform host side effects such
    /// as filesystem or network access. Never use speculative evaluation.
    ///
    /// `cursor` is a source-relative UTF-8 byte offset on a character boundary,
    /// including the end of source. Replacement ranges use the same coordinates.
    /// Hosts must validate the cursor before calling and validate returned ranges.
    /// The shared borrow is not a sandbox: providers must uphold this contract
    /// even when using interior mutability. Completion support is optional.
    fn complete(&self, _source: &str, _cursor: usize) -> Vec<CompletionItem> {
        Vec::new()
    }

    /// Looks up a language-specific name using already authorized, in-memory metadata.
    /// `Ok(None)` means unknown; unsupported inspection and failures are distinct.
    /// Must never evaluate source, mutate session state (including through interior
    /// mutability), request/grant capabilities, or read files/access the network.
    /// Return `Unsupported` if lookup cannot meet these constraints. Only expose
    /// metadata the host is already authorized to see. A shared borrow is not a sandbox.
    fn inspect(&self, _name: &str) -> Result<Option<SymbolDescription>, InspectionError> {
        Err(InspectionError::Unsupported)
    }

    /// Returns an independent handle that can be used while `evaluate` holds the
    /// mutable session borrow, including from another thread.
    ///
    /// `None` means cancellation is unavailable. A successful request only signals
    /// intent, not completion: evaluation may already have finished. Implementations
    /// must keep the handle and evaluation coherent: an evaluation actually stopped
    /// by cancellation reports `Cancelled`, and reports whether reset is required.
    /// Requests must not inadvertently cancel a later evaluation; implementations
    /// define handle lifetime and behavior when idle, reset, or closed.
    fn cancellation_handle(&self) -> Option<Arc<dyn CancellationHandle>> {
        None
    }
}

/// Optional metadata for a session-visible symbol. Reuses document symbol kinds
/// and coordinates; providers may project existing analysis or interpreter metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDescription {
    pub name: String,
    pub kind: crate::SymbolKind,
    pub signature: Option<String>,
    pub documentation: Option<String>,
    pub origin: Option<SymbolOrigin>,
    /// Source text, if already available and authorized; never loaded by Scope.
    pub definition: Option<String>,
    pub location: Option<SymbolLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolOrigin {
    BuiltIn,
    Workspace,
    Interactive,
}

/// Descriptive location only, not authority to open or read the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolLocation {
    pub path: crate::LanguagePath<'static>,
    pub range: crate::TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectionError {
    Unsupported,
    NoSession,
    Authorization { code: String, message: String },
    Infrastructure { code: String, message: String },
}

impl fmt::Display for InspectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "symbol inspection is unsupported"),
            Self::NoSession => write!(f, "no interactive language selected"),
            Self::Authorization { code, message } => {
                write!(f, "inspection authorization error ({code}): {message}")
            }
            Self::Infrastructure { code, message } => {
                write!(f, "inspection infrastructure error ({code}): {message}")
            }
        }
    }
}
impl std::error::Error for InspectionError {}

/// A suggested source edit. Coordinates refer to the entire supplied source,
/// including newlines, not just the cursor's line or a command prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    /// Half-open UTF-8 byte range to replace. Must be ordered, within source,
    /// and have both endpoints on character boundaries. Empty ranges insert.
    pub replacement: std::ops::Range<usize>,
    pub insert_text: String,
    pub detail: Option<String>,
}

/// Whether input is ready for evaluation, needs more source, or is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completeness {
    /// Source is ready for evaluation (not a guarantee of evaluation success).
    Complete,
    /// Additional input may complete the source.
    Incomplete,
    /// A syntax error that additional input cannot resolve.
    /// Diagnostics use source-relative ranges and may be empty if unavailable.
    Invalid { diagnostics: Vec<Diagnostic> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationResult {
    pub output: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub status: EvaluationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationStatus {
    Success,
    Failed,
    /// The evaluation was cancelled; a poisoned session cannot evaluate until reset.
    Cancelled {
        requires_reset: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    UnsupportedLanguage(String),
    Authorization { code: String, message: String },
    Infrastructure { code: String, message: String },
    ResetRequired,
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedLanguage(language) => {
                write!(formatter, "unsupported language: {language}")
            }
            Self::Authorization { code, message } => {
                write!(formatter, "authorization error ({code}): {message}")
            }
            Self::Infrastructure { code, message } => {
                write!(formatter, "infrastructure error ({code}): {message}")
            }
            Self::ResetRequired => write!(formatter, "session reset required"),
        }
    }
}

impl std::error::Error for SessionError {}

/// Thread-safe cancellation signaling, separate from mutable session access.
pub trait CancellationHandle: Send + Sync {
    /// Signals cancellation without waiting for evaluation to finish.
    fn request_cancel(&self) -> Result<(), CancellationError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancellationError {
    Unsupported,
    Failed { code: String, message: String },
}

impl fmt::Display for CancellationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(formatter, "cancellation unsupported"),
            Self::Failed { code, message } => {
                write!(formatter, "cancellation failed ({code}): {message}")
            }
        }
    }
}

impl std::error::Error for CancellationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Position, TextRange};
    use std::sync::{Condvar, Mutex};

    struct BasicSession;

    impl InteractiveLanguageSession for BasicSession {
        fn language_id(&self) -> &str {
            "mock"
        }

        fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
            Ok(EvaluationResult {
                output: vec![source.into(), "done".into()],
                diagnostics: if source == "bad" {
                    vec![Diagnostic::error(
                        TextRange::new(Position::new(0, 0), Position::new(0, 3)),
                        "invalid input",
                    )]
                } else {
                    Vec::new()
                },
                status: if source == "bad" {
                    EvaluationStatus::Failed
                } else {
                    EvaluationStatus::Success
                },
            })
        }

        fn reset(&mut self) -> Result<(), SessionError> {
            Ok(())
        }
    }

    struct Provider;

    impl InteractiveSessionProvider for Provider {
        fn language_id(&self) -> &'static str {
            "mock"
        }

        fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
            Ok(Box::new(BasicSession))
        }
    }

    #[test]
    fn provider_and_session_defaults_work_through_trait_objects() {
        let provider: &dyn InteractiveSessionProvider = &Provider;
        let mut session = provider.create_session().unwrap();
        assert_eq!(session.language_id(), provider.language_id());
        for source in ["", "(", "bad"] {
            assert_eq!(session.completeness(source), Completeness::Complete);
        }
        for source in ["", "é", "first\nsecond"] {
            let read_only: &dyn InteractiveLanguageSession = &*session;
            assert!(read_only.complete(source, source.len()).is_empty());
        }
        assert_eq!(session.inspect("hello"), Err(InspectionError::Unsupported));
        assert!(session.cancellation_handle().is_none());
        let result = session.evaluate("hello").unwrap();
        assert_eq!(result.output, ["hello", "done"]);
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.status, EvaluationStatus::Success);
        let result = session.evaluate("bad").unwrap();
        assert_eq!(result.status, EvaluationStatus::Failed);
        assert_eq!(result.diagnostics[0].message, "invalid input");
        session.reset().unwrap();
        session.close().unwrap();
    }

    #[test]
    fn completion_override_dispatches_read_only_with_source_byte_offsets() {
        struct CompletingSession;
        impl InteractiveLanguageSession for CompletingSession {
            fn language_id(&self) -> &str {
                "mock.completion"
            }
            fn evaluate(&mut self, _: &str) -> Result<EvaluationResult, SessionError> {
                panic!("completion must not evaluate")
            }
            fn reset(&mut self) -> Result<(), SessionError> {
                panic!("completion must not reset")
            }
            fn complete(&self, source: &str, cursor: usize) -> Vec<CompletionItem> {
                assert_eq!(source, "é\nna");
                assert_eq!(cursor, 5);
                vec![CompletionItem {
                    label: "name".into(),
                    replacement: 3..cursor,
                    insert_text: "name".into(),
                    detail: Some("existing binding".into()),
                }]
            }
        }
        let session: &dyn InteractiveLanguageSession = &CompletingSession;
        let items = session.complete("é\nna", 5);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].replacement, 3..5);
        assert_eq!(&"é\nna"[items[0].replacement.clone()], "na");
        assert_eq!(items[0].label, "name");
        assert_eq!(items[0].insert_text, "name");
        assert_eq!(items[0].detail.as_deref(), Some("existing binding"));
        assert_eq!(session.complete("é\nna", 5), items);
    }

    #[test]
    fn invalid_completeness_is_read_only_and_success_can_have_no_output() {
        use std::{cell::Cell, rc::Rc};

        struct LocalSession {
            evaluations: Rc<Cell<usize>>,
        }

        impl InteractiveLanguageSession for LocalSession {
            fn language_id(&self) -> &str {
                "mock.local"
            }

            fn completeness(&self, source: &str) -> Completeness {
                if source == "bad" {
                    Completeness::Invalid {
                        diagnostics: vec![
                            Diagnostic::error(
                                TextRange::new(Position::new(0, 0), Position::new(0, 3)),
                                "invalid syntax",
                            )
                            .with_source("mock.local")
                            .with_code("syntax"),
                        ],
                    }
                } else {
                    Completeness::Complete
                }
            }

            fn evaluate(&mut self, _source: &str) -> Result<EvaluationResult, SessionError> {
                self.evaluations.set(self.evaluations.get() + 1);
                Ok(EvaluationResult {
                    output: Vec::new(),
                    diagnostics: Vec::new(),
                    status: EvaluationStatus::Success,
                })
            }

            fn reset(&mut self) -> Result<(), SessionError> {
                self.evaluations.set(0);
                Ok(())
            }
        }

        let evaluations = Rc::new(Cell::new(0));
        let mut session: Box<dyn InteractiveLanguageSession> = Box::new(LocalSession {
            evaluations: evaluations.clone(),
        });
        let expected = Completeness::Invalid {
            diagnostics: vec![
                Diagnostic::error(
                    TextRange::new(Position::new(0, 0), Position::new(0, 3)),
                    "invalid syntax",
                )
                .with_source("mock.local")
                .with_code("syntax"),
            ],
        };
        let read_only: &dyn InteractiveLanguageSession = session.as_ref();
        assert_eq!(read_only.completeness("bad"), expected.clone());
        assert_eq!(read_only.completeness("bad"), expected);
        assert_eq!(read_only.completeness("valid"), Completeness::Complete);
        assert_eq!(evaluations.get(), 0);

        let result = session.evaluate("valid").unwrap();
        assert_eq!(result.status, EvaluationStatus::Success);
        assert!(result.output.is_empty());
        assert!(result.diagnostics.is_empty());
        assert_eq!(evaluations.get(), 1);
        assert_eq!(session.completeness("bad"), expected);
        assert_eq!(evaluations.get(), 1);
    }

    #[derive(Default)]
    struct Signal {
        state: Mutex<(bool, bool)>,
        changed: Condvar,
    }

    impl CancellationHandle for Signal {
        fn request_cancel(&self) -> Result<(), CancellationError> {
            let mut state = self.state.lock().unwrap();
            if state.0 {
                state.1 = true;
                self.changed.notify_all();
            }
            Ok(())
        }
    }

    struct CancellableSession {
        signal: Arc<Signal>,
        requires_reset: bool,
        poison_on_cancel: bool,
    }

    impl InteractiveLanguageSession for CancellableSession {
        fn language_id(&self) -> &str {
            "mock"
        }

        fn completeness(&self, source: &str) -> Completeness {
            if source.ends_with('(') {
                Completeness::Incomplete
            } else {
                Completeness::Complete
            }
        }

        fn evaluate(&mut self, _source: &str) -> Result<EvaluationResult, SessionError> {
            if self.requires_reset {
                return Err(SessionError::ResetRequired);
            }
            let mut state = self.signal.state.lock().unwrap();
            *state = (true, false);
            self.signal.changed.notify_all();
            while !state.1 {
                state = self.signal.changed.wait(state).unwrap();
            }
            *state = (false, false);
            self.requires_reset = self.poison_on_cancel;
            Ok(EvaluationResult {
                output: vec!["partial output".into()],
                diagnostics: Vec::new(),
                status: EvaluationStatus::Cancelled {
                    requires_reset: self.requires_reset,
                },
            })
        }

        fn reset(&mut self) -> Result<(), SessionError> {
            self.requires_reset = false;
            Ok(())
        }

        fn cancellation_handle(&self) -> Option<Arc<dyn CancellationHandle>> {
            Some(self.signal.clone())
        }
    }

    #[test]
    fn independent_handle_signals_during_sync_evaluation_and_reset_restores_session() {
        for poison_on_cancel in [false, true] {
            let signal = Arc::new(Signal::default());
            let mut session: Box<dyn InteractiveLanguageSession> = Box::new(CancellableSession {
                signal: signal.clone(),
                requires_reset: false,
                poison_on_cancel,
            });
            assert_eq!(session.completeness("("), Completeness::Incomplete);
            assert_eq!(session.completeness("()"), Completeness::Complete);
            for _ in 0..2 {
                let handle = session.cancellation_handle().unwrap();
                std::thread::scope(|threads| {
                    let signal = signal.clone();
                    threads.spawn(move || {
                        let mut state = signal.state.lock().unwrap();
                        while !state.0 {
                            state = signal.changed.wait(state).unwrap();
                        }
                        drop(state);
                        handle.request_cancel().unwrap();
                    });
                    let result = session.evaluate("wait").unwrap();
                    assert_eq!(result.output, ["partial output"]);
                    assert_eq!(
                        result.status,
                        EvaluationStatus::Cancelled {
                            requires_reset: poison_on_cancel,
                        }
                    );
                });
                if poison_on_cancel {
                    assert_eq!(session.evaluate("again"), Err(SessionError::ResetRequired));
                }
                session.reset().unwrap();
            }
        }
    }

    #[test]
    fn errors_preserve_structured_fields_and_implement_standard_error() {
        fn check(error: &dyn std::error::Error, expected: &str) {
            assert_eq!(error.to_string(), expected);
            assert!(error.source().is_none());
        }
        let errors = [
            (
                SessionError::UnsupportedLanguage("other".into()),
                "unsupported language: other",
            ),
            (
                SessionError::Authorization {
                    code: "denied".into(),
                    message: "not allowed".into(),
                },
                "authorization error (denied): not allowed",
            ),
            (
                SessionError::Infrastructure {
                    code: "offline".into(),
                    message: "unavailable".into(),
                },
                "infrastructure error (offline): unavailable",
            ),
            (SessionError::ResetRequired, "session reset required"),
        ];
        for (error, expected) in errors {
            assert_eq!(error.clone(), error);
            check(&error, expected);
        }
        struct Rejected(CancellationError);
        impl CancellationHandle for Rejected {
            fn request_cancel(&self) -> Result<(), CancellationError> {
                Err(self.0.clone())
            }
        }
        for (error, expected) in [
            (CancellationError::Unsupported, "cancellation unsupported"),
            (
                CancellationError::Failed {
                    code: "closed".into(),
                    message: "session closed".into(),
                },
                "cancellation failed (closed): session closed",
            ),
        ] {
            let handle: Arc<dyn CancellationHandle> = Arc::new(Rejected(error.clone()));
            assert_eq!(handle.request_cancel(), Err(error.clone()));
            check(&error, expected);
        }
    }
}
