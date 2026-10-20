//! Exact source context through the public embedding API; no runtime replay.
use scope_ide::{
    app::App, cli, language::InteractiveSession, scratch::ScratchBuffers, workspace::Workspace,
};
use scope_language::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[derive(Default)]
struct State {
    requests: Vec<SourceSnapshot>,
    retained: Vec<SourceSnapshot>,
    closed: bool,
}
impl SourceResolver for State {
    fn resolve(
        &self,
        source: &SourceId,
        revision: RevisionId,
    ) -> Result<SourceSnapshot, SourceUnavailable> {
        self.retained
            .iter()
            .find(|s| &s.source_id == source && s.revision_id == revision)
            .cloned()
            .ok_or(SourceUnavailable::UnknownOrExpired)
    }
}
struct Fake(Rc<RefCell<State>>);
impl InteractiveLanguageSession for Fake {
    fn language_id(&self) -> &str {
        "source.fake"
    }
    fn evaluate(&mut self, _: &str) -> Result<EvaluationResult, SessionError> {
        panic!("rich hook must be used")
    }
    fn evaluate_request(
        &mut self,
        request: EvaluationRequest<'_>,
    ) -> Result<EvaluationResult, SessionError> {
        let mut state = self.0.borrow_mut();
        let snapshot = SourceSnapshot {
            source_id: request.source_id,
            revision_id: request.revision_id,
            text: Arc::from(request.text),
        };
        let origin = if request.text == "invoke" {
            state.retained.first().unwrap().clone()
        } else {
            snapshot.clone()
        };
        let range = TextRange::new(Position::new(0, 0), Position::new(0, 1));
        let mut diagnostic = Diagnostic::error(range, "retained definition");
        diagnostic.locations.push(SourceLocation {
            source_id: origin.source_id,
            revision_id: origin.revision_id,
            range,
        });
        state.requests.push(snapshot.clone());
        state.retained.push(snapshot);
        Ok(EvaluationResult {
            output: vec![],
            diagnostics: vec![diagnostic],
            status: EvaluationStatus::Failed,
        })
    }
    fn reset(&mut self) -> Result<(), SessionError> {
        self.0.borrow_mut().retained.clear();
        Ok(())
    }
    fn close(&mut self) -> Result<(), SessionError> {
        let mut s = self.0.borrow_mut();
        s.retained.clear();
        s.closed = true;
        Ok(())
    }
}
fn installed_session() -> (Rc<RefCell<State>>, InteractiveSession) {
    let state = Rc::new(RefCell::new(State::default()));
    let mut session = InteractiveSession::new();
    session
        .install("source.fake", Box::new(Fake(state.clone())))
        .unwrap();
    (state, session)
}
fn shell(input: &str) -> Rc<RefCell<State>> {
    let (state, session) = installed_session();
    let app = App::builder(Workspace::read_only(env!("CARGO_MANIFEST_DIR")).unwrap())
        .build()
        .unwrap();
    let mut output = Vec::new();
    cli::run_with_session(app, session, input.as_bytes(), &mut output).unwrap();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("[attributed source revision]")
    );
    assert!(state.borrow().closed);
    state
}
#[test]
fn repl_requests_are_exact_distinct_and_isolated_and_eof_closes() {
    let first = shell("  definition  \ninvoke\n");
    let second = shell("  definition  \n");
    let a = first.borrow();
    let b = second.borrow();
    assert_eq!(&*a.requests[0].text, "  definition  ");
    assert_eq!(&*a.requests[1].text, "invoke");
    assert_ne!(a.requests[0].source_id, a.requests[1].source_id);
    assert_ne!(a.requests[0].source_id, b.requests[0].source_id);
    assert_eq!(a.requests[0].revision_id, RevisionId::from_version(0));
    assert_eq!(
        a.resolve(&a.requests[0].source_id, a.requests[0].revision_id),
        Err(SourceUnavailable::UnknownOrExpired)
    );
}
#[test]
fn retained_buffer_diagnostic_resolves_old_text_not_current_revision() {
    let (state, mut session) = installed_session();
    let mut buffers = ScratchBuffers::default();
    buffers.create("source.fake", "d\r\n  exact \n");
    let result = buffers.selected().unwrap().evaluate(&mut session).unwrap();
    let original = result.diagnostics[0].locations[0].clone();
    buffers
        .selected_mut()
        .unwrap()
        .apply_edit(scope_ide::text::TextEdit::insert(
            scope_ide::text::Position::new(0, 1),
            "edited",
        ))
        .unwrap();
    buffers.selected().unwrap().evaluate(&mut session).unwrap();
    let result = session.evaluate("invoke").unwrap();
    assert_eq!(result.diagnostics[0].locations[0], original);
    let s = state.borrow();
    assert_eq!(
        &*s.resolve(&original.source_id, original.revision_id)
            .unwrap()
            .text,
        "d\r\n  exact \n"
    );
    assert_eq!(s.requests[0].source_id, s.requests[1].source_id);
    assert_ne!(s.requests[0].revision_id, s.requests[1].revision_id);
    assert_ne!(s.requests[1].source_id, s.requests[2].source_id);
    assert_eq!(
        s.resolve(&SourceId::fresh(), original.revision_id),
        Err(SourceUnavailable::UnknownOrExpired)
    );
    drop(s);
    session.reset().unwrap();
    assert_eq!(
        state
            .borrow()
            .resolve(&original.source_id, original.revision_id),
        Err(SourceUnavailable::UnknownOrExpired)
    );
    buffers.selected().unwrap().evaluate(&mut session).unwrap();
    let (replacement_state, replacement) = installed_session();
    drop(replacement); // a separate owner cannot resolve this source
    assert_eq!(
        replacement_state
            .borrow()
            .resolve(&original.source_id, original.revision_id),
        Err(SourceUnavailable::UnknownOrExpired)
    );
    session.clear().unwrap();
    assert!(state.borrow().closed);
}
#[test]
fn earlier_repl_provenance_and_multiple_locations_survive_later_requests() {
    let (state, mut controller) = installed_session();
    controller.evaluate("definition").unwrap();
    let first = state.borrow().requests[0].clone();
    let mut result = controller.evaluate("invoke").unwrap();
    let d = &mut result.diagnostics[0];
    assert_eq!(d.locations[0].source_id, first.source_id);
    assert_eq!(d.locations[0].revision_id, first.revision_id);
    let current = state.borrow().requests[1].clone();
    d.locations.push(SourceLocation {
        source_id: current.source_id,
        revision_id: current.revision_id,
        range: d.range,
    });
    assert_eq!(d.locations.len(), 2);
    assert_eq!(
        &*state
            .borrow()
            .resolve(&first.source_id, first.revision_id)
            .unwrap()
            .text,
        "definition"
    );
    let (next, _) = installed_session();
    controller
        .install("source.fake", Box::new(Fake(next)))
        .unwrap();
    assert!(state.borrow().closed);
    assert_eq!(
        state.borrow().resolve(&first.source_id, first.revision_id),
        Err(SourceUnavailable::UnknownOrExpired)
    );
    assert!(
        Diagnostic::error(d.range, "legacy")
            .with_source("producer")
            .locations
            .is_empty()
    );
}
