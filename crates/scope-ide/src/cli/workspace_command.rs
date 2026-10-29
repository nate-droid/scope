//! Explicit workspace submissions through the existing shell dispatcher.
use super::ReadOnlyCommand;
use crate::{app::App, language::InteractiveSession, scratch::ScratchBuffers};

/// Agent entry points for retained workbench actions. Neither action grants tools,
/// semantic operations, source mutation, or filesystem access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceAgentAction {
    Ask,
    ProposeEdit,
}

impl WorkspaceAgentAction {
    /// Build a document-only workbench command. Instructions are JSON encoded
    /// so quotes/newlines cannot become commands.
    /// Pass the workbench's conservative draft blocker, including parked panes.
    pub fn command(
        self,
        request: &str,
        draft_blocker: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        if self == Self::ProposeEdit
            && let Some(reason) = draft_blocker
        {
            return Err(
                format!("cannot propose edits while editor drafts are dirty: {reason}").into(),
            );
        }
        if request.trim().is_empty() {
            return Err("agent request must not be empty".into());
        }
        let command = match self {
            Self::Ask => "document-ask",
            Self::ProposeEdit => "document-propose",
        };
        Ok(format!(
            ":scope {command} {}",
            serde_json::to_string(request)?
        ))
    }
}

/// Retained command session: presentation output and the existing proposal owner.
/// Keep this session paired with the same `ScratchBuffers` owner; source IDs are
/// owner-local. Clearing output does not clear proposals or grant authority.
/// A transcript includes normal shell diagnostics; an accepted submission does not
/// imply that the requested operation succeeded. No output is interpreted as code.
#[derive(Default)]
pub struct WorkspaceCommandOutput {
    transcript: Vec<u8>,
    proposals: super::source_proposals::Proposals,
    #[cfg(feature = "agent")]
    agent_config: Option<super::AgentConfig>,
}

impl std::fmt::Debug for WorkspaceCommandOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceCommandOutput")
            .field("transcript", &self.transcript)
            .finish_non_exhaustive()
    }
}

impl WorkspaceCommandOutput {
    /// Creates the retained session with a validated, host-owned editing contract.
    /// Clearing presentation output never clears this configuration or proposals.
    /// Concrete semantic calls still require the configuration's authorization.
    #[cfg(feature = "agent")]
    pub fn with_editing(
        editing: super::InteractiveEditConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            transcript: Vec::new(),
            proposals: super::source_proposals::Proposals::configured(Some(editing))?,
            agent_config: None,
        })
    }
    #[cfg(feature = "agent")]
    pub(crate) fn set_agent_config(&mut self, config: super::AgentConfig) {
        self.agent_config = Some(config);
    }

    /// Existing controller shared by later source review/accept/reject commands.
    /// Trusted hosts may pass this to `run_semantic_edit_with_discovery`; that
    /// operation still requires its own capability and authorization contract.
    /// Keep the controller paired with this session's original source owner.
    pub fn source_edit_controller(&mut self) -> &mut crate::source_edit::SourceEditController {
        self.proposals.controller_mut()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.transcript
    }

    pub fn clear(&mut self) {
        self.transcript.clear();
    }

    /// Dispatch a document-only workbench action using an explicit host semantic
    /// binding and model. Supply the current draft blocker at invocation time,
    /// not when opening the request prompt. Failures return an error; successful
    /// output is retained in `bytes()`. No backend fallback or grants are installed.
    #[expect(
        clippy::too_many_arguments,
        reason = "Keep host authority and invocation state explicit without changing the dispatch API"
    )]
    pub fn submit_agent(
        &mut self,
        action: WorkspaceAgentAction,
        request: &str,
        draft_blocker: Option<&str>,
        app: &mut App,
        session: &mut InteractiveSession,
        host: Option<&mut dyn ReadOnlyCommand>,
        sources: &mut ScratchBuffers,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let line = action.command(request, draft_blocker)?;
        self.submit(&line, app, session, host, sources)
    }

    #[cfg(not(feature = "agent"))]
    fn submit_document_agent(
        &mut self,
        _: bool,
        _: &str,
        _: Option<&mut dyn ReadOnlyCommand>,
        _: &ScratchBuffers,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Err(super::AGENT_FEATURE_REQUIRED.into())
    }

    #[cfg(feature = "agent")]
    fn submit_document_agent(
        &mut self,
        propose: bool,
        request: &str,
        host: Option<&mut dyn ReadOnlyCommand>,
        sources: &ScratchBuffers,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        let source = sources.selected()?;
        let host = host.ok_or("document agent unavailable: no semantic agent host configured")?;
        let binding = host
            .semantic_agent()
            .ok_or("document agent unavailable: host has no semantic agent binding")?;
        if source.language_id() != binding.service.language_id() {
            return Err("document agent selected scratch language does not match configured semantic service".into());
        }
        // A launcher model is installed only by explicit opt-in. Host-provided
        // models take precedence; service and authorization remain host-owned.
        let mut launcher_model;
        let model: &mut dyn crate::agent::DecisionModel = match binding.model {
            Some(model) => model,
            None => {
                let config = self.agent_config.as_ref().filter(|config| config.launcher_binding())
                    .ok_or("document agent unavailable: an explicit host model is required; no default endpoint fallback")?;
                launcher_model = crate::agent::CellAgent::new(config.clone())?;
                &mut launcher_model
            }
        };
        if propose {
            if !self.proposals.requires_evaluation() {
                return Err(
                    "document proposal unavailable: explicit host editing contract required".into(),
                );
            }
            // Pin the selected identity; request text cannot supply a target directive.
            let instruction = format!("@buffer:{} {request}", source.id());
            self.proposals.validate_request(&instruction, sources)?;
            struct Model<'a>(&'a mut dyn crate::agent::DecisionModel);
            impl crate::agent::DecisionModel for Model<'_> {
                fn decide(
                    &mut self,
                    messages: &[crate::agent::DecisionMessage],
                    tokens: u32,
                ) -> Result<String, String> {
                    self.0.decide(messages, tokens)
                }
            }
            self.proposals.generate(
                &mut Model(model),
                &instruction,
                sources,
                &mut self.transcript,
            )
        } else {
            let outcome = super::run_retained_scratch_agent(
                source,
                binding.service,
                model,
                request,
                binding.limits,
                super::RetainedScratchAgentConfig {
                    allowed_operation_ids: binding.allowed_operation_ids,
                    authorize: binding.authorize,
                    trace: binding.trace,
                },
            )?;
            (binding.observe)(&outcome);
            write!(self.transcript, "{}", outcome.activity_summary())?;
            if source.language_id() == "scope-script" {
                writeln!(
                    self.transcript,
                    "Script evidence: inspect returns source and syntax admission only. No prior Script runtime diagnostics are supplied; syntax is not runtime validation or semantic correctness."
                )?;
            }
            writeln!(
                self.transcript,
                "Document agent: {:?}: {}",
                outcome.run.stop, outcome.run.content
            )?;
            Ok(())
        }
    }

    /// Submit exactly one explicitly namespaced command. Document agent aliases
    /// use only the selected scratch and explicit host binding/model; other
    /// commands use the normal dispatcher, with no terminal input or history.
    /// Pane-local draft checks remain the caller's responsibility for raw
    /// document-propose submissions; `submit_agent` accepts the current blocker.
    /// App, session, scratch selection and independent semantic binding stay with
    /// the caller, including on failure. Host authorization is never synthesized.
    /// Block-entry commands belong in the interactive shell, not this facility.
    pub fn submit(
        &mut self,
        line: &str,
        app: &mut App,
        session: &mut InteractiveSession,
        host: Option<&mut dyn ReadOnlyCommand>,
        sources: &mut ScratchBuffers,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !line.starts_with(":scope ") || line.contains(['\n', '\r']) {
            return Err("workspace submission requires one explicit :scope command line".into());
        }
        // These commands request a block/editor reader rather than one dispatch.
        let command = line[7..].trim();
        if command.split_whitespace().next() == Some("edit") {
            return Err("use the Scope shell for block editing".into());
        }
        let (name, request) = command
            .split_once(char::is_whitespace)
            .unwrap_or((command, ""));
        if matches!(name, "document-ask" | "document-propose") {
            let request = if request.trim_start().starts_with('"') {
                serde_json::from_str::<String>(request.trim())?
            } else {
                request.to_owned()
            };
            if request.trim().is_empty() {
                return Err("document agent request must not be empty".into());
            }
            return self.submit_document_agent(name == "document-propose", &request, host, sources);
        }
        // Only the explicitly opted-in stock route requires document aliases.
        // Embedded hosts retain the existing dispatcher, including contracted
        // `agent edit` with their own language service and authorization policy.
        #[cfg(feature = "agent")]
        if self
            .agent_config
            .as_ref()
            .is_some_and(|config| config.launcher_binding())
            && matches!(name, "agent" | "ask" | "proposal-agent")
        {
            return Err("workbench agents require document-ask or document-propose with an explicit host semantic binding and model; generic App agents are unavailable here".into());
        }
        let input = format!("{line}\n");
        super::run_shell_with_proposals(
            app,
            session,
            #[cfg(feature = "agent")]
            self.agent_config.clone().unwrap_or_else(|| {
                super::AgentConfig::local(super::DEFAULT_AGENT_MODEL, super::DEFAULT_AGENT_BASE_URL)
            }),
            #[cfg(not(feature = "agent"))]
            super::AgentConfig,
            std::io::Cursor::new(input),
            &mut self.transcript,
            false,
            host,
            sources,
            &mut self.proposals,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::Document, text::TextRange};

    #[test]
    fn agent_actions_encode_document_requests_without_command_injection() {
        let request = "replace \"quoted\" text\n:scope source-accept 1";
        let line = WorkspaceAgentAction::ProposeEdit
            .command(request, None)
            .unwrap();
        assert!(!line.contains(['\n', '\r']));
        assert_eq!(
            serde_json::from_str::<String>(line.strip_prefix(":scope document-propose ").unwrap())
                .unwrap(),
            request
        );
        let line = WorkspaceAgentAction::Ask
            .command("explain source", Some("dirty pane"))
            .unwrap();
        assert_eq!(line, ":scope document-ask \"explain source\"");
        for request in ["", "  "] {
            assert!(WorkspaceAgentAction::Ask.command(request, None).is_err());
        }
    }

    #[test]
    fn dirty_agent_proposal_is_rejected_before_dispatch_and_preserves_source() {
        let root = crate::test_support::TempWorkspace::new("workspace-agent-draft");
        let mut app = App::new(root.workspace()).unwrap();
        let mut session = InteractiveSession::new();
        let mut sources = ScratchBuffers::default();
        let id = sources.create("text", "original");
        let mut output = WorkspaceCommandOutput::default();
        let error = output
            .submit_agent(
                WorkspaceAgentAction::ProposeEdit,
                "change source",
                Some("parked editor has a draft"),
                &mut app,
                &mut session,
                None,
                &mut sources,
            )
            .unwrap_err();
        assert!(error.to_string().contains("parked editor"));
        assert!(output.bytes().is_empty());
        assert_eq!(sources.get(id).unwrap().text(), "original");
        assert_eq!(sources.get(id).unwrap().version(), 0);
    }

    #[cfg(not(feature = "agent"))]
    #[test]
    fn agent_actions_report_unavailable_feature_without_mutation() {
        let root = crate::test_support::TempWorkspace::new("workspace-agent-unavailable");
        let mut app = App::new(root.workspace()).unwrap();
        let mut session = InteractiveSession::new();
        let mut sources = ScratchBuffers::default();
        let id = sources.create("text", "original");
        let mut output = WorkspaceCommandOutput::default();
        for action in [WorkspaceAgentAction::Ask, WorkspaceAgentAction::ProposeEdit] {
            output.clear();
            let error = output
                .submit_agent(
                    action,
                    "explain source",
                    None,
                    &mut app,
                    &mut session,
                    None,
                    &mut sources,
                )
                .unwrap_err();
            assert!(error.to_string().contains("agent support is disabled"));
            assert_eq!(sources.get(id).unwrap().text(), "original");
            assert_eq!(sources.get(id).unwrap().version(), 0);
        }
    }

    #[derive(Default)]
    struct Host(Vec<(String, Option<String>)>);
    impl ReadOnlyCommand for Host {
        fn invoke(
            &mut self,
            _: &Document,
            _: TextRange,
            _: &str,
            _: &mut dyn std::io::Write,
        ) -> Result<(), Box<dyn std::error::Error>> {
            panic!("must not fall back to App document")
        }
        fn invoke_script_with_target(
            &mut self,
            source: &crate::scratch::ScratchBuffer,
            target: Option<&crate::scratch::ScratchBuffer>,
            output: &mut dyn std::io::Write,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.0
                .push((source.text().into(), target.map(|s| s.text().into())));
            writeln!(output, "host result")?;
            Ok(())
        }
    }

    #[test]
    fn commands_retain_sources_capture_output_and_resolve_only_explicit_target() {
        let root = crate::test_support::TempWorkspace::new("workspace-command");
        let mut app = App::new(root.workspace()).unwrap();
        let mut session = InteractiveSession::new();
        let mut sources = ScratchBuffers::default();
        let target = sources.create("text", "target");
        let source = sources.create("scope-script", "print(1)");
        sources.bind_semantic_target(Some(target)).unwrap();
        let mut host = Host::default();
        let mut output = WorkspaceCommandOutput::default();
        output
            .submit(
                ":scope script run",
                &mut app,
                &mut session,
                Some(&mut host),
                &mut sources,
            )
            .unwrap();
        assert_eq!(host.0, [("print(1)".into(), Some("target".into()))]);
        assert!(String::from_utf8_lossy(output.bytes()).contains("host result"));
        assert_eq!(sources.selected().unwrap().id(), source);
        assert_eq!(sources.semantic_target().unwrap().unwrap().id(), target);
        for line in [
            format!(":scope scratch select {target}"),
            ":scope scratch discard".into(),
            format!(":scope scratch select {source}"),
        ] {
            output
                .submit(&line, &mut app, &mut session, Some(&mut host), &mut sources)
                .unwrap();
        }
        // Use the normal lifecycle command; unavailable explicit targets must not
        // become unbound or silently select another source.
        output
            .submit(
                ":scope script run",
                &mut app,
                &mut session,
                Some(&mut host),
                &mut sources,
            )
            .unwrap();
        assert_eq!(host.0.len(), 1);
    }

    const PROPOSE: &str = r#":scope source-propose {"edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":1}},"replacement":"changed"}]}"#;

    #[test]
    fn proposals_survive_submissions_and_transcript_clear_without_retargeting() {
        let root = crate::test_support::TempWorkspace::new("retained-proposals");
        let mut app = App::new(root.workspace()).unwrap();
        let mut session = InteractiveSession::new();
        let mut sources = ScratchBuffers::default();
        let original = sources.create("text", "a");
        let mut output = WorkspaceCommandOutput::default();
        output
            .submit(PROPOSE, &mut app, &mut session, None, &mut sources)
            .unwrap();
        let other = sources.create("text", "a");
        output.clear();
        for command in [
            ":scope source-proposals",
            ":scope source-diff 1",
            ":scope source-accept 1",
        ] {
            output
                .submit(command, &mut app, &mut session, None, &mut sources)
                .unwrap();
        }
        assert_eq!(sources.selected().unwrap().id(), other);
        assert_eq!(sources.get(other).unwrap().text(), "a");
        assert_eq!(sources.get(original).unwrap().text(), "changed");
        assert_eq!(sources.get(original).unwrap().version(), 1);
        assert!(String::from_utf8_lossy(output.bytes()).contains("1 pending source proposals"));
        output
            .submit(
                ":scope source-accept 1",
                &mut app,
                &mut session,
                None,
                &mut sources,
            )
            .unwrap();
        assert_eq!(sources.get(original).unwrap().version(), 1);
    }

    #[test]
    fn rejection_staleness_closure_and_bad_ids_never_mutate_source() {
        for failure in ["reject", "stale", "closed", "bad-id", "malformed"] {
            let root = crate::test_support::TempWorkspace::new("retained-proposals-failure");
            let mut app = App::new(root.workspace()).unwrap();
            let mut session = InteractiveSession::new();
            let mut sources = ScratchBuffers::default();
            let original = sources.create("text", "a");
            let mut output = WorkspaceCommandOutput::default();
            output
                .submit(PROPOSE, &mut app, &mut session, None, &mut sources)
                .unwrap();
            let preparation = match failure {
                "reject" => ":scope source-reject 1",
                "stale" => ":scope scratch replace 1 1 1 2 newer",
                "closed" => ":scope scratch discard",
                _ => ":scope context",
            };
            output
                .submit(preparation, &mut app, &mut session, None, &mut sources)
                .unwrap();
            let survivor = if failure == "closed" {
                sources.create("text", "a")
            } else {
                original
            };
            let before = (
                sources.get(survivor).unwrap().text().to_owned(),
                sources.get(survivor).unwrap().version(),
            );
            let accept = match failure {
                "bad-id" => ":scope source-accept 999",
                "malformed" => ":scope source-accept invalid",
                _ => ":scope source-accept 1",
            };
            output.clear();
            output
                .submit(accept, &mut app, &mut session, None, &mut sources)
                .unwrap();
            assert!(
                String::from_utf8_lossy(output.bytes()).contains("error:"),
                "{failure}"
            );
            assert_eq!(
                (
                    sources.get(survivor).unwrap().text().to_owned(),
                    sources.get(survivor).unwrap().version()
                ),
                before,
                "{failure}"
            );
            if failure == "closed" {
                assert_eq!(
                    output
                        .source_edit_controller()
                        .get(crate::proposal::ProposalId(1))
                        .unwrap()
                        .status(),
                    crate::proposal::ProposalStatus::Discarded
                );
                output.clear();
                output
                    .submit(
                        ":scope source-proposals",
                        &mut app,
                        &mut session,
                        None,
                        &mut sources,
                    )
                    .unwrap();
                assert!(
                    String::from_utf8_lossy(output.bytes()).contains("0 pending source proposals")
                );
            }
        }
    }

    #[test]
    fn implicit_evaluation_and_multiline_submissions_are_rejected_before_dispatch() {
        let root = crate::test_support::TempWorkspace::new("workspace-command-reject");
        let mut app = App::new(root.workspace()).unwrap();
        let mut session = InteractiveSession::new();
        let mut sources = ScratchBuffers::default();
        let mut output = WorkspaceCommandOutput::default();
        for line in [
            "print(1)",
            "script run",
            ":scope help\n:scope script run",
            ":scope help\r",
            ":scope edit",
        ] {
            assert!(
                output
                    .submit(line, &mut app, &mut session, None, &mut sources)
                    .is_err()
            );
        }
        assert!(output.bytes().is_empty());
        output
            .submit(
                ":scope script run",
                &mut app,
                &mut session,
                None,
                &mut sources,
            )
            .unwrap();
        assert!(
            String::from_utf8_lossy(output.bytes()).contains("no read-only host command installed")
        );
        output.clear();
        assert!(output.bytes().is_empty());
    }
}
