use super::*;
use crate::source_context::{HostSourceContext, SourceContext};

pub(super) fn print_app_target(app: &App, output: &mut impl Write) -> std::io::Result<()> {
    if let Some(document) = app.open_document() {
        writeln!(
            output,
            "App document target: {:?} @{} path={}",
            crate::source_edit::SourceId::Document(document.id()),
            document.version(),
            inspection_text(document.path().as_str())
        )
    } else {
        writeln!(
            output,
            "App document target: none (agent still has workspace context)"
        )
    }
}

pub(super) fn print_buffer_target(
    buffer: &crate::scratch::ScratchBuffer,
    output: &mut impl Write,
) -> std::io::Result<()> {
    writeln!(
        output,
        "Shell buffer target: {:?} @{} name=scratch-{} destination={}",
        crate::source_edit::SourceId::Scratch(buffer.id()),
        buffer.version(),
        buffer.id(),
        buffer
            .destination_path()
            .map_or_else(|| "[unbound]".into(), |path| inspection_text(path.as_str()))
    )
}

fn describe(
    label: &str,
    source: Option<SourceContext<'_>>,
    output: &mut impl Write,
) -> std::io::Result<()> {
    let Some(source) = source else {
        return writeln!(output, "{label}: none");
    };
    writeln!(
        output,
        "{label}: {:?} @{} / {} / {}",
        source.source_id().unwrap(),
        source.version().unwrap(),
        source.path().map_or_else(
            || "[unbound scratch]".into(),
            |path| inspection_text(path.as_str())
        ),
        if source.is_dirty() == Some(true) {
            "dirty"
        } else {
            "clean"
        }
    )?;
    if let Some(buffer) = source.buffer() {
        writeln!(output, "  name: scratch-{}", buffer.id())?;
    }
    writeln!(
        output,
        "  semantic service: {}",
        source.semantic_service().map_or_else(
            || "none".into(),
            |service| inspection_text(service.language_id())
        )
    )
}

pub(super) fn print_source_context(
    app: &App,
    buffers: &ScratchBuffers,
    session: &InteractiveSession,
    output: &mut impl Write,
) -> std::io::Result<()> {
    let context = HostSourceContext::derive(app, buffers, session);
    describe("App document", context.app, output)?;
    describe("Selected shell buffer", context.buffer, output)?;
    writeln!(
        output,
        "Interactive session: {}",
        session
            .language_id()
            .map_or_else(|| "none".into(), inspection_text)
    )?;
    writeln!(
        output,
        "App and buffer targets differ: {} (ownership, not path equality)",
        context.targets_differ()
    )?;
    if cfg!(feature = "tools") {
        describe(
            "Inspection target (eligible operations/confirmation required)",
            context.app,
            output,
        )?;
    } else {
        writeln!(
            output,
            "Inspection target: unavailable (requires feature tools)"
        )?;
    }
    if cfg!(feature = "agent") {
        describe(
            "Agent document context (workspace tools remain separate)",
            context.app,
            output,
        )?;
        describe(
            "Agent edit source (host configuration/authorization required)",
            context.buffer,
            output,
        )?;
    } else {
        writeln!(output, "Agent target: unavailable (requires feature agent)")?;
    }
    describe("Source proposal creation target", context.buffer, output)?;
    writeln!(
        output,
        "Source proposal review/accept: retained proposal baseline, not current selection"
    )?;
    writeln!(
        output,
        "Workspace proposal commands: isolated approved-path snapshot, not either in-memory source"
    )?;
    match (buffers.selected().ok(), session.language_id()) {
        (Some(_), Some(_)) if session.reset_required() => writeln!(
            output,
            "Buffer evaluation target: unavailable (session reset required)"
        )?,
        (Some(buffer), Some(language)) if buffer.language_id() == language => {
            describe("Buffer evaluation target", context.buffer, output)?
        }
        (Some(_), Some(_)) => writeln!(
            output,
            "Buffer evaluation target: unavailable (buffer/session language mismatch)"
        )?,
        (_, None) => writeln!(
            output,
            "Buffer evaluation target: unavailable (no interactive session)"
        )?,
        (None, _) => writeln!(
            output,
            "Buffer evaluation target: none (no selected buffer)"
        )?,
    }
    writeln!(
        output,
        "Routing: bare open/show/save target App only without a session; :scope open/show/save target shell buffers. With a session, unprefixed input is language source. Use :scope session none before bare App open."
    )?;
    writeln!(
        output,
        "Context is observational: no execution, renewed authorization, or freshness reset."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_and_actual_shell_context_transitions() {
        assert_eq!(
            ShellCommand::parse("context").unwrap(),
            ShellCommand::Context
        );
        assert!(ShellCommand::parse("context extra").is_err());
        let root = crate::test_support::TempWorkspace::new("shell-context");
        root.write_file("main.scope", "disk");
        let app = App::new(root.workspace()).unwrap();
        let mut output = Vec::new();
        run_shell(app, tests_config(), std::io::Cursor::new("context\n:scope open main.scope\n:scope context\nopen main.scope\nreplace 1 1 1 1 app\n:scope scratch replace 1 1 1 1 buffer\ncontext\nopen main.scope\ncontext\n:scope discard\ncontext\nquit\n"), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("App document: none"));
        assert!(output.contains("Selected shell buffer: none"));
        assert!(output.contains("App and buffer targets differ: true"));
        assert!(output.contains("Document(DocumentId(1)) @1"));
        assert!(output.contains("Scratch(0) @1"));
        assert!(output.contains("Document(DocumentId(2)) @0"));
        assert_eq!(root.read_file("main.scope"), "disk");
    }
    #[cfg(not(feature = "agent"))]
    fn tests_config() -> AgentConfig {
        AgentConfig
    }
    #[cfg(feature = "agent")]
    fn tests_config() -> AgentConfig {
        AgentConfig::local("unused", "http://localhost:1234/v1")
    }
}
