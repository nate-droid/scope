use super::*;
use crate::{cli::ScriptPresentation, scratch::ScratchBuffers};

/// Bounded intentions, not shell fragments. Focus never grants execution authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceAction {
    RunScript,
    /// Invocation token pins the selected identity and committed revision.
    Downstream {
        id: String,
        scratch: u64,
        source: scope_language::SourceId,
        language: String,
        version: u64,
    },
    NewScript,
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

#[derive(Default)]
pub(super) struct Picker {
    query: String,
    selected: usize,
}

impl WorkspaceTui {
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
                if sources.is_empty() {
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
        add(
            WorkspaceAction::RunScript,
            "script.run".into(),
            "Run selected Script".into(),
            Some("F5"),
            self.run_disabled(sources, direct_script_supported),
        );
        add(
            WorkspaceAction::NewScript,
            "scratch.new-script".into(),
            "New Script scratch (select)".into(),
            None,
            None,
        );
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
            "Commit focused editor draft".into(),
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
        let supported = host.as_ref().is_some_and(|h| h.supports_direct_script());
        let info = self
            .actions(sources, supported)
            .into_iter()
            .find(|i| i.action == action)
            .ok_or("action is not available in this workspace")?;
        if let Some(reason) = info.disabled_reason {
            return Err(reason);
        }
        match action {
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
                let line = format!(
                    ":scope downstream {} {} {}",
                    entry.namespace,
                    entry.command,
                    entry.args.join(" ")
                );
                sources
                    .bind_semantic_target(Some(scratch))
                    .map_err(|e| e.to_string())?;
                self.output_errors.push_str(&format!("Downstream {}: scratch {scratch}, source {source:?}, language {language}, committed revision {version} (historical attribution; not semantic approval)\n", entry.id));
                self.show_result = false;
                self.output
                    .submit(&line, app, &mut InteractiveSession::new(), host, sources)
                    .map_err(|e| e.to_string())?;
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
                sources.create("scope-script", "");
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
                self.output_errors.push_str("Actions: F1 search, Up/Down select, Enter invoke, Esc cancel.\nNew Script scratch explicitly changes selection. Ctrl-S commits in memory; F5 runs committed selected scope-script, never semantic target.\nOutput: F6 focus, Up/Down or PageUp/PageDown scroll, Alt-Left/Right history; F3 returns to previous pane.\nF4 commands and F2 shell remain available. Results indicate interpreter completion only, not semantic approval.\nDirty drafts (including parked/unrelated drafts) conservatively block runs. F1 lists blockers. No implicit commit, save, or native authority.\n");
            }
        }
        Ok(())
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
        self.actions(sources, supported)
            .into_iter()
            .filter(|i| {
                format!("{} {}", i.id, i.label)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect()
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
            Paragraph::new(text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Actions: {} • Esc cancel", picker.query)),
            ),
            area,
        );
    }
}
