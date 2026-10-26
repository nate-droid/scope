//! Bounded, declarative host workflows. Native downstream authorization remains host-owned.
use super::{DownstreamOutcome, ReadOnlyCommand, buffer_editor};
use crate::{
    language::LanguageRegistry,
    scratch::{ScratchBuffer, ScratchBuffers},
    workspace::Workspace,
};
use std::io::{BufRead, Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Invocation-local selected-source metadata, never an implicit semantic target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedMetadata {
    pub id: u64,
    pub language: String,
    pub revision: u64,
}
impl SelectedMetadata {
    pub fn from_buffer(buffer: &ScratchBuffer) -> Self {
        Self {
            id: buffer.id(),
            language: buffer.language_id().into(),
            revision: buffer.version(),
        }
    }
}

/// A successful explicit binding. A closed previous target has no live metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingTransition {
    pub previous: Option<SelectedMetadata>,
    pub current: SelectedMetadata,
}

/// Owned invocation snapshots delivered on success or editor cancellation, not errors.
/// Created/opened entries record metadata immediately after each lifecycle operation;
/// `selected` records the final selection, including any committed editor changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompositionReport {
    pub selected: Option<SelectedMetadata>,
    pub created_or_opened: Vec<SelectedMetadata>,
    pub bindings: Vec<BindingTransition>,
    pub canceled: bool,
}

/// Literal typed steps; no expansion, recursion, evaluation, save, or shell dispatch.
#[derive(Clone, Debug)]
pub enum CompositionStep {
    Create {
        language: String,
    },
    Open {
        path: String,
    },
    Select {
        id: u64,
    },
    /// Uses the existing transactional command editor; cancellation stops all later steps.
    Edit,
    /// Validates language, explicitly binds the semantic target, and pins metadata.
    BindSelected {
        expected_language: String,
    },
    /// Requires an earlier binding and passes that exact live source as target.
    Downstream {
        namespace: String,
        command: String,
        args: Vec<String>,
    },
}

/// Invoked only as `:scope NAME COMMAND`, with no additional arguments.
#[derive(Clone, Debug)]
pub struct Composition {
    pub namespace: String,
    pub command: String,
    pub steps: Vec<CompositionStep>,
}

// `scratch new` accepts exactly one whitespace-delimited language token, not
// necessarily a registered evaluator (scratch ownership does not require one).
pub(super) fn valid_language(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_whitespace) && value.len() <= 4096
}

fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

pub(super) fn registrations(host: Option<&dyn ReadOnlyCommand>) -> Result<Vec<Composition>> {
    let entries = host.map_or(&[][..], ReadOnlyCommand::compositions);
    if entries.len() > 64 {
        return Err("at most 64 compositions are allowed".into());
    }
    let reserved = [
        "help",
        "languages",
        "session",
        "session-reset",
        "describe",
        "effect",
        "where",
        "context",
        "inspect",
        "downstream",
        "scratch",
        "line",
        "edit",
        "open",
        "buffers",
        "buffer",
        "show",
        "save",
        "save-as",
        "reload",
        "reload-discard",
        "close",
        "discard",
        "semantic-target",
        "script",
        "source",
        "proposal",
        "agent",
        "quit",
        "exit",
        "capabilities",
        "authority",
        "files",
        "lines",
        "replace",
        "language",
        "outline",
        "diagnostics",
        "dirty",
        "revert",
        "refresh",
        "host",
        "run",
    ];
    for (index, entry) in entries.iter().enumerate() {
        if !name(&entry.namespace)
            || !name(&entry.command)
            || reserved.contains(&entry.namespace.as_str())
            || super::ShellCommand::names().contains(&entry.namespace.as_str())
        {
            return Err("invalid or reserved composition namespace/command".into());
        }
        if entries[..index]
            .iter()
            .any(|e| e.namespace == entry.namespace && e.command == entry.command)
        {
            return Err("duplicate composition command".into());
        }
        if entry.steps.is_empty() || entry.steps.len() > 32 {
            return Err("composition requires 1–32 steps".into());
        }
        for step in &entry.steps {
            let valid = match step {
                CompositionStep::Create { language }
                | CompositionStep::BindSelected {
                    expected_language: language,
                } => valid_language(language),
                CompositionStep::Open { path } => path.len() <= 4096,
                CompositionStep::Downstream {
                    namespace,
                    command,
                    args,
                } => {
                    name(namespace)
                        && name(command)
                        && args.len() <= 64
                        && args.iter().all(|a| a.len() <= 4096)
                }
                _ => true,
            };
            if !valid {
                return Err("invalid or oversized composition step".into());
            }
        }
    }
    Ok(entries.to_vec())
}

pub(super) fn execute(
    entry: &Composition,
    buffers: &mut ScratchBuffers,
    workspace: &Workspace,
    registry: &LanguageRegistry,
    host: &mut Option<&mut dyn ReadOnlyCommand>,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    let mut bound = None;
    let mut report = CompositionReport::default();
    for step in &entry.steps {
        match step {
            CompositionStep::Create { language } => {
                if !valid_language(language) {
                    return Err("scratch new requires exactly one language token".into());
                }
                let id = buffers.create(language.clone(), "");
                buffers.select(id)?;
                report
                    .created_or_opened
                    .push(SelectedMetadata::from_buffer(buffers.get(id)?));
            }
            CompositionStep::Open { path } => {
                let id = buffers.open(workspace, registry, path)?;
                buffers.select(id)?;
                report
                    .created_or_opened
                    .push(SelectedMetadata::from_buffer(buffers.get(id)?));
            }
            CompositionStep::Select { id } => buffers.select(*id)?,
            CompositionStep::Edit => {
                let outcome = buffer_editor::run_with_completion(
                    buffers,
                    input,
                    output,
                    &|source, draft, cursor| {
                        host.as_deref()
                            .map_or_else(Vec::new, |h| h.complete_source(source, draft, cursor))
                    },
                )?;
                if outcome == buffer_editor::EditorOutcome::Canceled {
                    writeln!(output, "Composition canceled; later steps skipped.")?;
                    report.canceled = true;
                    break;
                }
            }
            CompositionStep::BindSelected { expected_language } => {
                let metadata = SelectedMetadata::from_buffer(buffers.selected()?);
                if metadata.language != *expected_language {
                    return Err(format!(
                        "selected language mismatch: expected {expected_language}, found {}",
                        metadata.language
                    )
                    .into());
                }
                let previous_id = buffers.semantic_target_id();
                let previous = previous_id
                    .and_then(|id| buffers.get(id).ok())
                    .map(SelectedMetadata::from_buffer);
                let previous_revision = previous.as_ref().map(|source| source.revision);
                buffers.bind_semantic_target(Some(metadata.id))?;
                writeln!(
                    output,
                    "Semantic target: previous id={previous_id:?} revision={previous_revision:?}; new id={} revision={}",
                    metadata.id, metadata.revision
                )?;
                report.bindings.push(BindingTransition {
                    previous,
                    current: metadata.clone(),
                });
                bound = Some(metadata);
            }
            CompositionStep::Downstream {
                namespace,
                command,
                args,
            } => {
                let expected = bound
                    .as_ref()
                    .ok_or("downstream composition step requires BindSelected")?;
                let source = buffers.selected()?;
                if SelectedMetadata::from_buffer(source) != *expected {
                    return Err("bound source metadata changed".into());
                }
                if buffers.semantic_target_id() != Some(expected.id) {
                    return Err("semantic target binding changed".into());
                }
                let host = host.as_deref_mut().ok_or("no composition host installed")?;
                if !host.downstream_namespaces().contains(&namespace.as_str()) {
                    return Err("unregistered downstream namespace".into());
                }
                let args: Vec<_> = args.iter().map(String::as_str).collect();
                match host.invoke_downstream(namespace, command, &args, Some(source), output)? {
                    DownstreamOutcome::Handled => (),
                    DownstreamOutcome::Unhandled => {
                        return Err("unhandled composition downstream command".into());
                    }
                }
            }
        }
    }
    report.selected = buffers.selected().ok().map(SelectedMetadata::from_buffer);
    if let Some(host) = host.as_deref_mut() {
        host.composition_completed(&report);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    struct Host {
        entries: Vec<Composition>,
        calls: Vec<SelectedMetadata>,
        reports: Vec<CompositionReport>,
    }
    impl ReadOnlyCommand for Host {
        fn composition_completed(&mut self, report: &CompositionReport) {
            self.reports.push(report.clone());
        }
        fn compositions(&self) -> &[Composition] {
            &self.entries
        }
        fn downstream_namespaces(&self) -> &[&str] {
            &["demo"]
        }
        fn invoke(
            &mut self,
            _: &crate::document::Document,
            _: crate::text::TextRange,
            _: &str,
            _: &mut dyn Write,
        ) -> Result<()> {
            panic!("legacy dispatch")
        }
        fn invoke_downstream(
            &mut self,
            _: &str,
            command: &str,
            _: &[&str],
            target: Option<&ScratchBuffer>,
            _: &mut dyn Write,
        ) -> std::result::Result<DownstreamOutcome, super::super::DownstreamError> {
            self.calls
                .push(SelectedMetadata::from_buffer(target.unwrap()));
            if command == "deny" {
                return Err(super::super::DownstreamError::new(
                    "host_denied",
                    "not authorized",
                ));
            }
            Ok(DownstreamOutcome::Handled)
        }
    }
    fn bind() -> CompositionStep {
        CompositionStep::BindSelected {
            expected_language: "text".into(),
        }
    }
    fn downstream() -> CompositionStep {
        CompositionStep::Downstream {
            namespace: "demo".into(),
            command: "read".into(),
            args: vec![],
        }
    }
    #[test]
    fn execution_pins_metadata_and_stops_on_cancel_or_stale_revision() {
        let root = crate::test_support::TempWorkspace::new("composition");
        for (steps, input, calls, fails) in [
            (
                vec![CompositionStep::Edit, bind(), downstream()],
                "hello\n:done\n",
                1,
                false,
            ),
            (
                vec![CompositionStep::Edit, bind(), downstream()],
                ":cancel\n",
                0,
                false,
            ),
            (
                vec![CompositionStep::Edit, bind(), downstream()],
                "",
                0,
                false,
            ),
            (
                vec![bind(), CompositionStep::Edit, downstream()],
                "hello\n:done\n",
                0,
                true,
            ),
        ] {
            let mut buffers = ScratchBuffers::default();
            let target = buffers.create("text", "independent");
            buffers.bind_semantic_target(Some(target)).unwrap();
            let id = buffers.create("text", "");
            let entry = Composition {
                namespace: "demo-flow".into(),
                command: "edit".into(),
                steps,
            };
            let mut host = Host {
                entries: vec![entry.clone()],
                calls: vec![],
                reports: vec![],
            };
            let result = execute(
                &entry,
                &mut buffers,
                &root.workspace(),
                &LanguageRegistry::default(),
                &mut Some(&mut host),
                &mut io::Cursor::new(input),
                &mut Vec::new(),
            );
            assert_eq!(result.is_err(), fails);
            assert_eq!(host.reports.len(), usize::from(!fails));
            if !fails {
                let report = &host.reports[0];
                assert_eq!(report.canceled, input == ":cancel\n" || input.is_empty());
                assert_eq!(
                    report.selected,
                    Some(SelectedMetadata::from_buffer(buffers.selected().unwrap()))
                );
                assert_eq!(report.bindings.len(), calls);
            }
            assert_eq!(host.calls.len(), calls);
            if calls != 0 {
                assert_eq!(
                    host.calls[0],
                    SelectedMetadata::from_buffer(buffers.get(id).unwrap())
                );
            }
            assert_eq!(
                buffers.semantic_target_id(),
                Some(if input == ":cancel\n" || input.is_empty() {
                    target
                } else {
                    id
                })
            );
        }
    }
    #[test]
    fn failures_stop_without_fallback_or_unintended_selection() {
        let root = crate::test_support::TempWorkspace::new("composition-failures");
        for case in [
            "missing",
            "wrong-language",
            "closed",
            "failed-open",
            "changed-selection",
            "denied",
            "invalid-language",
        ] {
            let mut buffers = ScratchBuffers::default();
            let old = buffers.create("text", "old");
            buffers.bind_semantic_target(Some(old)).unwrap();
            let selected = buffers.create("text", "selected");
            if case == "missing" || case == "closed" {
                buffers.discard_selected().unwrap();
            }
            let steps = match case {
                "wrong-language" => vec![
                    CompositionStep::BindSelected {
                        expected_language: "rust".into(),
                    },
                    downstream(),
                ],
                "closed" => vec![
                    CompositionStep::Select { id: selected },
                    bind(),
                    downstream(),
                ],
                "failed-open" => vec![
                    CompositionStep::Open {
                        path: "missing-file.txt".into(),
                    },
                    bind(),
                    downstream(),
                ],
                "changed-selection" => {
                    vec![bind(), CompositionStep::Select { id: old }, downstream()]
                }
                "denied" => vec![
                    bind(),
                    CompositionStep::Downstream {
                        namespace: "demo".into(),
                        command: "deny".into(),
                        args: vec![],
                    },
                    downstream(),
                ],
                "invalid-language" => vec![
                    CompositionStep::Create {
                        language: "two tokens".into(),
                    },
                    bind(),
                    downstream(),
                ],
                _ => vec![bind(), downstream()],
            };
            // Closing the only selected buffer must not cause implicit fallback.
            if case == "missing" {
                buffers.select(old).unwrap();
                buffers.discard_selected().unwrap();
            }
            let before_selection = buffers.selected().ok().map(|s| s.id());
            let entry = Composition {
                namespace: "flow".into(),
                command: "test".into(),
                steps,
            };
            let mut host = Host {
                entries: vec![],
                calls: vec![],
                reports: vec![],
            };
            let mut output = Vec::new();
            let error = execute(
                &entry,
                &mut buffers,
                &root.workspace(),
                &LanguageRegistry::default(),
                &mut Some(&mut host),
                &mut io::Cursor::new(""),
                &mut output,
            )
            .unwrap_err();
            assert_eq!(
                host.calls.len(),
                usize::from(case == "denied"),
                "{case}: {error}"
            );
            let rebound = case == "changed-selection" || case == "denied";
            assert_eq!(
                buffers.semantic_target_id(),
                Some(if rebound { selected } else { old }),
                "{case}"
            );
            assert_eq!(
                buffers.selected().ok().map(|s| s.id()),
                if case == "changed-selection" {
                    Some(old)
                } else {
                    before_selection
                },
                "{case}"
            );
            if case == "denied" {
                assert!(error.to_string().contains("host_denied"));
            }
            if rebound {
                let report = String::from_utf8(output).unwrap();
                assert!(report.contains(&format!("previous id=Some({old}) revision=Some(")));
                assert!(report.contains(&format!("new id={selected} revision=")));
            }
        }
    }
    #[test]
    fn create_accepts_unregistered_language_tokens_like_scratch_new() {
        let root = crate::test_support::TempWorkspace::new("composition-language");
        let mut buffers = ScratchBuffers::default();
        let entry = Composition {
            namespace: "flow".into(),
            command: "new".into(),
            steps: vec![CompositionStep::Create {
                language: "Custom.Language".into(),
            }],
        };
        execute(
            &entry,
            &mut buffers,
            &root.workspace(),
            &LanguageRegistry::default(),
            &mut None,
            &mut io::Cursor::new(""),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(buffers.selected().unwrap().language_id(), "Custom.Language");
    }
    #[test]
    fn report_returns_actual_create_open_and_binding_identities() {
        let root = crate::test_support::TempWorkspace::new("composition-report");
        root.write_file("opened.txt", "live file");
        let mut buffers = ScratchBuffers::default();
        let previous_id = buffers.create("text", "previous");
        buffers.bind_semantic_target(Some(previous_id)).unwrap();
        // Consume an ID so the report cannot accidentally derive IDs from step indices.
        buffers.create("text", "discarded");
        buffers.discard_selected().unwrap();
        let previous = SelectedMetadata::from_buffer(buffers.get(previous_id).unwrap());
        let entry = Composition {
            namespace: "flow".into(),
            command: "start".into(),
            steps: vec![
                CompositionStep::Create {
                    language: "text".into(),
                },
                bind(),
                CompositionStep::Open {
                    path: "opened.txt".into(),
                },
                CompositionStep::BindSelected {
                    expected_language: "text".into(),
                },
            ],
        };
        let mut host = Host {
            entries: vec![],
            calls: vec![],
            reports: vec![],
        };
        execute(
            &entry,
            &mut buffers,
            &root.workspace(),
            &LanguageRegistry::default(),
            &mut Some(&mut host),
            &mut io::Cursor::new(""),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(host.reports.len(), 1);
        let report = &host.reports[0];
        assert!(!report.canceled);
        assert_eq!(report.created_or_opened.len(), 2);
        let created = &report.created_or_opened[0];
        let opened = &report.created_or_opened[1];
        assert!(created.id > previous_id + 1);
        assert_ne!(created.id, opened.id);
        for metadata in &report.created_or_opened {
            assert_eq!(
                *metadata,
                SelectedMetadata::from_buffer(buffers.get(metadata.id).unwrap())
            );
        }
        assert_eq!(buffers.get(opened.id).unwrap().text(), "live file");
        assert_eq!(report.selected.as_ref(), Some(opened));
        assert_eq!(
            report.bindings,
            vec![
                BindingTransition {
                    previous: Some(previous),
                    current: created.clone()
                },
                BindingTransition {
                    previous: Some(created.clone()),
                    current: opened.clone()
                },
            ]
        );
    }
    #[test]
    fn all_parser_names_and_explicit_controls_are_reserved() {
        for namespace in super::super::ShellCommand::names().iter().copied().chain([
            "session-reset",
            "describe",
            "effect",
            "where",
        ]) {
            let host = Host {
                entries: vec![Composition {
                    namespace: namespace.into(),
                    command: "test".into(),
                    steps: vec![CompositionStep::Edit],
                }],
                calls: vec![],
                reports: vec![],
            };
            assert!(registrations(Some(&host)).is_err(), "{namespace}");
        }
    }
    #[test]
    fn registration_rejects_duplicates_reserved_names_and_limits() {
        let entry = Composition {
            namespace: "flow".into(),
            command: "edit".into(),
            steps: vec![CompositionStep::Edit],
        };
        let mut host = Host {
            entries: vec![entry.clone()],
            calls: vec![],
            reports: vec![],
        };
        assert_eq!(registrations(Some(&host)).unwrap().len(), 1);
        host.entries.push(entry.clone());
        assert!(registrations(Some(&host)).is_err());
        host.entries = vec![Composition {
            namespace: "session".into(),
            ..entry.clone()
        }];
        assert!(registrations(Some(&host)).is_err());
        host.entries = vec![Composition {
            steps: vec![CompositionStep::Edit; 33],
            ..entry
        }];
        assert!(registrations(Some(&host)).is_err());
    }
    #[test]
    fn selected_metadata_tracks_revision_not_target() {
        let mut buffers = ScratchBuffers::default();
        let target = buffers.create("text", "target");
        buffers.bind_semantic_target(Some(target)).unwrap();
        let selected = buffers.create("text", "source");
        let metadata = SelectedMetadata::from_buffer(buffers.selected().unwrap());
        assert_eq!(metadata.id, selected);
        assert_eq!(buffers.semantic_target_id(), Some(target));
    }
}
