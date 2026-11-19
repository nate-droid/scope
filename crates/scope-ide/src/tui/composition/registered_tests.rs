//! Real UI ownership/review boundaries with a deterministic discovery/provider fixture.
use super::*;
use crate::{
    registered_document_closure::*,
    test_support::TempWorkspace,
    text::{Position, TextEdit},
    tool::semantic::{HostAuthorizationRequest, HostSemanticPolicy},
    tui::{EditableScratchPane, RegisteredExecution, RegisteredOutput},
};
use scope_language::{
    SemanticOperationDescription, SemanticOperationEffect, SemanticTargetRequirement,
};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
#[derive(Default)]
struct Counter(AtomicU32);
impl Counter {
    fn get(&self) -> u32 {
        self.0.load(Ordering::SeqCst)
    }
    fn set(&self, value: u32) {
        self.0.store(value, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct Counts {
    discovery: Counter,
    policy: Counter,
    dispatch: Counter,
    format: Counter,
}
struct GraphDiscovery(Arc<Counts>);
impl Discovery for GraphDiscovery {
    fn capture_with_diagnostics(
        &self,
        root: &str,
        sources: Vec<RegisteredSource>,
        limits: Limits,
    ) -> Result<DiscoveredClosure, DiscoveryFailure> {
        let diagnostic = sources.iter().find_map(|source| {
            let start = source.text.find("invalid")?;
            Some(DiscoveryDiagnostic {
                module: source.module.clone(),
                path: None,
                start,
                end: start + "invalid".len(),
                line: 1,
                column: source.text[..start].chars().count() + 1,
                message: "unknown declaration invalid".into(),
            })
        });
        self.capture(root, sources, limits)
            .map_err(|error| DiscoveryFailure {
                error,
                diagnostics: diagnostic.into_iter().collect(),
            })
    }
    fn capture(
        &self,
        root: &str,
        sources: Vec<RegisteredSource>,
        limits: Limits,
    ) -> Result<DiscoveredClosure, DiscoveryError> {
        self.0.discovery.set(self.0.discovery.get() + 1);
        if !sources.iter().any(|source| source.module == root)
            || sources.iter().any(|source| source.text.contains("invalid"))
        {
            return Err(DiscoveryError {
                phase: DiscoveryPhase::Acquisition,
                category: DiscoveryCategory::MissingSource,
            });
        }
        let sources: Vec<_> = sources
            .into_iter()
            .filter(|source| source.module == root || (root == "root" && source.module == "dep"))
            .collect();
        if root == "root" && sources.len() != 2 {
            return Err(DiscoveryError {
                phase: DiscoveryPhase::Acquisition,
                category: DiscoveryCategory::MissingSource,
            });
        }
        let graph = sources
            .iter()
            .map(|source| {
                (
                    source.module.clone(),
                    if source.module == "root" {
                        vec!["dep".into()]
                    } else {
                        vec![]
                    },
                )
            })
            .collect();
        let exports = sources
            .iter()
            .map(|source| (source.module.clone(), vec![]))
            .collect();
        let compiler_sources = sources
            .iter()
            .rev()
            .map(|source| source.identity.clone())
            .collect();
        // This fixture identifies its complete source table; it is not a language compiler.
        let portable_identity = sources
            .iter()
            .flat_map(|source| {
                format!("{}:{}:{}", source.identity, source.revision, source.text).into_bytes()
            })
            .collect();
        DiscoveredClosure::new(DiscoveryEvidence {
            root: root.into(),
            sources,
            graph,
            exports,
            compiler_sources,
            compiler_identity: "ui-graph-fixture-v1".into(),
            identity_version: 1,
            portable_identity,
            limits,
        })
    }
}
struct Policy {
    counts: Arc<Counts>,
    allow: bool,
}
impl HostSemanticPolicy for Policy {
    fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
        self.counts.policy.set(self.counts.policy.get() + 1);
        let HostAuthorizationRequest::Closure { binding, review } = request else {
            panic!("single-source authorization is not a closure grant")
        };
        assert_eq!(binding.review().unwrap().as_str(), review.as_str());
        assert_eq!(binding.snapshot().sources.len(), 2);
        assert!(
            binding
                .snapshot()
                .sources
                .iter()
                .any(|source| source.module == "dep" && source.text == "dep body")
        );
        assert!(review.as_str().contains("\"module\":\"dep\""));
        assert!(review.as_str().contains("prepared-sha256"));
        self.allow
    }
}
type Provider = PreparedProvider<Arc<Counts>, String>;
struct Harness {
    root: TempWorkspace,
    app: crate::app::App<Provider>,
    ui: WorkspaceTui<Provider>,
    sources: crate::scratch::ScratchBuffers,
    counts: Arc<Counts>,
}
impl Harness {
    fn new(allow: bool) -> Self {
        Self::with_review_requirement(allow, None)
    }

    fn with_review_requirement(
        allow: bool,
        requirement: Option<
            fn(&crate::closure_inspection::ClosureInspectionBinding) -> Option<String>,
        >,
    ) -> Self {
        let root = TempWorkspace::new("registered-ui");
        root.write_file("main.sprout", "root body");
        root.write_file("dep.sprout", "dep body");
        let counts = Arc::new(Counts::default());
        let mut app = App::new(root.workspace()).unwrap();
        let operation = SemanticOperationDescription {
            id: "evaluate-bundle-v1".into(),
            description: "Registered UI fixture".into(),
            arguments: vec![],
            effect: SemanticOperationEffect::Read,
            target: SemanticTargetRequirement::Document,
            constraints: None,
        };
        assert!(
            app.enable_registered_session(RegisteredDocuments::new(
                GraphDiscovery(counts.clone()),
                TrustedLoadedArtifact::default(),
                vec![operation],
                serde_json::json!({"mode":"bounded"}),
                Limits::default(),
                10,
            ))
            .is_ok()
        );
        for (path, module) in [("main.sprout", "root"), ("dep.sprout", "dep")] {
            app.open_path(path).unwrap();
            app.enroll_open_document(module.into(), "sprout".into())
                .unwrap();
        }
        app.select_registered_document("root").unwrap();
        let app = app
            .try_with_prepared_provider(
                PreparedImplementationDescriptor {
                    sha256: "ab".repeat(32),
                    abi: "ui-test".into(),
                    execution_policy: serde_json::json!({"imports":false}),
                },
                counts.clone(),
                |counts, request| {
                    counts.dispatch.set(counts.dispatch.get() + 1);
                    assert_eq!(request.closure().root(), "root");
                    assert_eq!(request.binding().snapshot().sources.len(), 2);
                    Ok("42".to_owned())
                },
            )
            .unwrap();
        let format_counts = counts.clone();
        let mut ui = WorkspaceTui::<Provider>::for_provider();
        ui.register_editable("editor", EditableScratchPane::new())
            .unwrap();
        ui.layout("main", View::leaf("editor")).unwrap();
        ui.geometry(Rect::new(0, 0, 120, 32));
        let execution = RegisteredExecution::new(
            "evaluate-bundle-v1",
            move |result: &RegisteredClosureResult<Result<String, ProviderFailure>>| {
                format_counts.format.set(format_counts.format.get() + 1);
                RegisteredOutput {
                    summary: format!("Value: {}", result.output().as_ref().unwrap()),
                    details: format!(
                        "Retained output: {}; original dep body",
                        result.output().as_ref().unwrap()
                    ),
                }
            },
        );
        let policy = Policy {
            counts: counts.clone(),
            allow,
        };
        let execution = match requirement {
            Some(requirement) => execution.with_policy_authorized_run(policy, requirement),
            None => execution.with_policy(policy),
        };
        ui.set_registered_execution(Some(execution)).unwrap();
        let mut h = Self {
            root,
            app,
            ui,
            sources: Default::default(),
            counts,
        };
        h.ui.invoke_action(
            WorkspaceAction::FocusAppDocument,
            &mut h.app,
            &mut h.sources,
            None,
        )
        .unwrap();
        h
    }
    fn action(&mut self, action: WorkspaceAction) {
        self.ui
            .invoke_action(action, &mut self.app, &mut self.sources, None)
            .unwrap();
    }
    fn key(&mut self, code: KeyCode) {
        self.ui.handle_event_with_sources(
            Event::Key(KeyEvent::new(code, event::KeyModifiers::NONE)),
            &mut self.app,
            &mut self.sources,
        );
    }
    fn screen(&mut self) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(150, 40)).unwrap();
        terminal
            .draw(|frame| {
                self.ui
                    .render_run_frame(frame, &self.app, Some(&self.sources), false)
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }
    fn edit_dep(&mut self, prefix: &str) {
        self.app
            .registered_documents_mut()
            .unwrap()
            .document_mut("dep")
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), prefix))
            .unwrap();
    }
}

fn native_feedback(h: &mut Harness, fail: bool) {
    h.ui.set_registered_execution(Some(
        RegisteredExecution::<Result<String, ProviderFailure>>::new("evaluate-bundle-v1", |_| {
            RegisteredOutput {
                summary: "Checked fixture".into(),
                details: "Original typed completion".into(),
            }
        })
        .with_invocation_contract(InvocationContract {
            schema_version: "fixture-v1".into(),
            entry: "fixture_entry".into(),
            output: "fixture-output".into(),
            effect: "read-v1".into(),
        })
        .with_native_feedback(move |completion| {
            assert_eq!(
                completion.binding().snapshot().compilation_options["invocation_contract"]["entry"],
                "fixture_entry"
            );
            if fail {
                return Err("unsupported checked return".into());
            }
            let value = completion.output().as_ref().map_err(|e| format!("{e:?}"))?;
            Ok(scope_language::SemanticOperationResult::ReadOnly {
                value: serde_json::json!({"computed": value.parse::<i32>().unwrap(), "flag": true}),
                rationale: "typed fixture output".into(),
            })
        }),
    ))
    .unwrap();
}

#[test]
fn registered_native_completion_enters_shared_slots_and_navigation_is_passive() {
    use crate::semantic_inspection::{InspectionFreshness, InspectionOrigin};
    let mut h = Harness::new(true);
    native_feedback(&mut h, false);
    h.action(WorkspaceAction::RunCurrentDocument);
    h.key(KeyCode::Enter);
    let first = h.ui.inspection_result().unwrap().clone();
    assert_eq!(first.freshness_app(&h.app), InspectionFreshness::Current);
    let InspectionOrigin::Closure(origin) = &first.origin else {
        panic!("closure origin")
    };
    assert!(format!("{origin:?}").contains(&"ab".repeat(32)));
    h.edit_dep("changed ");
    assert_eq!(first.freshness_app(&h.app), InspectionFreshness::Stale);
    h.action(WorkspaceAction::RunCurrentDocument);
    h.key(KeyCode::Enter);
    assert_eq!(
        h.ui.inspection_result().unwrap().freshness_app(&h.app),
        InspectionFreshness::Current
    );
    let retained = h.ui.retained_inspection.as_ref().unwrap();
    assert!(retained.comparison_disabled().is_none());
    let before = (
        h.counts.discovery.get(),
        h.counts.policy.get(),
        h.counts.dispatch.get(),
    );
    for action in [
        WorkspaceAction::InspectResult,
        WorkspaceAction::CompareResults,
        WorkspaceAction::ViewDetails,
    ] {
        h.action(action);
        let _ = h.screen();
        h.key(KeyCode::Down);
        h.key(KeyCode::Enter);
        h.key(KeyCode::Esc);
    }
    assert_eq!(
        before,
        (
            h.counts.discovery.get(),
            h.counts.policy.get(),
            h.counts.dispatch.get()
        )
    );
    assert_eq!(h.counts.dispatch.get(), 2);
    assert_eq!(first.freshness_app(&h.app), InspectionFreshness::Stale);
    let unknown_app = App::new(h.root.workspace()).unwrap();
    assert_eq!(
        first.freshness_app(&unknown_app),
        InspectionFreshness::Unknown
    );
}

#[cfg(feature = "desktop")]
#[test]
fn desktop_tabs_preserve_registered_observation_and_dependency_ownership() {
    use crate::semantic_inspection::InspectionFreshness;
    let mut h = Harness::new(true);
    native_feedback(&mut h, false);
    h.ui.enable_desktop_tabs(&h.app);
    h.action(WorkspaceAction::RunCurrentDocument);
    h.key(KeyCode::Enter);
    h.key(KeyCode::Esc);
    h.ui.open_app_document(&mut h.app, "dep.sprout").unwrap();
    assert_eq!(h.app.selected_registered_module(), Some("dep"));
    // run_optional_sources records newly shown documents when it draws a frame.
    h.ui.desktop_tabs
        .as_mut()
        .unwrap()
        .push("dep.sprout".into());
    assert_eq!(
        h.ui.inspection_result().unwrap().freshness_app(&h.app),
        InspectionFreshness::Current
    );
    assert_eq!(
        h.ui.registered_result().unwrap().discovered().root(),
        "root"
    );
    h.ui.close_desktop_tab(&mut h.app, "dep.sprout").unwrap();
    assert_eq!(h.app.selected_registered_module(), Some("root"));
    assert!(
        h.app
            .registered_documents()
            .unwrap()
            .document("dep")
            .is_some()
    );
    assert_eq!(
        h.ui.inspection_result().unwrap().freshness_app(&h.app),
        InspectionFreshness::Current
    );
    assert_eq!(h.counts.dispatch.get(), 1);
}

#[test]
fn registered_projection_failure_retains_evidence_without_native_data() {
    let mut h = Harness::new(true);
    native_feedback(&mut h, true);
    h.action(WorkspaceAction::RunCurrentDocument);
    h.key(KeyCode::Enter);
    let result = h.ui.inspection_result().unwrap();
    assert!(result.native_result.is_none());
    assert!(format!("{:?}", result.result).contains("unsupported checked return"));
    assert!(matches!(
        result.origin,
        crate::semantic_inspection::InspectionOrigin::Closure(_)
    ));
    assert_eq!(h.counts.dispatch.get(), 1);
}

#[test]
fn registered_check_publishes_and_discovers_without_authorization_or_dispatch() {
    let mut h = Harness::new(false);
    h.ui.handle_event_with_sources(Event::Paste("unsaved ".into()), &mut h.app, &mut h.sources);
    let identity = h
        .app
        .open_document()
        .unwrap()
        .evaluation_source_id()
        .clone();
    h.action(WorkspaceAction::CheckCurrentDocument);
    assert_eq!(h.app.open_document().unwrap().text(), "unsaved root body");
    assert_eq!(
        h.app.open_document().unwrap().evaluation_source_id(),
        &identity
    );
    assert_eq!(h.root.read_file("main.sprout"), "root body");
    assert_eq!(h.counts.discovery.get(), 1);
    assert_eq!(h.counts.policy.get(), 0);
    assert_eq!(h.counts.dispatch.get(), 0);
    assert!(!h.ui.registered_modal_open());
    assert!(h.screen().contains("Closure valid"));
    assert!(h.ui.registered_result().is_none());
    h.action(WorkspaceAction::InspectDocument);
    assert!(h.screen().contains("\"module\":\"dep\""));
    h.key(KeyCode::Enter);
    assert_eq!(h.counts.discovery.get(), 1);
    assert_eq!(h.counts.policy.get(), 0);
    assert_eq!(h.counts.dispatch.get(), 0);
    h.key(KeyCode::Esc);
    h.edit_dep("invalid ");
    h.action(WorkspaceAction::CheckCurrentDocument);
    let failed = h.screen();
    assert!(failed.contains("Closure capture rejected"));
    assert!(failed.contains("dep.sprout:1:1: unknown declaration invalid"));
    assert!(!failed.contains("Diagnostics {"));
    assert_eq!(h.counts.policy.get(), 0);
    assert_eq!(h.counts.dispatch.get(), 0);
}

#[test]
fn registered_same_run_action_publishes_reviews_and_consumes_exact_selection_once() {
    let mut h = Harness::new(true);
    h.ui.handle_event_with_sources(Event::Paste("unsaved ".into()), &mut h.app, &mut h.sources);
    h.action(WorkspaceAction::RunCurrentDocument);
    assert_eq!(
        h.ui.primary_f5_action(event::KeyModifiers::NONE, &h.sources),
        Some(WorkspaceAction::RunCurrentDocument)
    );
    assert_eq!(
        h.ui.primary_f5_action(event::KeyModifiers::SHIFT, &h.sources),
        Some(WorkspaceAction::CheckCurrentDocument)
    );
    assert!(h.ui.registered_modal_open());
    let screen = h.screen();
    assert!(screen.contains("Run registered closure?"));
    assert!(screen.contains("Approval required"));
    assert!(!screen.contains("\"module\":\"dep\""));
    h.key(KeyCode::Char('d'));
    let screen = h.screen();
    assert!(screen.contains("unsaved root body"));
    assert!(screen.contains("\"module\":\"dep\""));
    assert_eq!(
        (
            h.counts.discovery.get(),
            h.counts.policy.get(),
            h.counts.dispatch.get()
        ),
        (1, 0, 0)
    );
    assert_eq!(
        h.ui.actions(&h.sources, false)
            .iter()
            .filter(|item| item.label == "Run")
            .count(),
        1
    );
    h.key(KeyCode::Enter);
    assert_eq!(
        (
            h.counts.discovery.get(),
            h.counts.policy.get(),
            h.counts.dispatch.get()
        ),
        (1, 1, 1)
    );
    assert_eq!(
        h.ui.registered_result().unwrap().output().as_ref().unwrap(),
        "42"
    );
    h.key(KeyCode::Enter);
    assert_eq!(
        (
            h.counts.discovery.get(),
            h.counts.policy.get(),
            h.counts.dispatch.get()
        ),
        (1, 1, 1)
    );
    assert_eq!(h.counts.format.get(), 1);
    assert!(!h.ui.registered_modal_open());
    assert_eq!(h.ui.focused(), Some("output"));
    assert!(h.screen().contains("Value: 42"));
    h.key(KeyCode::Char('d'));
    assert!(h.screen().contains("Retained output: 42"));
    h.key(KeyCode::Esc);
    assert!(h.screen().contains("Value: 42"));
    h.action(WorkspaceAction::InspectDocument);
    assert!(h.screen().contains("Original whole-closure review"));
    assert_eq!(h.counts.format.get(), 1);
    assert_eq!(h.counts.discovery.get(), 1);
    assert_eq!(h.root.read_file("main.sprout"), "root body");
}

#[test]
fn registered_policy_authorized_run_dispatches_only_when_policy_allows() {
    for allow in [true, false] {
        let mut h = Harness::with_review_requirement(allow, Some(|_| None));
        h.action(WorkspaceAction::CheckCurrentDocument);
        assert_eq!(h.counts.policy.get(), 0);
        h.action(WorkspaceAction::RunCurrentDocument);
        assert!(!h.ui.registered_modal_open());
        assert_eq!(h.counts.policy.get(), 1);
        assert_eq!(h.counts.dispatch.get(), u32::from(allow));
        assert_eq!(h.ui.registered_result().is_some(), allow);
        let screen = h.screen();
        assert!(screen.contains(if allow { "Value: 42" } else { "Run rejected" }));
        assert!(!screen.contains("prepared-sha256"));
    }
}

#[test]
fn registered_policy_authorized_run_can_require_explicit_review() {
    let mut h = Harness::with_review_requirement(
        true,
        Some(|_| Some("Network access is not covered by routine Run approval.".into())),
    );
    h.action(WorkspaceAction::RunCurrentDocument);
    assert!(h.ui.registered_modal_open());
    assert!(h.screen().contains("Network access is not covered"));
    assert_eq!(h.counts.policy.get(), 0);
    assert_eq!(h.counts.dispatch.get(), 0);
    h.key(KeyCode::Enter);
    assert_eq!(h.counts.policy.get(), 1);
    assert_eq!(h.counts.dispatch.get(), 1);
}

#[test]
fn registered_run_denial_and_cancellation_never_dispatch_or_replay() {
    let mut h = Harness::new(false);
    h.action(WorkspaceAction::RunCurrentDocument);
    h.key(KeyCode::Enter);
    assert!(h.screen().contains("Run rejected"));
    assert_eq!(h.counts.policy.get(), 1);
    assert_eq!(h.counts.dispatch.get(), 0);
    assert!(h.ui.registered_result().is_none());
    h.key(KeyCode::Enter);
    assert_eq!(h.counts.policy.get(), 1);
    h.key(KeyCode::Esc);
    h.action(WorkspaceAction::RunCurrentDocument);
    h.key(KeyCode::Esc);
    assert!(!h.ui.registered_modal_open());
    assert!(h.screen().contains("Run cancelled"));
    assert_eq!(h.counts.policy.get(), 1);
    assert_eq!(h.counts.dispatch.get(), 0);
}

#[test]
fn registered_dependency_and_policy_changes_reject_pending_run_without_new_approval() {
    for policy_change in [false, true] {
        let mut h = Harness::new(true);
        h.action(WorkspaceAction::RunCurrentDocument);
        if policy_change {
            h.app
                .set_registered_policy(serde_json::json!({"mode":"different"}))
                .unwrap();
        } else {
            h.edit_dep("changed ");
        }
        h.key(KeyCode::Enter);
        assert!(h.screen().contains("Run rejected"));
        assert_eq!(h.counts.discovery.get(), 1);
        assert_eq!(h.counts.policy.get(), 0);
        assert_eq!(h.counts.dispatch.get(), 0);
        h.key(KeyCode::Enter);
        assert_eq!(h.counts.dispatch.get(), 0);
    }
}

#[test]
fn registered_result_inspection_is_passive_and_marks_changed_dependency_historical() {
    let mut h = Harness::new(true);
    h.action(WorkspaceAction::RunCurrentDocument);
    h.key(KeyCode::Enter);
    h.key(KeyCode::Esc);
    h.edit_dep("new ");
    assert!(h.screen().contains("STALE"));
    h.action(WorkspaceAction::InspectDocument);
    let screen = h.screen();
    assert!(screen.contains("STALE"));
    assert!(screen.contains("Retained output: 42"));
    let result = h.ui.registered_result().unwrap();
    let dep_index = result
        .discovered()
        .compiler_sources()
        .iter()
        .position(|identity| {
            result
                .discovered()
                .sources()
                .iter()
                .any(|source| source.module == "dep" && &source.identity == identity)
        })
        .unwrap();
    assert_eq!(
        &*result.compiler_source(dep_index).unwrap().text,
        "dep body"
    );
    h.key(KeyCode::Enter);
    h.key(KeyCode::Esc);
    assert_eq!(
        (
            h.counts.discovery.get(),
            h.counts.policy.get(),
            h.counts.dispatch.get(),
            h.counts.format.get()
        ),
        (1, 1, 1, 1)
    );
    h.ui.focus_named("output").unwrap();
    h.key(KeyCode::Char('d'));
    assert!(h.ui.registered_modal_open());
    assert!(h.screen().contains("Retained output: 42"));
    h.key(KeyCode::Esc);
    h.ui.select_editor("editor", &h.app, Some(&h.sources))
        .unwrap();
    h.ui.handle_event_with_sources(Event::Paste("draft ".into()), &mut h.app, &mut h.sources);
    h.action(WorkspaceAction::InspectDocument);
    assert!(h.screen().contains("STALE"));
    assert_eq!(h.counts.dispatch.get(), 1);
}

#[test]
fn registered_quick_open_selects_owned_unsaved_documents_even_after_disk_removal() {
    let mut h = Harness::new(true);
    h.ui.open_app_document(&mut h.app, "dep.sprout").unwrap();
    let dep_id = h
        .app
        .open_document()
        .unwrap()
        .evaluation_source_id()
        .clone();
    h.ui.handle_event_with_sources(Event::Paste("unsaved ".into()), &mut h.app, &mut h.sources);
    assert!(h.ui.open_app_document(&mut h.app, "main.sprout").is_err());
    h.ui.handle_event_with_sources(
        Event::Key(KeyEvent::new(
            KeyCode::Char('s'),
            event::KeyModifiers::CONTROL,
        )),
        &mut h.app,
        &mut h.sources,
    );
    h.ui.open_app_document(&mut h.app, "main.sprout").unwrap();
    std::fs::remove_file(std::path::Path::new(h.app.workspace().root()).join("dep.sprout"))
        .unwrap();
    h.app.refresh_files().unwrap();
    h.action(WorkspaceAction::OpenAppDocument);
    h.ui.command_line = Some("dep".into());
    let path =
        h.ui.open_document_event(
            &Event::Key(KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE)),
            &h.app,
        )
        .unwrap();
    assert_eq!(path, "dep.sprout");
    h.ui.submit_command(
        &path,
        &mut h.app,
        &mut InteractiveSession::new(),
        None,
        &mut h.sources,
    );
    assert_eq!(h.app.selected_registered_module(), Some("dep"));
    assert_eq!(
        h.app.open_document().unwrap().evaluation_source_id(),
        &dep_id
    );
    assert_eq!(h.app.open_document().unwrap().text(), "unsaved dep body");
    assert!(h.app.open_document().unwrap().is_dirty());
    assert_eq!(
        (
            h.counts.discovery.get(),
            h.counts.policy.get(),
            h.counts.dispatch.get()
        ),
        (0, 0, 0)
    );
}

#[test]
fn registered_check_navigates_to_current_import_and_rejects_changed_revision() {
    let mut h = Harness::new(true);
    h.edit_dep("é invalid ");
    h.action(WorkspaceAction::CheckCurrentDocument);
    h.action(WorkspaceAction::NavigateResult);
    assert_eq!(h.app.selected_registered_module(), Some("dep"));
    assert_eq!(
        h.ui.current_editor_draft(&h.app, Some(&h.sources))
            .unwrap()
            .cursor,
        3
    );
    h.edit_dep("changed ");
    h.ui.select_registered_app_document(&mut h.app, "root")
        .unwrap();
    let stale =
        h.ui.invoke_action(
            WorkspaceAction::NavigateResult,
            &mut h.app,
            &mut h.sources,
            None,
        )
        .unwrap_err();
    assert!(stale.contains("historical"), "{stale}");
    assert_eq!(h.app.selected_registered_module(), Some("root"));
    assert_eq!(h.counts.dispatch.get(), 0);
}
