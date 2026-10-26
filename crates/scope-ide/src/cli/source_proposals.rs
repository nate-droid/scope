use std::io::Write;

use crate::{
    proposal::{ProposalId, ProposalStatus},
    scratch::ScratchBuffers,
    source_edit::{SourceEditController, SourceId},
    text::{Position, TextEdit, TextRange},
};

const DIFF_BYTES: usize = 16 * 1024;

fn target_label(target: SourceId, version: u64) -> String {
    match target {
        SourceId::Scratch(id) => {
            format!("owner=shell-buffer source=Scratch({id}) version={version}")
        }
        SourceId::Document(id) => {
            format!(
                "owner=app-document source=Document({}) version={version}",
                id.as_u64()
            )
        }
    }
}

fn print_target(
    proposal: &crate::proposal::SourceEditProposal,
    output: &mut impl Write,
) -> std::io::Result<()> {
    writeln!(
        output,
        "target: {} (proposal baseline; path is destination context only)",
        target_label(proposal.target(), proposal.expected_version())
    )
}

#[cfg(all(test, feature = "agent"))]
mod configured_tests;

/// Target directives live inside the existing JSON-quoted agent instruction,
/// since the shell parser is shared with commands outside this module.
#[derive(Debug, PartialEq, Eq)]
enum EditTarget {
    Buffer(u64),
    Path(std::path::PathBuf),
}

fn edit_request(request: &str) -> Result<(Option<EditTarget>, &str), String> {
    let request = request.trim();
    let (target, instruction) = if let Some(rest) = request.strip_prefix("@buffer:") {
        let (id, instruction) = rest
            .split_once(char::is_whitespace)
            .ok_or("@buffer requires an unsigned buffer ID and instruction")?;
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("@buffer requires an unsigned buffer ID".into());
        }
        (
            EditTarget::Buffer(id.parse().map_err(|_| "invalid buffer ID")?),
            instruction,
        )
    } else if let Some(rest) = request.strip_prefix("@path:") {
        let mut values = serde_json::Deserializer::from_str(rest).into_iter::<String>();
        let path = values
            .next()
            .ok_or("@path requires a JSON-quoted path")?
            .map_err(|_| "@path requires a JSON-quoted path")?;
        let instruction = &rest[values.byte_offset()..];
        if path.is_empty() || !instruction.starts_with(char::is_whitespace) {
            return Err("@path requires a nonempty path followed by an instruction".into());
        }
        (EditTarget::Path(path.into()), instruction)
    } else {
        return Ok((None, request));
    };
    let instruction = instruction.trim();
    if instruction.is_empty() {
        return Err("target directive requires an instruction".into());
    }
    Ok((Some(target), instruction))
}

#[cfg(feature = "agent")]
fn edit_buffer<'a>(
    buffers: &'a ScratchBuffers,
    target: Option<&EditTarget>,
) -> Result<&'a crate::scratch::ScratchBuffer, Box<dyn std::error::Error>> {
    use crate::source_edit::{SourceCandidate, SourceId, TargetSelector, resolve_edit_target};
    let candidates: Vec<_> = buffers
        .list()
        .map(|buffer| SourceCandidate {
            id: SourceId::Scratch(buffer.id()),
            path: buffer
                .destination_path()
                .map(|path| std::path::Path::new(path.as_str())),
            language: buffer.language_id(),
        })
        .collect();
    let user = target.map(|target| match target {
        EditTarget::Buffer(id) => TargetSelector::Id(SourceId::Scratch(*id)),
        EditTarget::Path(path) => TargetSelector::Path(path),
    });
    let selected = buffers
        .selected()
        .ok()
        .map(|buffer| SourceId::Scratch(buffer.id()));
    let id = resolve_edit_target(&candidates, user, selected, None)?;
    match id {
        SourceId::Scratch(id) => Ok(buffers.get(id)?),
        SourceId::Document(_) => {
            Err("CLI agent edits require an existing scratch/workspace buffer".into())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Command {
    Propose(String),
    AgentEdit(String),
    List,
    Review(ProposalId),
    Diff(ProposalId),
    Accept(ProposalId),
    Reject(ProposalId),
}

impl Command {
    pub(super) fn parse(name: &str, arguments: &str) -> Result<Self, String> {
        if name == "source-propose" {
            if arguments.trim().is_empty() {
                return Err("source-propose requires JSON".into());
            }
            return Ok(Self::Propose(arguments.trim().into()));
        }
        if name == "source-proposals" {
            return if arguments.trim().is_empty() {
                Ok(Self::List)
            } else {
                Err("source-proposals takes no arguments".into())
            };
        }
        let mut parts = arguments.split_whitespace();
        let id = parts.next().ok_or("missing source proposal ID")?;
        if !id.bytes().all(|byte| byte.is_ascii_digit()) || parts.next().is_some() {
            return Err("expected one unsigned source proposal ID".into());
        }
        let id = ProposalId(id.parse().map_err(|_| "invalid source proposal ID")?);
        Ok(match name {
            "source-proposal" => Self::Review(id),
            "source-diff" => Self::Diff(id),
            "source-accept" => Self::Accept(id),
            "source-reject" => Self::Reject(id),
            _ => return Err("unknown source proposal command".into()),
        })
    }
}

/// One fixed, host-owned workflow for interactive `agent edit` commands.
/// Configuration requires evaluated editing; failures never fall back to generation.
#[cfg(feature = "agent")]
pub struct InteractiveEditConfig {
    /// Explicitly selected semantic service; independent of REPL evaluator selection.
    pub service: &'static dyn scope_language::LanguageService,
    /// Fixed for the shell's controller: Strict, with host-owned accepted mandatory criteria.
    pub task: crate::task_contract::TaskContract,
    /// Explicit capabilities and bounded execution settings. Selection-only operations are unsupported.
    pub options: crate::agent::SemanticEditOptions,
    /// Concrete-call authorization, including any candidate execution performed by the provider.
    pub authorize: Box<dyn for<'a> FnMut(crate::tool::semantic::SemanticAuthorization<'a>) -> bool>,
}

#[derive(Default)]
pub(super) struct Proposals {
    controller: SourceEditController,
    #[cfg(feature = "agent")]
    editing: Option<InteractiveEditConfig>,
}

impl Proposals {
    pub(super) fn compact(
        &mut self,
        source: &crate::scratch::ScratchBuffer,
        reduction: super::ScriptReduction,
        output: &mut impl Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::proposal::SourceEditProposal;
        if reduction.edits.is_empty() {
            return Err("reducer returned no edits".into());
        }
        let rationale = format!(
            "Rule: {}\nChecked preconditions: {:?}\nEvaluation snapshot: {:?}\nAssurance: unevaluated; local rule only, not validation evidence; fresh whole-snapshot execution only",
            reduction.rule, reduction.checked, reduction.original,
        );
        let proposal = SourceEditProposal::new(
            self.controller.next()?,
            source,
            reduction.edits,
            Some(rationale.clone()),
        )?;
        let candidate = proposal
            .preview(source)
            .map_err(crate::source_edit::ControllerError::Validation)?;
        if candidate == source.text() {
            return Err("reducer returned an unchanged candidate".into());
        }
        let diff = proposal
            .unified_diff(source)
            .map_err(crate::source_edit::ControllerError::Validation)?;
        print_target(&proposal, output)?;
        writeln!(output, "{}", escaped_lines(&rationale))?;
        writeln!(output, "{}", escaped_lines(&diff))?;
        match self.controller.register(proposal, source) {
            Ok(id) => writeln!(
                output,
                "source proposal {} pending; not saved or evaluated; :source-accept {}, :source-reject {}",
                id.0, id.0, id.0,
            )?,
            Err(error) => writeln!(
                output,
                "Preview only: {error}; no proposal registered; configured assurance unchanged."
            )?,
        }
        Ok(())
    }

    pub(super) fn controller_mut(&mut self) -> &mut SourceEditController {
        &mut self.controller
    }

    /// Closure invalidates pending proposals, without retargeting or reopening.
    /// Revision changes remain pending so normal freshness diagnostics explain them.
    pub(super) fn observe(&mut self, buffers: &ScratchBuffers) {
        let closed: Vec<_> = self.controller.list()
            .filter(|proposal| proposal.status() == ProposalStatus::Pending)
            .filter(|proposal| matches!(proposal.target(), SourceId::Scratch(id) if buffers.get(id).is_err()))
            .map(|proposal| proposal.id())
            .collect();
        for id in closed {
            self.controller.discard(id).expect("listed proposal exists");
        }
    }

    #[cfg(feature = "agent")]
    pub(super) fn requires_evaluation(&self) -> bool {
        self.editing.is_some()
    }

    #[cfg(feature = "agent")]
    pub(super) fn configured(
        editing: Option<InteractiveEditConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let controller = if let Some(config) = &editing {
            use crate::task_contract::AssurancePolicy;
            if config.task.policy != AssurancePolicy::Strict
                || !config
                    .task
                    .criteria
                    .values()
                    .any(|c| c.is_accepted() && c.mandatory)
                || config.task.criteria.values().any(|c| {
                    c.is_accepted()
                        && matches!(c.origin, scope_language::ExpectationOrigin::Agent(_))
                })
            {
                return Err("configured editing requires Strict assurance and host-owned accepted mandatory criteria".into());
            }
            SourceEditController::with_task_contract(config.task.clone())
        } else {
            SourceEditController::default()
        };
        Ok(Self {
            controller,
            editing,
        })
    }

    #[cfg(feature = "agent")]
    pub(super) fn validate_request(
        &self,
        request: &str,
        buffers: &ScratchBuffers,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (target, _) = edit_request(request)?;
        let buffer = edit_buffer(buffers, target.as_ref())?;
        if let Some(editing) = &self.editing {
            if buffer.language_id() != editing.service.language_id() {
                return Err(
                    "configured editing requires a buffer matching the host language service"
                        .into(),
                );
            }
            let operations = editing.service.semantic_operations();
            let supported = |name: &str, effect| {
                operations.iter().any(|op| {
                    name == format!("language.{}.{}", editing.service.language_id(), op.id)
                        && op.effect == effect
                        && op.target == scope_language::SemanticTargetRequirement::Document
                })
            };
            if editing.options.capabilities.propose.is_empty()
                || editing.options.capabilities.propose.iter().any(|name| {
                    !supported(
                        name,
                        scope_language::SemanticOperationEffect::ProposeChanges,
                    )
                })
                || editing
                    .options
                    .capabilities
                    .inspect
                    .iter()
                    .any(|name| !supported(name, scope_language::SemanticOperationEffect::Read))
            {
                return Err(
                    "configured editing requires compatible document semantic capabilities".into(),
                );
            }
        }
        Ok(())
    }

    #[cfg(feature = "agent")]
    pub(super) fn generate(
        &mut self,
        model: &mut impl crate::agent::DecisionModel,
        instruction: &str,
        buffers: &ScratchBuffers,
        output: &mut impl Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.validate_request(instruction, buffers)?;
        let (target, instruction) = edit_request(instruction)?;
        let buffer = edit_buffer(buffers, target.as_ref())?;
        if let Some(editing) = &mut self.editing {
            let run = crate::agent::run_semantic_edit(
                model,
                &mut self.controller,
                buffer,
                editing.service,
                crate::source_edit::AgentEditRequest::new(buffer, instruction),
                &editing.options,
                &mut editing.authorize,
            );
            let (crate::agent::EditOutcome::ProposalReady, Some(id)) = (run.outcome, run.proposal)
            else {
                return Err(format!(
                    "configured editing stopped: {:?}; configured checks not established; {} proposal attempts, {} corrections; no proposal applied",
                    run.outcome, run.proposal_attempts, run.corrections
                )
                .into());
            };
            let proposal = self.controller.get(id)?;
            print_target(proposal, output)?;
            writeln!(
                output,
                "source proposal {} ready; pending; source unchanged; not saved; source-diff {}",
                id.0, id.0
            )?;
            if let Some(evidence) = proposal.evidence() {
                writeln!(
                    output,
                    "Validation: configured observations (not universal correctness)"
                )?;
                for (check, criterion) in &editing.task.criteria {
                    let outcome = evidence
                        .observations
                        .checks
                        .get(check)
                        .map(|observation| observation.outcome)
                        .unwrap_or(scope_language::CheckOutcome::NotEvaluated);
                    writeln!(
                        output,
                        "  {}: {:?} (accepted: {}, mandatory: {})",
                        serde_json::to_string(check)?,
                        outcome,
                        criterion.is_accepted(),
                        criterion.mandatory
                    )?;
                }
            } else {
                writeln!(output, "Not evaluated")?;
            }
            return Ok(());
        }
        let id = self.controller.generate(
            model,
            buffer,
            instruction,
            crate::agent_edit::SourceEditLimits::default(),
        )?;
        let next = id.0;
        let proposal = self.controller.get(id)?;
        print_target(proposal, output)?;
        writeln!(
            output,
            "source proposal {next}: buffer {}, version {}, {} edits; pending, not saved or evaluated; source-diff {next}, source-accept {next}, source-reject {next}",
            proposal.target_buffer(),
            proposal.expected_version(),
            proposal.edits().len()
        )?;
        Ok(())
    }

    pub(super) fn run(
        &mut self,
        command: Command,
        buffers: &mut ScratchBuffers,
        output: &mut impl Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match command {
            Command::AgentEdit(request) => {
                edit_request(&request)?;
                return Err("agent editing requires the agent capability".into());
            }
            Command::Propose(json) => {
                let (edits, rationale) = payload(&json)?;
                let id = self
                    .controller
                    .propose(buffers.selected()?, edits, rationale)?;
                let next = id.0;
                let proposal = self.controller.get(id)?;
                print_target(proposal, output)?;
                writeln!(
                    output,
                    "source proposal {next} created for buffer {} at version {}; pending, not saved or evaluated; source-proposal {next}, source-diff {next}, source-accept {next}, source-reject {next}",
                    proposal.target_buffer(),
                    proposal.expected_version()
                )?;
            }
            Command::List => {
                let mut count = 0;
                for proposal in self
                    .controller
                    .list()
                    .filter(|p| p.status() == ProposalStatus::Pending)
                {
                    print_target(proposal, output)?;
                    writeln!(
                        output,
                        "source proposal {}: buffer {}, version {}, {} edits (pending)",
                        proposal.id().0,
                        proposal.target_buffer(),
                        proposal.expected_version(),
                        proposal.edits().len()
                    )?;
                    count += 1;
                }
                writeln!(output, "{count} pending source proposals")?;
            }
            Command::Review(id) | Command::Diff(id) | Command::Accept(id) | Command::Reject(id) => {
                let proposal = self.controller.get(id)?;
                print_target(proposal, output)?;
                // Never reinterpret a document identity as a shell buffer ID.
                let target_buffer = match proposal.target() {
                    SourceId::Scratch(id) => id,
                    SourceId::Document(_) if matches!(command, Command::Reject(_)) => {
                        self.controller.reject(id)?;
                        writeln!(
                            output,
                            "source proposal {}: {:?}",
                            id.0,
                            self.controller.get(id)?.status()
                        )?;
                        return Ok(());
                    }
                    SourceId::Document(_) => {
                        return Err("source proposal target is not a shell buffer".into());
                    }
                };
                match command {
                    Command::Reject(_) => {
                        self.controller.reject(id)?;
                        writeln!(
                            output,
                            "source proposal {}: {:?}",
                            id.0,
                            self.controller.get(id)?.status()
                        )?;
                    }
                    Command::Accept(_) => {
                        self.controller
                            .accept(id, buffers.get_mut(target_buffer)?)?;
                        writeln!(
                            output,
                            "applied target: {}",
                            target_label(
                                SourceId::Scratch(target_buffer),
                                buffers.get(target_buffer)?.version()
                            )
                        )?;
                        let assurance = if self.controller.get(id)?.evidence().is_some() {
                            "not saved; candidate observations retained, not re-evaluated on acceptance"
                        } else {
                            "not saved or evaluated"
                        };
                        writeln!(
                            output,
                            "source proposal {} applied to buffer {}; {}",
                            id.0, target_buffer, assurance
                        )?;
                    }
                    _ => {
                        let buffer = buffers.get(target_buffer)?;
                        self.controller.preview(id, buffer)?;
                        let proposal = self.controller.get(id)?;
                        writeln!(
                            output,
                            "source proposal {}: buffer {}, language {}, destination {}, version {}",
                            id.0,
                            buffer.id(),
                            escaped(buffer.language_id()),
                            escaped(
                                buffer
                                    .destination_path()
                                    .map(|p| p.as_str())
                                    .unwrap_or("(unbound)")
                            ),
                            proposal.expected_version()
                        )?;
                        if matches!(command, Command::Diff(_)) {
                            let diff = self.controller.diff_bounded(id, buffer, DIFF_BYTES)?;
                            writeln!(output, "{}", escaped_lines(&diff.text))?;
                            if diff.truncated {
                                writeln!(
                                    output,
                                    "[diff truncated; display only, acceptance uses all edits]"
                                )?;
                            }
                        } else {
                            writeln!(output, "{}", proposal.to_json())?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn escaped(text: &str) -> String {
    text.chars().flat_map(char::escape_debug).collect()
}

fn escaped_lines(text: &str) -> String {
    text.split('\n').map(escaped).collect::<Vec<_>>().join("\n")
}

fn object<'a>(
    value: &'a serde_json::Value,
    fields: &[&str],
) -> Result<&'a serde_json::Map<String, serde_json::Value>, String> {
    let object = value.as_object().ok_or("expected JSON object")?;
    if object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err("unknown JSON field in source proposal".into());
    }
    Ok(object)
}

fn position(value: &serde_json::Value) -> Result<Position, String> {
    let value = object(value, &["line", "column"])?;
    let coordinate = |key| {
        value
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("{key} must be a nonnegative integer coordinate"))
    };
    Ok(Position::new(coordinate("line")?, coordinate("column")?))
}

fn payload(json: &str) -> Result<(Vec<TextEdit>, Option<String>), String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid source proposal JSON: {e}"))?;
    let value = object(&value, &["edits", "rationale"])?;
    let rationale = match value.get("rationale") {
        None => None,
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        _ => return Err("rationale must be a string when provided".into()),
    };
    let edits = value
        .get("edits")
        .and_then(serde_json::Value::as_array)
        .ok_or("edits must be an array")?;
    let edits = edits
        .iter()
        .map(|edit| {
            let edit = object(edit, &["range", "replacement"])?;
            let range = object(edit.get("range").ok_or("missing range")?, &["start", "end"])?;
            Ok(TextEdit::replace(
                TextRange::new(
                    position(range.get("start").ok_or("missing start")?)?,
                    position(range.get("end").ok_or("missing end")?)?,
                ),
                edit.get("replacement")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("replacement must be a string")?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((edits, rationale))
}

#[cfg(test)]
mod tests {
    #[test]
    fn compact_retains_original_owner_and_unevaluated_assurance() {
        use super::*;
        use crate::text::{Span, TextBuffer, TextEdit};
        let mut buffers = ScratchBuffers::default();
        buffers.create("scope-script", "let mut n=6; n=8; n");
        let source = buffers.selected().unwrap();
        let owner = source.id();
        let original = source.text().to_owned();
        let snapshot = scope_language::SourceSnapshot {
            source_id: source.evaluation_source_id().clone(),
            revision_id: scope_language::RevisionId::from_version(source.version()),
            text: original.clone().into(),
        };
        let reduction = super::super::ScriptReduction {
            original: snapshot,
            edits: vec![TextEdit::replace(
                TextBuffer::new(&original)
                    .range_for_span(Span::new(0, original.len()))
                    .unwrap(),
                "let mut n=8; n",
            )],
            rule: "test supplied rule".into(),
            checked: vec!["test precondition".into()],
        };
        let mut proposals = Proposals::default();
        let mut output = Vec::new();
        proposals
            .compact(source, reduction.clone(), &mut output)
            .unwrap();
        assert_eq!(buffers.get(owner).unwrap().text(), original);
        let id = proposals.controller.list().next().unwrap().id();
        assert!(proposals.controller.get(id).unwrap().evidence().is_none());
        let display = String::from_utf8(output).unwrap();
        assert!(display.contains("test precondition"));
        assert!(display.contains("unevaluated"));
        buffers.create("scope-script", "other");
        proposals
            .run(Command::Accept(id), &mut buffers, &mut Vec::new())
            .unwrap();
        assert_eq!(buffers.get(owner).unwrap().text(), "let mut n=8; n");
        assert_eq!(buffers.selected().unwrap().text(), "other");

        let mut blocked = Proposals::default();
        blocked.controller =
            SourceEditController::with_validation_contract(scope_language::ValidationContract {
                id: "required".into(),
                origin: scope_language::ExpectationOrigin::User("test".into()),
                checks: std::collections::BTreeMap::new(),
            });
        let mut baseline = ScratchBuffers::default();
        baseline.create("scope-script", original);
        let mut output = Vec::new();
        blocked
            .compact(baseline.selected().unwrap(), reduction, &mut output)
            .unwrap();
        assert_eq!(blocked.controller.list().count(), 0);
        assert!(String::from_utf8(output).unwrap().contains("Preview only"));
    }
    #[test]
    fn proposal_target_label_is_owner_tagged_and_uses_supplied_version() {
        assert_eq!(
            super::target_label(crate::source_edit::SourceId::Scratch(7), 42),
            "owner=shell-buffer source=Scratch(7) version=42"
        );
        assert_eq!(
            super::target_label(
                crate::source_edit::SourceId::Document(crate::document::DocumentId::new(7)),
                3
            ),
            "owner=app-document source=Document(7) version=3"
        );
    }

    use super::*;

    #[cfg(feature = "agent")]
    #[test]
    fn mock_agent_registers_reviewable_proposal_without_applying() {
        struct Mock;
        impl crate::agent::DecisionModel for Mock {
            fn decide(
                &mut self,
                messages: &[crate::agent::DecisionMessage],
                _: u32,
            ) -> Result<String, String> {
                let context: serde_json::Value =
                    serde_json::from_str(&messages[1].content).unwrap();
                Ok(serde_json::json!({
                    "type": "source_edit", "target_buffer": context["target_buffer"],
                    "expected_version": context["expected_version"], "expected_source": context["source"],
                    "edits": [{"range": {"start": {"line": 0, "column": 0}, "end": {"line": 0, "column": 1}}, "replacement": "changed"}],
                    "rationale": "review me"
                }).to_string())
            }
        }
        let mut buffers = ScratchBuffers::default();
        buffers.create("no-evaluator", "a");
        let mut proposals = Proposals::configured(None).unwrap();
        let mut output = Vec::new();
        proposals
            .generate(&mut Mock, "change", &buffers, &mut output)
            .unwrap();
        assert_eq!(buffers.selected().unwrap().text(), "a");
        assert_eq!(buffers.selected().unwrap().version(), 0);
        assert!(buffers.selected().unwrap().destination_path().is_none());
        assert_eq!(
            proposals.controller.get(ProposalId(1)).unwrap().status(),
            ProposalStatus::Pending
        );
        let readiness = String::from_utf8(output.clone()).unwrap();
        assert!(readiness.contains("pending, not saved or evaluated"));
        assert!(readiness.contains("source-diff 1"));
        assert!(!readiness.contains("checks passed"));
        assert!(!readiness.contains("correctness"));
        proposals
            .run(Command::Diff(ProposalId(1)), &mut buffers, &mut output)
            .unwrap();
        proposals
            .run(Command::Reject(ProposalId(1)), &mut buffers, &mut output)
            .unwrap();
        assert_eq!(buffers.selected().unwrap().text(), "a");
        proposals
            .generate(&mut Mock, "change", &buffers, &mut output)
            .unwrap();
        proposals
            .run(Command::Accept(ProposalId(2)), &mut buffers, &mut output)
            .unwrap();
        assert_eq!(buffers.selected().unwrap().text(), "changed");
        assert!(buffers.selected().unwrap().is_dirty());
        assert!(buffers.selected().unwrap().destination_path().is_none());
    }

    #[test]
    fn target_directives_preserve_legacy_requests_and_parse_paths_with_spaces() {
        assert_eq!(edit_request("change this"), Ok((None, "change this")));
        assert_eq!(
            edit_request("@buffer:12 change"),
            Ok((Some(EditTarget::Buffer(12)), "change"))
        );
        assert_eq!(
            edit_request(r#"@path:"src/a b.rs" change"#),
            Ok((Some(EditTarget::Path("src/a b.rs".into())), "change"))
        );
        for request in [
            "@buffer:1",
            "@buffer:-1 change",
            "@buffer:18446744073709551616 change",
            "@path:unquoted change",
            r#"@path:"" change"#,
            r#"@path:"a"change"#,
            r#"@path:"a" "#,
        ] {
            assert!(edit_request(request).is_err(), "{request}");
        }
    }

    #[cfg(feature = "agent")]
    #[test]
    fn explicit_target_overrides_selection_without_selecting_or_mutating() {
        let mut buffers = ScratchBuffers::default();
        let first = buffers.create("", "first");
        let second = buffers.create("rust", "second");
        assert_eq!(edit_buffer(&buffers, None).unwrap().id(), second);
        assert_eq!(
            edit_buffer(&buffers, Some(&EditTarget::Buffer(first)))
                .unwrap()
                .id(),
            first
        );
        assert!(edit_buffer(&buffers, Some(&EditTarget::Buffer(999))).is_err());
        assert!(edit_buffer(&buffers, Some(&EditTarget::Path("unopened.rs".into()))).is_err());
        assert_eq!(buffers.selected().unwrap().id(), second);
        assert_eq!(buffers.get(first).unwrap().text(), "first");
        assert_eq!(buffers.get(first).unwrap().version(), 0);
    }

    const JSON: &str = r#"{"edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":1}},"replacement":"changed"}],"rationale":"review me"}"#;

    #[test]
    fn agent_edit_parser_requires_explicit_quoted_instruction() {
        assert!(
            matches!(super::super::ShellCommand::parse("agent edit \"change\"").unwrap(), super::super::ShellCommand::Source(Command::AgentEdit(s)) if s == "change")
        );
        for input in [
            "agent edit",
            "agent edit unquoted",
            "agent edit \"\"",
            "agent edit \"change\" extra",
        ] {
            assert!(super::super::ShellCommand::parse(input).is_err());
        }
    }

    #[cfg(not(feature = "agent"))]
    #[test]
    fn missing_agent_capability_preserves_source() {
        let mut buffers = ScratchBuffers::default();
        buffers.create("unused", "original");
        let mut proposals = Proposals::default();
        let mut output = Vec::new();
        let error = proposals
            .run(
                Command::AgentEdit("change".into()),
                &mut buffers,
                &mut output,
            )
            .unwrap_err();
        assert!(error.to_string().contains("requires the agent capability"));
        assert!(output.is_empty());
        assert_eq!(buffers.selected().unwrap().text(), "original");
        assert!(proposals.controller.list().next().is_none());
    }

    #[test]
    fn source_proposal_lifecycle_routes_without_selection_save_or_execution() {
        let mut buffers = ScratchBuffers::default();
        buffers.create("no-evaluator", "a");
        let target = buffers.selected().unwrap().id();
        let mut proposals = Proposals::default();
        let mut output = Vec::new();
        proposals
            .run(Command::Propose(JSON.into()), &mut buffers, &mut output)
            .unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            format!(
                "target: owner=shell-buffer source=Scratch({target}) version=0 (proposal baseline; path is destination context only)\nsource proposal 1 created for buffer {target} at version 0; pending, not saved or evaluated; source-proposal 1, source-diff 1, source-accept 1, source-reject 1\n"
            )
        );
        assert_eq!(
            proposals.controller.get(ProposalId(1)).unwrap().status(),
            ProposalStatus::Pending
        );
        assert_eq!(buffers.selected().unwrap().text(), "a");
        assert_eq!(buffers.selected().unwrap().version(), 0);
        assert!(buffers.selected().unwrap().is_dirty());
        buffers.create("other", "untouched");
        let selected = buffers.selected().unwrap().id();
        for command in [
            Command::List,
            Command::Review(ProposalId(1)),
            Command::Diff(ProposalId(1)),
            Command::Accept(ProposalId(1)),
        ] {
            proposals.run(command, &mut buffers, &mut output).unwrap();
            assert_eq!(buffers.selected().unwrap().id(), selected);
        }
        let feedback = String::from_utf8(output.clone()).unwrap();
        assert!(feedback.contains(&format!(
            "target: owner=shell-buffer source=Scratch({target}) version=0"
        )));
        assert!(feedback.contains(&format!(
            "applied target: owner=shell-buffer source=Scratch({target}) version=1"
        )));
        assert!(!feedback.contains(&format!("source=Scratch({selected})")));
        assert_eq!(buffers.get(target).unwrap().text(), "changed");
        assert!(buffers.get(target).unwrap().is_dirty());
        assert!(buffers.get(target).unwrap().destination_path().is_none());
        assert_eq!(buffers.selected().unwrap().text(), "untouched");
        assert!(
            proposals
                .run(Command::Accept(ProposalId(1)), &mut buffers, &mut output)
                .is_err()
        );
        assert_eq!(buffers.get(target).unwrap().version(), 1);
    }

    #[test]
    fn proposal_without_selected_target_does_not_advertise_success() {
        let mut buffers = ScratchBuffers::default();
        let mut proposals = Proposals::default();
        let mut output = Vec::new();
        assert!(
            proposals
                .run(Command::Propose(JSON.into()), &mut buffers, &mut output)
                .is_err()
        );
        assert!(output.is_empty());
        assert!(proposals.controller.list().next().is_none());
    }

    #[test]
    fn source_proposal_stale_between_preview_and_accept_and_rejection() {
        let mut buffers = ScratchBuffers::default();
        buffers.create("unused", "a");
        let mut proposals = Proposals::default();
        let mut output = Vec::new();
        proposals
            .run(Command::Propose(JSON.into()), &mut buffers, &mut output)
            .unwrap();
        proposals
            .run(Command::Diff(ProposalId(1)), &mut buffers, &mut output)
            .unwrap();
        buffers
            .selected_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 1), "!"))
            .unwrap();
        for command in [
            Command::Review(ProposalId(1)),
            Command::Diff(ProposalId(1)),
            Command::Accept(ProposalId(1)),
        ] {
            let error = proposals
                .run(command, &mut buffers, &mut output)
                .unwrap_err();
            assert!(error.to_string().contains("RevisionConflict"));
        }
        proposals
            .run(Command::Reject(ProposalId(1)), &mut buffers, &mut output)
            .unwrap();
        proposals
            .run(Command::Reject(ProposalId(1)), &mut buffers, &mut output)
            .unwrap();
        assert!(
            proposals
                .run(Command::Accept(ProposalId(1)), &mut buffers, &mut output)
                .is_err()
        );
        assert_eq!(buffers.selected().unwrap().text(), "a!");
        assert_eq!(buffers.selected().unwrap().version(), 1);
        assert_eq!(
            proposals.controller.get(ProposalId(1)).unwrap().status(),
            ProposalStatus::Rejected
        );
    }

    #[test]
    fn source_proposal_invalid_input_and_ranges_are_transactional() {
        let mut buffers = ScratchBuffers::default();
        buffers.create("unused", "é");
        let mut proposals = Proposals::default();
        for json in [
            "not JSON",
            "{}",
            r#"{"edits":[],"capabilities":[]}"#,
            r#"{"edits":[],"rationale":false}"#,
            JSON,
        ] {
            let mut output = Vec::new();
            assert!(
                proposals
                    .run(Command::Propose(json.into()), &mut buffers, &mut output)
                    .is_err()
            );
            assert!(
                output.is_empty(),
                "failed proposals must not advertise review actions"
            );
        }
        assert!(proposals.controller.list().next().is_none());
        let id = proposals
            .controller
            .propose(
                buffers.selected().unwrap(),
                vec![TextEdit::insert(Position::new(0, 0), "valid")],
                None,
            )
            .unwrap();
        assert_eq!(id, ProposalId(1));
        assert_eq!(buffers.selected().unwrap().text(), "é");
        assert_eq!(buffers.selected().unwrap().version(), 0);
        assert_eq!(escaped_lines("+\u{1b}[31m\r\n"), "+\\u{1b}[31m\\r\n");
        for name in [
            "source-proposal",
            "source-diff",
            "source-accept",
            "source-reject",
        ] {
            for arguments in ["", "-1", "1 extra", "18446744073709551616"] {
                assert!(Command::parse(name, arguments).is_err());
            }
        }
    }
}

#[cfg(test)]
#[test]
fn source_proposal_shell_routing_never_saves() {
    use crate::{app::App, test_support::TempWorkspace};
    let fixture = TempWorkspace::new("source-proposal-shell");
    fixture.write_file("a", "a");
    let app = App::new(fixture.workspace()).unwrap();
    #[cfg(feature = "agent")]
    let config = crate::agent::AgentConfig::local("unused", "http://localhost:1234/v1");
    #[cfg(not(feature = "agent"))]
    let config = super::AgentConfig;
    let mut output = Vec::new();
    let commands = concat!(
        ":scope scratch open a\n",
        "source-propose {\"edits\":[{\"range\":{\"start\":{\"line\":0,\"column\":0},\"end\":{\"line\":0,\"column\":1}},\"replacement\":\"changed\"}]}\n",
        "source-proposals\nsource-proposal 1\nsource-diff 1\nsource-accept 1\nsource-accept 1\nsource-reject 1\nquit\n"
    );
    super::run_shell(app, config, std::io::Cursor::new(commands), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("1 pending source proposals"));
    assert!(output.contains("not saved or evaluated"));
    assert!(output.contains("source proposal error: Applied"));
    assert_eq!(fixture.read_file("a"), "a");
}
