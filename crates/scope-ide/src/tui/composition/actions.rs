use super::*;
use crate::{cli::ScriptPresentation, scratch::ScratchBuffers};

/// Bounded intentions, not shell fragments. Focus never grants execution authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceAction {
    RunScript,
    /// Publish the current App editor draft in memory, then review candidate evaluation.
    RunCurrentDocument,
    /// Publish the current App editor draft in memory, then check diagnostics.
    CheckCurrentDocument,
    /// Review and explicitly confirm a read operation on the active App document.
    InspectDocument,
    /// Open a workspace file as the existing editor's App-owned target.
    OpenAppDocument,
    SaveAppDocument,
    FocusAppDocument,
    /// Check the committed App document, never an unpublished editor draft.
    CheckDocumentDiagnostics,
    /// Invocation token pins the selected identity and committed revision.
    Downstream {
        id: String,
        scratch: u64,
        source: scope_language::SourceId,
        language: String,
        version: u64,
    },
    NewScript,
    CommandPrompt(String),
    SelectDocument(u64),
    Focus(String),
    PreviousPane,
    CommitDraft,
    FocusDraft {
        pane: String,
        scratch: u64,
    },
    PreviousResult,
    NextResult,
    ShowTranscript,
    ShowResult,
    Help,
    MoreActions,
}

#[derive(Clone, Debug)]
pub struct ActionInfo {
    pub action: WorkspaceAction,
    pub id: String,
    pub label: String,
    pub shortcut: Option<&'static str>,
    pub disabled_reason: Option<String>,
}

/// A bounded command shortcut. Registration grants no semantic authority.
#[derive(Clone, Debug)]
pub struct DownstreamAction {
    pub id: String,
    pub label: String,
    pub language: String,
    pub namespace: String,
    pub command: String,
    pub args: Vec<String>,
}

/// Contextual input requested at action invocation, never at registration time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownstreamPrompt {
    pub label: String,
    pub choices: Vec<DownstreamPromptChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownstreamPromptChoice {
    pub label: String,
    pub args: Vec<String>,
}

pub(super) struct PendingPrompt {
    pub(super) action: WorkspaceAction,
    pub(super) prompt: DownstreamPrompt,
    pub(super) selected: usize,
}

#[derive(Default)]
pub(super) struct Picker {
    query: String,
    selected: usize,
    advanced: bool,
}

impl WorkspaceTui {
    pub(super) fn refresh_action_context(&mut self, app: &App) {
        self.action_document_present = app.open_document().is_some();
        self.action_candidate_available =
            super::super::semantic::SemanticInspection::candidate_available(app);
    }

    pub(super) fn primary_f5_action(
        &self,
        modifiers: event::KeyModifiers,
        sources: &ScratchBuffers,
    ) -> Option<WorkspaceAction> {
        if self.app_editor {
            if modifiers.contains(event::KeyModifiers::SHIFT) {
                return Some(WorkspaceAction::CheckCurrentDocument);
            }
            return self
                .action_candidate_available
                .then_some(WorkspaceAction::RunCurrentDocument);
        }
        // Non-Script views retain their own F5 key handling.
        (modifiers.is_empty()
            && sources
                .selected()
                .is_ok_and(|source| source.language_id() == "scope-script"))
        .then_some(WorkspaceAction::RunScript)
    }
    /// Adds a contextual shortcut to the normal downstream command route.
    pub fn register_downstream_action(&mut self, action: DownstreamAction) -> Result<(), String> {
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        };
        if self.downstream_actions.len() >= 64
            || !token(&action.id)
            || !token(&action.namespace)
            || !token(&action.command)
            || action.label.is_empty()
            || action.label.len() > 256
            || action.language.is_empty()
            || action.language.len() > 64
            || action.args.len() > 32
            || action.args.iter().any(|s| {
                s.is_empty()
                    || s.len() > 1024
                    || s.chars()
                        .any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '\\'))
            })
            || self.downstream_actions.iter().any(|a| a.id == action.id)
        {
            return Err("invalid, duplicate, or excessive downstream action registration".into());
        }
        self.downstream_actions.push(action);
        Ok(())
    }
    pub(super) fn draft_blocker(&self) -> Option<String> {
        let blockers: Vec<_> = self
            .instances
            .iter()
            .filter(|(_, i)| i.pane.has_dirty_draft())
            .map(|(id, i)| {
                let sources = i.pane.dirty_draft_sources();
                if i.pane.has_dirty_app_draft() {
                    format!("pane {id}: unpublished App document draft; F1 Edit document to publish or cancel")
                } else if sources.is_empty() {
                    format!("pane {id}: draft source metadata unavailable")
                } else {
                    format!(
                        "pane {id}: {}",
                        sources
                            .iter()
                            .map(|(scratch, detail)| format!("scratch {scratch} {detail}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            })
            .collect();
        (!blockers.is_empty()).then(|| format!("commit or cancel all editor drafts before execution; {}. F1: Select blocking draft, then Ctrl-S commits or Ctrl-C cancels. Conservative guard includes unrelated drafts.", blockers.join("; ")))
    }

    fn run_disabled(&self, sources: &ScratchBuffers, supported: bool) -> Option<String> {
        if self.app_editor {
            return Some(
                "App editor: use F1 Evaluate; Script execution targets a separate scratch".into(),
            );
        }
        if !supported {
            return Some("host has not opted into pure direct Script execution".into());
        }
        match sources.selected() {
            Err(_) => return Some("no live selected scratch".into()),
            Ok(source) if source.language_id() != "scope-script" => {
                return Some(format!(
                    "selected scratch {} is {}, not scope-script",
                    source.id(),
                    source.language_id()
                ));
            }
            _ => {}
        }
        self.draft_blocker()
    }

    /// Metadata and availability are derived from capabilities and current state.
    pub fn actions(
        &self,
        sources: &ScratchBuffers,
        direct_script_supported: bool,
    ) -> Vec<ActionInfo> {
        let mut items = Vec::new();
        let mut add = |action, id: String, label: String, shortcut, disabled_reason| {
            items.push(ActionInfo {
                action,
                id,
                label,
                shortcut,
                disabled_reason,
            })
        };
        for (action, id, label, shortcut) in [
            (
                WorkspaceAction::RunCurrentDocument,
                "document.run-current",
                "Run",
                Some("F5"),
            ),
            (
                WorkspaceAction::CheckCurrentDocument,
                "document.check-current",
                "Check",
                Some("Alt-Enter / Shift-F5"),
            ),
        ] {
            if action == WorkspaceAction::RunCurrentDocument
                && (!self.app_editor || !self.action_candidate_available)
            {
                continue;
            }
            add(
                action,
                id.into(),
                label.into(),
                shortcut,
                (!self.app_editor
                    || !self
                        .focused
                        .as_ref()
                        .and_then(|id| self.instances.get(id))
                        .is_some_and(|instance| instance.editable))
                .then(|| "open and focus an App document editor; no scratch fallback".into()),
            );
        }
        add(
            WorkspaceAction::RunScript,
            "script.run".into(),
            "Run selected Script".into(),
            None,
            self.run_disabled(sources, direct_script_supported),
        );
        add(
            WorkspaceAction::InspectDocument,
            "document.evaluate".into(),
            "Inspect".into(),
            None,
            self.draft_blocker(),
        );
        add(
            WorkspaceAction::CheckDocumentDiagnostics,
            "document.diagnostics".into(),
            "Check document diagnostics".into(),
            None,
            self.draft_blocker(),
        );
        add(
            WorkspaceAction::OpenAppDocument,
            "document.open-app".into(),
            "Open".into(),
            None,
            self.draft_blocker(),
        );
        add(
            WorkspaceAction::FocusAppDocument,
            "document.edit-app".into(),
            "Edit document".into(),
            None,
            None,
        );
        add(
            WorkspaceAction::SaveAppDocument,
            "document.save-app".into(),
            "Save".into(),
            None,
            self.draft_blocker(),
        );
        add(
            WorkspaceAction::NewScript,
            "scratch.new-script".into(),
            "New Script (select and edit)".into(),
            None,
            None,
        );
        for (id, label, command) in [
            (
                "document.open",
                "Open scratch file — enter workspace-relative path",
                ":scope open ",
            ),
            (
                "document.switch",
                "Switch Document — list open documents",
                ":scope buffers",
            ),
            (
                "document.save",
                "Save committed document — publish draft first",
                ":scope save",
            ),
            (
                "document.save-as",
                "Save As — authorized destination; publish first",
                ":scope save-as ",
            ),
            (
                "document.ask",
                "Ask Agent — committed active source, read only",
                ":scope document-ask ",
            ),
            (
                "document.propose",
                "Propose Edit — committed active source, explicit review",
                ":scope document-propose ",
            ),
            (
                "document.review",
                "Review source proposal — enter proposal ID",
                ":scope source-proposal ",
            ),
            (
                "document.accept",
                "Accept reviewed source proposal — enter proposal ID",
                ":scope source-accept ",
            ),
            (
                "document.reject",
                "Reject source proposal — enter proposal ID",
                ":scope source-reject ",
            ),
            (
                "workspace.return",
                "Return to Workbench (leave editor; retain draft)",
                "",
            ),
        ] {
            add(
                WorkspaceAction::CommandPrompt(command.into()),
                id.into(),
                label.into(),
                None,
                None,
            );
        }
        if let Ok(source) = sources.selected()
            && source.language_id() == "language.path"
        {
            let pending: Vec<_> = self
                .output
                .source_proposals()
                .list()
                .filter(|p| {
                    p.target() == crate::source_edit::SourceId::Scratch(source.id())
                        && p.status() == crate::proposal::ProposalStatus::Pending
                })
                .collect();
            if let [proposal] = pending.as_slice() {
                for (id, label, command) in [
                    (
                        "path.review",
                        "Path: Review Proof Proposal — exact edits and evidence",
                        "source-proposal",
                    ),
                    ("path.diff", "Path: Show Proof Diff", "source-diff"),
                    (
                        "path.accept",
                        "Path: Accept Proof Proposal — apply in memory, then check",
                        "source-accept",
                    ),
                    (
                        "path.reject",
                        "Path: Reject Proof Proposal",
                        "source-reject",
                    ),
                ] {
                    add(
                        WorkspaceAction::CommandPrompt(format!(
                            ":scope {command} {}",
                            proposal.id().0
                        )),
                        id.into(),
                        label.into(),
                        None,
                        self.draft_blocker(),
                    );
                }
            }
        }
        for source in sources.list() {
            let name = source
                .destination_path()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "Untitled Script".into());
            add(
                WorkspaceAction::SelectDocument(source.id()),
                format!("document.select.{}", source.id()),
                format!(
                    "Switch Document: {name} [{}] revision {}{}",
                    source.id(),
                    source.version(),
                    if source.is_dirty() {
                        " • unsaved"
                    } else {
                        ""
                    }
                ),
                None,
                None,
            );
        }
        for id in self.visible_focus_ids() {
            add(
                WorkspaceAction::Focus(id.clone()),
                format!("focus.{id}"),
                format!("Focus {id}"),
                None,
                None,
            );
        }
        add(
            WorkspaceAction::PreviousPane,
            "focus.previous".into(),
            "Focus previous pane".into(),
            Some("F3"),
            self.last_focused
                .as_ref()
                .filter(|id| self.visible_focus_ids().contains(id))
                .is_none()
                .then(|| "no visible previous pane".into()),
        );
        let editable = self
            .focused
            .as_ref()
            .and_then(|id| self.instances.get(id))
            .is_some_and(|i| i.editable);
        add(
            WorkspaceAction::CommitDraft,
            "editor.commit".into(),
            "Publish editor draft to document (in memory; not Save)".into(),
            Some("Ctrl-S"),
            (!editable).then(|| "focus an editable pane first".into()),
        );
        for (pane, instance) in &self.instances {
            for (scratch, detail) in instance.pane.dirty_draft_sources() {
                let reason = if sources.get(scratch).is_err() {
                    Some(
                        "scratch is closed; draft retained, cannot commit to a closed source"
                            .into(),
                    )
                } else if !self.visible_focus_ids().contains(pane) {
                    Some("editor pane is hidden in this layout".into())
                } else {
                    None
                };
                add(
                    WorkspaceAction::FocusDraft {
                        pane: pane.clone(),
                        scratch,
                    },
                    format!("draft.{pane}.{scratch}"),
                    format!("Select blocking draft {scratch} in {pane}: {detail}"),
                    None,
                    reason,
                );
            }
        }
        add(
            WorkspaceAction::PreviousResult,
            "result.previous".into(),
            "Previous retained result".into(),
            Some("Alt-Left"),
            (self.result_index.unwrap_or(0) == 0).then(|| "no earlier result".into()),
        );
        add(
            WorkspaceAction::NextResult,
            "result.next".into(),
            "Next retained result".into(),
            Some("Alt-Right"),
            (self
                .result_index
                .is_none_or(|i| i + 1 >= self.results.len()))
            .then(|| "no later result".into()),
        );
        add(
            WorkspaceAction::ShowResult,
            "output.result".into(),
            "Show retained Script result".into(),
            None,
            self.result_index
                .is_none()
                .then(|| "no retained direct result".into()),
        );
        add(
            WorkspaceAction::ShowTranscript,
            "output.transcript".into(),
            "Show command transcript".into(),
            None,
            None,
        );
        add(
            WorkspaceAction::Help,
            "workspace.help".into(),
            "Workspace help".into(),
            None,
            None,
        );
        if let Ok(source) = sources.selected() {
            for entry in &self.downstream_actions {
                if entry.language == source.language_id() {
                    add(
                        WorkspaceAction::Downstream {
                            id: entry.id.clone(),
                            scratch: source.id(),
                            source: source.evaluation_source_id().clone(),
                            language: source.language_id().into(),
                            version: source.version(),
                        },
                        format!("downstream.{}", entry.id),
                        entry.label.clone(),
                        None,
                        self.draft_blocker(),
                    );
                }
            }
        }
        add(
            WorkspaceAction::MoreActions,
            "workspace.more-actions".into(),
            "More actions".into(),
            None,
            None,
        );
        items
    }

    /// Rechecks availability at invocation. No shell parser, target or session is involved.
    pub fn invoke_action(
        &mut self,
        action: WorkspaceAction,
        app: &mut App,
        sources: &mut ScratchBuffers,
        mut host: Option<&mut dyn ReadOnlyCommand>,
    ) -> Result<(), String> {
        self.refresh_action_context(app);
        let supported = host.as_ref().is_some_and(|h| h.supports_direct_script());
        let info = self
            .actions(sources, supported)
            .into_iter()
            .find(|i| i.action == action)
            .ok_or_else(|| {
                if action == WorkspaceAction::RunCurrentDocument && self.app_editor {
                    "Run requires exactly one eligible zero-argument, read-only document CandidateEvaluation operation; use Inspect to choose explicitly"
                } else {
                    "action is not available in this workspace"
                }
            })?;
        if let Some(reason) = info.disabled_reason {
            return Err(reason);
        }
        if self.open_app_prompt {
            self.open_app_prompt = false;
            self.command_line = None;
        }
        match action {
            WorkspaceAction::MoreActions => {
                self.picker = Some(Picker {
                    advanced: true,
                    ..Picker::default()
                });
            }
            WorkspaceAction::RunCurrentDocument | WorkspaceAction::CheckCurrentDocument => {
                let document = app.open_document().ok_or("no App document is open")?;
                let identity = document.id();
                let focused = self
                    .focused
                    .as_ref()
                    .ok_or("focus the App document editor first")?;
                let instance = self
                    .instances
                    .get_mut(focused)
                    .ok_or("focused editor is unavailable")?;
                if !self.app_editor || !instance.editable {
                    return Err("focus the App document editor first; no scratch fallback".into());
                }
                // Reuse the editor's exact-baseline publication path, not a text copy.
                let handled = instance.pane.handle_source_event(
                    Event::Key(KeyEvent::new(
                        KeyCode::Char('s'),
                        event::KeyModifiers::CONTROL,
                    )),
                    &mut super::super::ScratchEditAccess::app(app.open_document_mut().unwrap()),
                );
                if !handled {
                    return Err("editor did not handle draft publication".into());
                }
                if app
                    .open_document()
                    .is_none_or(|document| document.id() != identity)
                {
                    return Err("App document changed during publication".into());
                }
                if let Some(reason) = self.draft_blocker() {
                    return Err(reason);
                }
                if action == WorkspaceAction::RunCurrentDocument {
                    self.semantic_inspection =
                        Some(super::super::semantic::SemanticInspection::candidate(app)?);
                } else {
                    self.invoke_action(
                        WorkspaceAction::CheckDocumentDiagnostics,
                        app,
                        sources,
                        host,
                    )?;
                }
            }
            WorkspaceAction::CommandPrompt(command) => {
                if command.is_empty() {
                    self.focus_named("output")?;
                } else {
                    self.command_line = Some(command);
                }
            }
            WorkspaceAction::CheckDocumentDiagnostics => {
                let document = app.open_document().ok_or("no App document is open")?;
                let mut diagnostics = Vec::new();
                crate::cli::print_diagnostics(app, &mut diagnostics).map_err(|e| e.to_string())?;
                self.show_result = false;
                self.output_scroll = 0;
                self.output_errors.push_str(&format!(
                    "Check document diagnostics — App {} — revision {}\n{}",
                    document.path().display(),
                    document.version(),
                    String::from_utf8_lossy(&diagnostics),
                ));
            }
            WorkspaceAction::OpenAppDocument => {
                self.command_line = Some(String::new());
                self.open_app_prompt = true;
            }
            WorkspaceAction::FocusAppDocument => {
                if app.open_document().is_none() {
                    return Err("no App document is open".into());
                }
                let editor = self
                    .visible_focus_ids()
                    .into_iter()
                    .find(|id| {
                        self.instances
                            .get(id)
                            .is_some_and(|instance| instance.editable)
                    })
                    .ok_or("no visible editable pane")?;
                self.focus_named(&editor)?;
                self.app_editor = true;
            }
            WorkspaceAction::SaveAppDocument => {
                app.save_open_document()
                    .map_err(|error| error.to_string())?;
            }
            WorkspaceAction::InspectDocument => {
                crate::semantic_inspection::discover_app(app).map_err(|e| e.to_string())?;
                self.semantic_inspection = Some(
                    self.retained_inspection
                        .take()
                        .unwrap_or_default()
                        .unrestricted(),
                );
            }
            WorkspaceAction::SelectDocument(id) => {
                self.app_editor = false;
                sources.select(id).map_err(|e| e.to_string())?;
                if let Some(editor) = self
                    .visible_focus_ids()
                    .into_iter()
                    .find(|id| self.instances.get(id).is_some_and(|i| i.editable))
                {
                    self.focus_named(&editor)?;
                }
            }
            WorkspaceAction::Downstream {
                id,
                scratch,
                source,
                language,
                version,
            } => {
                let selected = sources.selected().map_err(|e| e.to_string())?;
                if selected.id() != scratch
                    || selected.evaluation_source_id() != &source
                    || selected.language_id() != language
                    || selected.version() != version
                {
                    return Err("downstream action source changed; rediscover the action".into());
                }
                let entry = self
                    .downstream_actions
                    .iter()
                    .find(|a| a.id == id)
                    .ok_or("unknown downstream action")?;
                let host = host.as_deref_mut().ok_or("no downstream host installed")?;
                if let Some(prompt) = host.downstream_action_prompt(&id, selected)? {
                    if prompt.choices.is_empty() {
                        return Err("downstream prompt has no available choices".into());
                    }
                    self.pending_prompt = Some(PendingPrompt {
                        action: WorkspaceAction::Downstream {
                            id,
                            scratch,
                            source,
                            language,
                            version,
                        },
                        prompt,
                        selected: 0,
                    });
                    self.command_line = Some(String::new());
                } else {
                    let args = entry.args.clone();
                    self.dispatch_downstream(
                        &WorkspaceAction::Downstream {
                            id,
                            scratch,
                            source,
                            language,
                            version,
                        },
                        args,
                        sources,
                        host,
                    )?;
                }
            }
            WorkspaceAction::RunScript => {
                let record = host
                    .as_mut()
                    .unwrap()
                    .invoke_script_direct(sources.selected().map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                self.results.push(record);
                self.result_index = Some(self.results.len() - 1);
                self.show_result = true;
                self.output_scroll = 0;
            }
            WorkspaceAction::ShowResult => {
                self.show_result = true;
                self.output_scroll = 0;
            }
            WorkspaceAction::NewScript => {
                self.app_editor = false;
                sources.create("scope-script", "");
                if let Some(editor) = self.visible_focus_ids().into_iter().find(|id| {
                    self.instances
                        .get(id)
                        .is_some_and(|instance| instance.editable)
                }) {
                    self.focus_named(&editor)?;
                }
            }
            WorkspaceAction::Focus(id) => self.focus_named(&id)?,
            WorkspaceAction::PreviousPane => self.focus_named("last")?,
            WorkspaceAction::CommitDraft => {
                self.handle_event_with_sources(
                    Event::Key(KeyEvent::new(
                        KeyCode::Char('s'),
                        event::KeyModifiers::CONTROL,
                    )),
                    app,
                    sources,
                );
            }
            WorkspaceAction::FocusDraft { pane, scratch } => {
                self.focus_named(&pane)?;
                self.app_editor = false;
                sources.select(scratch).map_err(|e| e.to_string())?;
            }
            WorkspaceAction::PreviousResult => {
                self.result_index = self.result_index.map(|i| i - 1);
                self.show_result = true;
                self.output_scroll = 0;
            }
            WorkspaceAction::NextResult => {
                self.result_index = self.result_index.map(|i| i + 1);
                self.show_result = true;
                self.output_scroll = 0;
            }
            WorkspaceAction::ShowTranscript => {
                self.show_result = false;
                self.output_scroll = 0;
            }
            WorkspaceAction::Help => {
                self.show_result = false;
                self.output_errors.push_str("Actions: F1 search, Up/Down select, Enter invoke, Esc cancel.\nNew Script scratch explicitly changes selection. Ctrl-S publishes in memory only; use the explicit Save action to write to disk. Ctrl-R/F5 publishes the current App editor draft and opens candidate evaluation review. Alt-Enter (Shift-F5 alias) publishes and checks diagnostics. Neither Run nor Check saves to disk or falls back to scratch. Run selected Script remains an explicit separate action.\nOutput: F6 focus, Up/Down or PageUp/PageDown scroll, Alt-Left/Right history; F3 returns to previous pane.\nF4 commands and F2 shell remain available. Results indicate interpreter completion only, not semantic approval.\nDirty drafts (including parked/unrelated drafts) conservatively block runs. F1 lists blockers. Run/Check explicitly publish the focused App draft; unrelated drafts still block. No implicit save or native authority.\n");
            }
        }
        Ok(())
    }

    fn dispatch_downstream(
        &mut self,
        action: &WorkspaceAction,
        args: Vec<String>,
        sources: &mut ScratchBuffers,
        host: &mut dyn ReadOnlyCommand,
    ) -> Result<(), String> {
        let info = self
            .actions(sources, false)
            .into_iter()
            .find(|i| &i.action == action)
            .ok_or("downstream action source changed; rediscover the action")?;
        if let Some(reason) = info.disabled_reason {
            return Err(reason);
        }
        let WorkspaceAction::Downstream {
            id,
            scratch,
            source,
            language,
            version,
        } = action
        else {
            return Err("not a downstream action".into());
        };
        let entry = self
            .downstream_actions
            .iter()
            .find(|a| &a.id == id)
            .ok_or("unknown downstream action")?;
        sources
            .bind_semantic_target(Some(*scratch))
            .map_err(|e| e.to_string())?;
        self.show_result = false;
        self.output_errors.push_str(&format!("Downstream {id}: scratch {scratch}, source {source:?}, language {language}, committed revision {version} (historical attribution; not semantic approval)\n"));
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        let mut output = Vec::new();
        let result = host.invoke_downstream_with_proposals(
            &entry.namespace,
            &entry.command,
            &refs,
            Some(sources.selected().map_err(|e| e.to_string())?),
            self.output.source_edit_controller(),
            &mut output,
        );
        self.output.append(&output);
        match result {
            Ok(crate::cli::DownstreamOutcome::Handled) => {}
            Ok(crate::cli::DownstreamOutcome::Unhandled) => {
                self.output.append(b"unhandled downstream command\n")
            }
            Err(error) => self.output.append(format!("{error}\n").as_bytes()),
        }
        Ok(())
    }

    pub(super) fn submit_prompt(
        &mut self,
        text: String,
        sources: &mut ScratchBuffers,
        host: Option<&mut dyn ReadOnlyCommand>,
    ) -> Result<(), String> {
        let pending = self.pending_prompt.take().ok_or("no pending prompt")?;
        if text.trim().is_empty() {
            return Err("prompt input must not be empty".into());
        }
        let WorkspaceAction::Downstream { id, .. } = &pending.action else {
            unreachable!()
        };
        let mut args = self
            .downstream_actions
            .iter()
            .find(|a| &a.id == id)
            .ok_or("unknown downstream action")?
            .args
            .clone();
        args.extend(pending.prompt.choices[pending.selected].args.clone());
        args.push(text);
        self.dispatch_downstream(
            &pending.action,
            args,
            sources,
            host.ok_or("no downstream host installed")?,
        )
    }

    pub(super) fn result_text(&self, sources: Option<&ScratchBuffers>) -> Option<String> {
        if !self.show_result {
            return None;
        }
        let index = self.result_index?;
        let record: &ScriptPresentation = self.results.get(index)?;
        let current = sources.and_then(|s| {
            s.list()
                .into_iter()
                .find(|b| b.evaluation_source_id() == &record.source_identity)
        });
        let attribution = match current {
            Some(source)
                if scope_language::RevisionId::from_version(source.version())
                    == record.source_revision =>
            {
                "current committed revision"
            }
            Some(_) => "older committed revision",
            None => "source closed or unavailable",
        };
        let mut text = format!(
            "Result {}/{} • {:?} revision {:?} • {attribution}\nstatus: interpreter {} (not semantic approval)\n",
            index + 1,
            self.results.len(),
            record.source_identity,
            record.source_revision,
            if record.error.is_none() {
                "success"
            } else {
                "failure"
            }
        );
        for (i, value) in record.prints.iter().enumerate() {
            text.push_str(&format!("print {}: {value}\n", i + 1));
        }
        if let Some(value) = &record.final_value {
            text.push_str(&format!("final: {value}\n"));
        }
        if let Some(error) = &record.error {
            text.push_str(&format!(
                "error: {}\nlocation: {}:{} (Unicode scalars; byte offset {})\n",
                error.message, error.line, error.column, error.byte_offset
            ));
        }
        Some(text)
    }

    pub(super) fn picker_items(
        &self,
        sources: &ScratchBuffers,
        supported: bool,
    ) -> Vec<ActionInfo> {
        let query = self
            .picker
            .as_ref()
            .map(|p| p.query.to_lowercase())
            .unwrap_or_default();
        let advanced = self.picker.as_ref().is_some_and(|picker| picker.advanced);
        let mut items: Vec<_> = self
            .actions(sources, supported)
            .into_iter()
            .filter(|item| {
                if !query.is_empty() {
                    return format!("{} {}", item.id, item.label)
                        .to_lowercase()
                        .contains(&query);
                }
                if advanced {
                    return item.action != WorkspaceAction::MoreActions;
                }
                match item.action {
                    WorkspaceAction::RunCurrentDocument => {
                        self.app_editor && self.action_candidate_available
                    }
                    WorkspaceAction::RunScript => {
                        !self.app_editor
                            && supported
                            && sources
                                .selected()
                                .is_ok_and(|source| source.language_id() == "scope-script")
                    }
                    WorkspaceAction::CheckCurrentDocument
                    | WorkspaceAction::SaveAppDocument
                    | WorkspaceAction::InspectDocument => {
                        self.app_editor && self.action_document_present
                    }
                    WorkspaceAction::OpenAppDocument | WorkspaceAction::MoreActions => true,
                    _ => false,
                }
            })
            .collect();
        if query.is_empty() && !advanced {
            for item in &mut items {
                if item.action == WorkspaceAction::RunScript {
                    item.label = "Run".into();
                    item.shortcut = Some("F5");
                }
            }
            items.sort_by_key(|item| match item.action {
                WorkspaceAction::RunCurrentDocument | WorkspaceAction::RunScript => 0,
                WorkspaceAction::CheckCurrentDocument => 1,
                WorkspaceAction::OpenAppDocument => 2,
                WorkspaceAction::SaveAppDocument => 3,
                WorkspaceAction::InspectDocument => 4,
                _ => 5,
            });
        }
        items
    }

    pub(super) fn picker_event(
        &mut self,
        input: &Event,
        sources: &ScratchBuffers,
        supported: bool,
    ) -> Option<WorkspaceAction> {
        let items = self.picker_items(sources, supported);
        let picker = self.picker.as_mut()?;
        if let Event::Key(key) = input
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Esc | KeyCode::F(1) => self.picker = None,
                KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
                KeyCode::Down => {
                    picker.selected = (picker.selected + 1).min(items.len().saturating_sub(1))
                }
                KeyCode::Backspace => {
                    picker.query.pop();
                    picker.selected = 0;
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(event::KeyModifiers::CONTROL | event::KeyModifiers::ALT) =>
                {
                    picker.query.push(ch);
                    picker.selected = 0;
                }
                KeyCode::Enter => {
                    if let Some(item) = items.get(picker.selected)
                        && item.disabled_reason.is_none()
                    {
                        let action = item.action.clone();
                        self.picker = None;
                        return Some(action);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub(super) fn render_picker(
        &self,
        frame: &mut ratatui::Frame<'_>,
        sources: &ScratchBuffers,
        supported: bool,
    ) {
        let Some(picker) = &self.picker else {
            return;
        };
        let items = self.picker_items(sources, supported);
        let area = frame.area();
        frame.render_widget(ratatui::widgets::Clear, area);
        let height = area.height.saturating_sub(2).max(1) as usize;
        let start = picker.selected.saturating_sub(height - 1);
        let text = items
            .iter()
            .enumerate()
            .skip(start)
            .map(|(n, i)| {
                format!(
                    "{} {} {}{}",
                    if n == picker.selected { ">" } else { " " },
                    i.label,
                    i.shortcut.unwrap_or(""),
                    i.disabled_reason
                        .as_ref()
                        .map(|r| format!(" — disabled: {r}"))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        frame.render_widget(
            Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(format!(
                "{}: {} • type to search all • Esc cancel",
                if picker.advanced {
                    "More actions"
                } else {
                    "Actions"
                },
                picker.query
            ))),
            area,
        );
    }
}
