//! Capability-neutral ownership and routing for optional interactive languages.
//!
//! Cleanup calls `close` best effort, including during controller `Drop` and when
//! discarding a replacement. Providers must ensure their session implementations
//! also release resources through RAII/`Drop`, even when `close` fails.

use std::sync::Arc;

use super::{
    CancellationHandle, Completeness, CompletionItem, EvaluationResult, EvaluationStatus,
    InteractiveLanguageSession, LanguageRegistry, SessionError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRoute<'a> {
    ScopeCommand(&'a str),
    Source(&'a str),
}

#[derive(Default)]
pub struct InteractiveSession {
    session: Option<Box<dyn InteractiveLanguageSession>>,
    reset_required: bool,
}

impl InteractiveSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn language_id(&self) -> Option<&str> {
        self.session.as_ref().map(|session| session.language_id())
    }

    /// Creates and validates a replacement before touching the active session.
    /// If closing the old session fails, it remains selected but may be partially
    /// closed: evaluation requires a successful reset, recreation, or clear.
    pub fn select(&mut self, registry: &LanguageRegistry, id: &str) -> Result<(), SessionError> {
        let provider = registry
            .interactive_provider(id)
            .ok_or_else(|| SessionError::UnsupportedLanguage(id.into()))?;
        self.install(id, provider.create_session()?)
    }

    /// Transfers ownership of a host-created session to this controller.
    ///
    /// Any nonempty language identity is supported without a registry provider.
    /// Identity validation happens before touching the active session. Rejected
    /// candidates are closed best effort and dropped (not returned to the caller).
    /// A valid replacement closes the old session before dropping it and installing
    /// the candidate, including for the same language. If old-session close fails,
    /// the old session remains selected but requires reset; the candidate is closed
    /// best effort and dropped. Close may have partial effects: this is not rollback.
    /// Installation grants no filesystem or agent-tool authority.
    pub fn install(
        &mut self,
        id: &str,
        mut replacement: Box<dyn InteractiveLanguageSession>,
    ) -> Result<(), SessionError> {
        if id.is_empty() || replacement.language_id() != id {
            let error = SessionError::Infrastructure {
                code: "language_id_mismatch".into(),
                message: format!(
                    "requested language {id:?}, candidate session {}",
                    replacement.language_id()
                ),
            };
            let _ = replacement.close();
            return Err(error);
        }
        if let Some(session) = self.session.as_mut()
            && let Err(error) = session.close()
        {
            self.reset_required = true;
            let _ = replacement.close();
            return Err(error);
        }
        self.session = Some(replacement);
        self.reset_required = false;
        Ok(())
    }

    /// Detaches the session even if resource cleanup reports an error.
    pub fn clear(&mut self) -> Result<(), SessionError> {
        self.reset_required = false;
        match self.session.take() {
            Some(mut session) => session.close(),
            None => Ok(()),
        }
    }

    pub fn reset(&mut self) -> Result<(), SessionError> {
        self.session.as_mut().ok_or_else(no_session)?.reset()?;
        self.reset_required = false;
        Ok(())
    }

    pub fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
        self.evaluate_request(scope_language::EvaluationRequest {
            text: source,
            source_id: scope_language::SourceId::fresh(),
            revision_id: scope_language::RevisionId::from_version(0),
        })
    }

    /// Dispatches source context without changing ownership or lifecycle policy.
    pub fn evaluate_request(
        &mut self,
        request: scope_language::EvaluationRequest<'_>,
    ) -> Result<EvaluationResult, SessionError> {
        let session = self.session.as_mut().ok_or_else(no_session)?;
        if self.reset_required {
            return Err(SessionError::ResetRequired);
        }
        let result = match session.evaluate_request(request) {
            Ok(result) => result,
            Err(SessionError::ResetRequired) => {
                self.reset_required = true;
                return Err(SessionError::ResetRequired);
            }
            Err(error) => return Err(error),
        };
        if matches!(
            result.status,
            EvaluationStatus::Cancelled {
                requires_reset: true
            }
        ) {
            self.reset_required = true;
        }
        Ok(result)
    }

    /// Inspects source through the active provider's read-only completeness hook.
    /// Returns `None` when no session is selected. Does not route or evaluate input,
    /// authorize capabilities, or clear a reset requirement; inspection remains
    /// available when evaluation requires reset. Provider purity is a contract,
    /// not a sandbox guarantee (see `InteractiveLanguageSession::completeness`).
    pub fn completeness(&self, source: &str) -> Option<Completeness> {
        self.session
            .as_ref()
            .map(|session| session.completeness(source))
    }

    /// Routes completion to Scope commands first, then the active language.
    /// Command context (even with no suggestions) never falls back to language.
    /// Invalid cursors return no suggestions; invalid provider edits are dropped.
    /// Inspection is read-only and remains available when evaluation needs reset.
    pub fn complete(&self, source: &str, cursor: usize) -> Vec<CompletionItem> {
        if !source.is_char_boundary(cursor) {
            return Vec::new();
        }
        let items = crate::cli::complete_commands(source, cursor, self.session.is_some())
            .unwrap_or_else(|| {
                self.session
                    .as_ref()
                    .map(|session| session.complete(source, cursor))
                    .unwrap_or_default()
            });
        items
            .into_iter()
            .filter(|item| {
                let range = &item.replacement;
                range.start <= range.end
                    && source.is_char_boundary(range.start)
                    && source.is_char_boundary(range.end)
            })
            .collect()
    }

    /// Read-only session lookup; never falls back to evaluation or filesystem analysis.
    /// Does not change reset requirements or authorize any capabilities.
    pub fn inspect(
        &self,
        name: &str,
    ) -> Result<Option<super::SymbolDescription>, super::InspectionError> {
        self.session
            .as_ref()
            .ok_or(super::InspectionError::NoSession)?
            .inspect(name)
    }

    pub fn cancellation_handle(&self) -> Option<Arc<dyn CancellationHandle>> {
        self.session
            .as_ref()
            .and_then(|session| session.cancellation_handle())
    }

    /// Explicit Scope commands work in either mode; bare shell input retains legacy parsing.
    pub fn route<'a>(&self, input: &'a str) -> InputRoute<'a> {
        if input == ":scope" {
            InputRoute::ScopeCommand("")
        } else if let Some(command) = input.strip_prefix(":scope ") {
            InputRoute::ScopeCommand(command)
        } else if self.session.is_some() {
            InputRoute::Source(input)
        } else {
            InputRoute::ScopeCommand(input)
        }
    }
}

impl Drop for InteractiveSession {
    fn drop(&mut self) {
        let _ = self.clear();
    }
}

fn no_session() -> SessionError {
    SessionError::Infrastructure {
        code: "no_interactive_session".into(),
        message: "no interactive language selected".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::InteractiveSessionProvider;
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct State {
        events: Vec<&'static str>,
        fail_create: bool,
        mismatch: bool,
        fail_close: bool,
        fail_reset: bool,
    }
    struct Provider(Mutex<State>);
    struct Mock(&'static Provider);

    impl InteractiveSessionProvider for &'static Provider {
        fn language_id(&self) -> &'static str {
            "mock"
        }
        fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
            let mut state = self.0.lock().unwrap();
            state.events.push("create");
            if state.fail_create {
                return Err(no_session());
            }
            Ok(Box::new(Mock(self)))
        }
    }
    impl InteractiveLanguageSession for Mock {
        fn language_id(&self) -> &str {
            if self.0.0.lock().unwrap().mismatch {
                "wrong"
            } else {
                "mock"
            }
        }
        fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
            self.0.0.lock().unwrap().events.push("evaluate");
            if source == "reset_required" {
                return Err(SessionError::ResetRequired);
            }
            Ok(EvaluationResult {
                output: vec![source.into()],
                diagnostics: vec![],
                status: match source {
                    "poison" => EvaluationStatus::Cancelled {
                        requires_reset: true,
                    },
                    "cancel" => EvaluationStatus::Cancelled {
                        requires_reset: false,
                    },
                    _ => EvaluationStatus::Success,
                },
            })
        }
        fn reset(&mut self) -> Result<(), SessionError> {
            let mut state = self.0.0.lock().unwrap();
            state.events.push("reset");
            if state.fail_reset {
                Err(no_session())
            } else {
                Ok(())
            }
        }
        fn close(&mut self) -> Result<(), SessionError> {
            let mut state = self.0.0.lock().unwrap();
            state.events.push("close");
            if state.fail_close {
                Err(no_session())
            } else {
                Ok(())
            }
        }
    }
    impl Drop for Mock {
        fn drop(&mut self) {
            self.0.0.lock().unwrap().events.push("drop");
        }
    }
    fn setup() -> (&'static Provider, LanguageRegistry) {
        let provider = Box::leak(Box::new(Provider(Mutex::new(State::default()))));
        let reference = Box::leak(Box::new(&*provider));
        (
            provider,
            LanguageRegistry::empty().with_interactive_provider(reference),
        )
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct InspectionState {
        events: Vec<&'static str>,
        dictionary: Vec<String>,
        stack: Vec<String>,
        grants: Vec<&'static str>,
        capability_requests: usize,
    }

    struct InspectingProvider(Mutex<InspectionState>);
    struct InspectingMock(&'static InspectingProvider);

    impl InteractiveSessionProvider for &'static InspectingProvider {
        fn language_id(&self) -> &'static str {
            "mock.inspect"
        }

        fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
            self.0.lock().unwrap().events.push("create");
            Ok(Box::new(InspectingMock(self)))
        }
    }

    fn syntax_diagnostic() -> super::super::Diagnostic {
        use super::super::{Diagnostic, Position, TextRange};
        Diagnostic::error(
            TextRange::new(Position::new(0, 0), Position::new(0, 3)),
            "invalid syntax",
        )
        .with_source("mock.inspect")
        .with_code("syntax")
    }

    impl InteractiveLanguageSession for InspectingMock {
        fn language_id(&self) -> &str {
            "mock.inspect"
        }

        fn complete(&self, source: &str, cursor: usize) -> Vec<CompletionItem> {
            assert!(source.is_char_boundary(cursor));
            let item = |replacement| CompletionItem {
                label: source.into(),
                replacement,
                insert_text: "suggested".into(),
                detail: Some(format!("cursor {cursor}")),
            };
            vec![
                item(0..cursor),
                item(cursor..source.len()),
                item(cursor..cursor),
                // Deliberately malformed replacement span exercises completion filtering.
                item(std::ops::Range { start: 1, end: 0 }),
                item(0..source.len() + 1),
                item(usize::MAX..usize::MAX),
                item(1..1),
            ]
        }

        fn completeness(&self, source: &str) -> Completeness {
            match source {
                "open" => Completeness::Incomplete,
                "bad" => Completeness::Invalid {
                    diagnostics: vec![syntax_diagnostic()],
                },
                "bad without diagnostic" => Completeness::Invalid {
                    diagnostics: vec![],
                },
                _ => Completeness::Complete,
            }
        }

        fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
            let mut state = self.0.0.lock().unwrap();
            state.events.push("evaluate");
            state.dictionary.push(source.into());
            state.stack.push(source.into());
            Ok(EvaluationResult {
                output: vec![source.into()],
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
            let mut state = self.0.0.lock().unwrap();
            state.events.push("reset");
            state.dictionary.clear();
            state.stack.clear();
            Ok(())
        }

        fn close(&mut self) -> Result<(), SessionError> {
            self.0.0.lock().unwrap().events.push("close");
            Ok(())
        }
    }

    #[test]
    fn completeness_preserves_state_grants_routing_and_lifecycle() {
        let provider = Box::leak(Box::new(InspectingProvider(Mutex::new(InspectionState {
            grants: vec!["existing-grant"],
            ..InspectionState::default()
        }))));
        let reference = Box::leak(Box::new(&*provider));
        let registry = LanguageRegistry::empty().with_interactive_provider(reference);
        let mut controller = InteractiveSession::new();
        assert_eq!(controller.completeness("open"), None);
        controller.select(&registry, "mock.inspect").unwrap();
        controller.evaluate("seed").unwrap();

        for poisoned in [false, true] {
            if poisoned {
                controller.evaluate("poison").unwrap();
            }
            let before = provider.0.lock().unwrap().clone();
            let read_only = &controller;
            for (source, expected) in [
                ("ready", Completeness::Complete),
                ("open", Completeness::Incomplete),
                (
                    "bad",
                    Completeness::Invalid {
                        diagnostics: vec![syntax_diagnostic()],
                    },
                ),
                (
                    "bad without diagnostic",
                    Completeness::Invalid {
                        diagnostics: vec![],
                    },
                ),
            ] {
                assert_eq!(read_only.completeness(source), Some(expected.clone()));
                assert_eq!(read_only.completeness(source), Some(expected));
                assert_eq!(read_only.route(source), InputRoute::Source(source));
            }
            assert_eq!(read_only.language_id(), Some("mock.inspect"));
            assert_eq!(
                read_only.route(":scope help"),
                InputRoute::ScopeCommand("help")
            );
            assert_eq!(*provider.0.lock().unwrap(), before);
            if poisoned {
                assert_eq!(
                    controller.evaluate("ready"),
                    Err(SessionError::ResetRequired)
                );
                assert_eq!(*provider.0.lock().unwrap(), before);
            } else {
                // Completeness remains advisory; evaluation does not invoke or gate on it.
                assert_eq!(controller.evaluate("open").unwrap().output, ["open"]);
            }
        }
        controller.reset().unwrap();
        assert_eq!(
            controller.evaluate("after reset").unwrap().output,
            ["after reset"]
        );
        controller.clear().unwrap();
        assert_eq!(controller.completeness("ready"), None);
        drop(controller);
        let state = provider.0.lock().unwrap();
        assert_eq!(
            state.events,
            [
                "create", "evaluate", "evaluate", "evaluate", "reset", "evaluate", "close"
            ]
        );
        assert_eq!(state.dictionary, ["after reset"]);
        assert_eq!(state.stack, ["after reset"]);
        assert_eq!(state.grants, ["existing-grant"]);
        assert_eq!(state.capability_requests, 0);
    }

    #[test]
    fn completion_routes_commands_and_preserves_read_only_session_state() {
        let provider = Box::leak(Box::new(InspectingProvider(Mutex::new(InspectionState {
            grants: vec!["existing-grant"],
            ..InspectionState::default()
        }))));
        let reference = Box::leak(Box::new(&*provider));
        let registry = LanguageRegistry::empty().with_interactive_provider(reference);
        let mut controller = InteractiveSession::new();
        assert!(controller.complete("é\nname", 7).is_empty());
        assert_eq!(
            controller.complete("he", 2),
            crate::cli::complete_commands("he", 2, false).unwrap()
        );
        controller.select(&registry, "mock.inspect").unwrap();
        controller.evaluate("seed").unwrap();

        for poisoned in [false, true] {
            if poisoned {
                controller.evaluate("poison").unwrap();
            }
            let before = provider.0.lock().unwrap().clone();
            let source = "é\nname";
            for cursor in [0, 2, 3, source.len()] {
                let items = controller.complete(source, cursor);
                assert_eq!(items.len(), 3);
                assert_eq!(items[0].label, source);
                assert_eq!(items[0].replacement, 0..cursor);
                assert_eq!(items[1].replacement, cursor..source.len());
                assert_eq!(items[2].replacement, cursor..cursor);
                assert_eq!(items[0].insert_text, "suggested");
                assert_eq!(items[0].detail, Some(format!("cursor {cursor}")));
            }
            for cursor in [1, source.len() + 1, usize::MAX] {
                assert!(controller.complete(source, cursor).is_empty());
            }
            for command in [":scope he", ":scope unknown-command", ":scope "] {
                let expected = crate::cli::complete_commands(command, command.len(), true)
                    .expect("explicit Scope command context");
                assert_eq!(controller.complete(command, command.len()), expected);
            }
            assert!(controller.complete(":scope unknown-command", 22).is_empty());
            assert_eq!(*provider.0.lock().unwrap(), before);
            assert_eq!(controller.reset_required, poisoned);
            if poisoned {
                assert_eq!(
                    controller.evaluate("ready"),
                    Err(SessionError::ResetRequired)
                );
                assert_eq!(*provider.0.lock().unwrap(), before);
            }
        }
    }

    #[test]
    fn default_completion_is_optional_and_never_evaluates() {
        let (provider, registry) = setup();
        let mut controller = InteractiveSession::new();
        controller.select(&registry, "mock").unwrap();
        for source in ["", "é", "first\nsecond"] {
            assert!(controller.complete(source, source.len()).is_empty());
        }
        assert_eq!(provider.0.lock().unwrap().events, ["create"]);
    }

    #[test]
    fn default_completeness_does_not_evaluate_or_change_session() {
        let (provider, registry) = setup();
        let mut controller = InteractiveSession::new();
        controller.select(&registry, "mock").unwrap();
        for source in ["", "open", "bad"] {
            assert_eq!(
                controller.completeness(source),
                Some(Completeness::Complete)
            );
        }
        assert_eq!(provider.0.lock().unwrap().events, ["create"]);
        assert_eq!(controller.evaluate("bad").unwrap().output, ["bad"]);
        controller.reset().unwrap();
        controller.clear().unwrap();
        assert_eq!(
            provider.0.lock().unwrap().events,
            ["create", "evaluate", "reset", "close", "drop"]
        );
    }

    #[test]
    fn selection_and_shutdown_lifecycle() {
        let (provider, registry) = setup();
        assert!(
            LanguageRegistry::default()
                .interactive_providers()
                .is_empty()
        );
        assert!(
            LanguageRegistry::empty()
                .interactive_provider("mock")
                .is_none()
        );
        assert_eq!(registry.interactive_providers().len(), 1);
        let mut controller = InteractiveSession::new();
        controller.select(&registry, "mock").unwrap();
        assert_eq!(
            controller.select(&registry, "unknown"),
            Err(SessionError::UnsupportedLanguage("unknown".into()))
        );
        provider.0.lock().unwrap().fail_create = true;
        assert!(controller.select(&registry, "mock").is_err());
        assert_eq!(controller.language_id(), Some("mock"));
        provider.0.lock().unwrap().fail_create = false;
        controller.select(&registry, "mock").unwrap();
        drop(controller);
        assert_eq!(
            provider.0.lock().unwrap().events,
            [
                "create", "create", "create", "close", "drop", "close", "drop"
            ]
        );
    }

    #[test]
    fn mismatch_and_close_failure_cleanup_replacements() {
        let (provider, registry) = setup();
        let mut controller = InteractiveSession::new();
        controller.select(&registry, "mock").unwrap();
        provider.0.lock().unwrap().mismatch = true;
        assert!(
            matches!(controller.select(&registry, "mock"), Err(SessionError::Infrastructure { code, .. }) if code == "language_id_mismatch")
        );
        provider.0.lock().unwrap().mismatch = false;
        assert_eq!(controller.language_id(), Some("mock"));
        provider.0.lock().unwrap().fail_close = true;
        assert!(controller.select(&registry, "mock").is_err());
        assert_eq!(controller.language_id(), Some("mock"));
        assert_eq!(controller.evaluate("x"), Err(SessionError::ResetRequired));
        assert!(controller.clear().is_err());
        assert_eq!(controller.language_id(), None);
        drop(controller);
        assert_eq!(
            provider.0.lock().unwrap().events,
            [
                "create", "create", "close", "drop", "create", "close", "close", "drop", "close",
                "drop"
            ]
        );
    }

    #[test]
    fn poison_requires_successful_reset() {
        let (provider, registry) = setup();
        let mut controller = InteractiveSession::default();
        assert_eq!(controller.evaluate("x"), Err(no_session()));
        assert_eq!(controller.reset(), Err(no_session()));
        assert_eq!(controller.completeness("x"), None);
        assert!(controller.cancellation_handle().is_none());
        controller.select(&registry, "mock").unwrap();
        assert_eq!(controller.completeness("x"), Some(Completeness::Complete));
        assert!(controller.cancellation_handle().is_none());
        controller.evaluate("cancel").unwrap();
        controller.evaluate("poison").unwrap();
        assert_eq!(controller.evaluate("x"), Err(SessionError::ResetRequired));
        provider.0.lock().unwrap().fail_reset = true;
        assert!(controller.reset().is_err());
        assert_eq!(controller.evaluate("x"), Err(SessionError::ResetRequired));
        provider.0.lock().unwrap().fail_reset = false;
        controller.reset().unwrap();
        assert_eq!(controller.evaluate("x").unwrap().output, ["x"]);
        controller.evaluate("poison").unwrap();
        controller.select(&registry, "mock").unwrap();
        controller.evaluate("x").unwrap();
    }

    #[test]
    fn reset_required_error_poison_is_enforced_by_controller() {
        let (provider, registry) = setup();
        let mut controller = InteractiveSession::new();
        controller.select(&registry, "mock").unwrap();
        assert_eq!(
            controller.evaluate("reset_required"),
            Err(SessionError::ResetRequired)
        );
        assert_eq!(controller.evaluate("x"), Err(SessionError::ResetRequired));
        assert_eq!(provider.0.lock().unwrap().events, ["create", "evaluate"]);
        provider.0.lock().unwrap().fail_reset = true;
        assert!(controller.reset().is_err());
        assert_eq!(controller.evaluate("x"), Err(SessionError::ResetRequired));
        provider.0.lock().unwrap().fail_reset = false;
        controller.reset().unwrap();
        controller.evaluate("x").unwrap();
    }

    #[test]
    fn close_failure_poison_requires_recovery() {
        let (provider, registry) = setup();
        let mut controller = InteractiveSession::new();
        controller.select(&registry, "mock").unwrap();
        for recreate in [false, true] {
            provider.0.lock().unwrap().fail_close = true;
            assert!(controller.select(&registry, "mock").is_err());
            assert_eq!(controller.evaluate("x"), Err(SessionError::ResetRequired));
            provider.0.lock().unwrap().fail_reset = true;
            assert!(controller.reset().is_err());
            assert_eq!(controller.evaluate("x"), Err(SessionError::ResetRequired));
            provider.0.lock().unwrap().fail_reset = false;
            provider.0.lock().unwrap().fail_close = false;
            if recreate {
                controller.select(&registry, "mock").unwrap();
            } else {
                controller.reset().unwrap();
            }
            controller.evaluate("x").unwrap();
        }
    }

    #[test]
    fn routing_preserves_source_and_legacy_shell_commands() {
        let (_, registry) = setup();
        let mut controller = InteractiveSession::new();
        assert_eq!(
            controller.route("language doc"),
            InputRoute::ScopeCommand("language doc")
        );
        assert_eq!(
            controller.route(":scope help"),
            InputRoute::ScopeCommand("help")
        );
        controller.select(&registry, "mock").unwrap();
        for source in [
            ": square dup * ;",
            "language doc",
            "",
            ":scopex",
            " :scope help",
            ":scope\thelp",
        ] {
            assert_eq!(controller.route(source), InputRoute::Source(source));
        }
        assert_eq!(controller.route(":scope"), InputRoute::ScopeCommand(""));
        assert_eq!(
            controller.route(":scope language doc"),
            InputRoute::ScopeCommand("language doc")
        );
    }
}
