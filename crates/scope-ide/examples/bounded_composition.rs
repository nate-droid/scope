//! Run with `cargo run -p scope-ide --example bounded_composition`.
//! Test with `cargo test -p scope-ide --example bounded_composition`.
//! All edits remain in caller-owned memory; no evaluator or disk writes are used.
use scope_ide::{
    app::App,
    cli::{
        DownstreamError, DownstreamOutcome, ReadOnlyCommand,
        composition::{Composition, CompositionStep, SelectedMetadata},
        run_borrowed_with_sources,
    },
    document::Document,
    language::InteractiveSession,
    scratch::{ScratchBuffer, ScratchBuffers},
    text::TextRange,
    workspace::Workspace,
};
use std::io::{Cursor, Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, PartialEq, Eq)]
struct Inspection {
    metadata: SelectedMetadata,
    text: String,
}

struct Host {
    entries: Vec<Composition>,
    authorize: bool,
    records: Vec<Inspection>,
}

impl Host {
    fn new(authorize: bool) -> Self {
        let bind = || CompositionStep::BindSelected {
            expected_language: "text".into(),
        };
        Self {
            entries: vec![
                Composition {
                    namespace: "notes".into(),
                    command: "create".into(),
                    steps: vec![
                        CompositionStep::Create {
                            language: "text".into(),
                        },
                        bind(),
                        CompositionStep::Edit,
                    ],
                },
                Composition {
                    namespace: "notes".into(),
                    command: "run-selected".into(),
                    // Rebind after editing: a binding pins the current revision.
                    steps: vec![
                        bind(),
                        CompositionStep::Downstream {
                            namespace: "demo".into(),
                            command: "inspect".into(),
                            args: vec![],
                        },
                    ],
                },
            ],
            authorize,
            records: vec![],
        }
    }
}

impl ReadOnlyCommand for Host {
    fn compositions(&self) -> &[Composition] {
        &self.entries
    }

    fn downstream_namespaces(&self) -> &[&str] {
        &["demo"]
    }

    fn invoke_downstream(
        &mut self,
        namespace: &str,
        command: &str,
        args: &[&str],
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> std::result::Result<DownstreamOutcome, DownstreamError> {
        if namespace != "demo" || command != "inspect" || !args.is_empty() {
            return Ok(DownstreamOutcome::Unhandled);
        }
        // Registration and binding are not authorization. Native hosts own this gate.
        if !self.authorize {
            return Err(DownstreamError::new(
                "host_denied",
                "inspection not authorized",
            ));
        }
        let target =
            target.ok_or_else(|| DownstreamError::new("missing_target", "bind a scratch first"))?;
        let record = Inspection {
            metadata: SelectedMetadata::from_buffer(target),
            text: target.text().into(),
        };
        writeln!(output, "inspect {:?}: {:?}", record.metadata, record.text)
            .map_err(|error| DownstreamError::new("output", error.to_string()))?;
        self.records.push(record);
        Ok(DownstreamOutcome::Handled)
    }

    fn invoke(&mut self, _: &Document, _: TextRange, _: &str, _: &mut dyn Write) -> Result<()> {
        Err("this host exposes only demo inspect".into())
    }
}

struct Demo {
    app: App,
    session: InteractiveSession,
    scratches: ScratchBuffers,
    host: Host,
}

impl Demo {
    fn new(authorize: bool) -> Result<Self> {
        Ok(Self {
            app: App::builder(Workspace::read_only(env!("CARGO_MANIFEST_DIR"))?).build()?,
            session: InteractiveSession::new(),
            scratches: ScratchBuffers::default(),
            host: Host::new(authorize),
        })
    }

    fn shell(&mut self, script: &str) -> Result<String> {
        let mut output = Vec::new();
        run_borrowed_with_sources(
            &mut self.app,
            &mut self.session,
            Some(&mut self.host),
            &mut self.scratches,
            Cursor::new(script),
            &mut output,
        )?;
        Ok(String::from_utf8(output)?)
    }
}

fn main() -> Result<()> {
    let mut demo = Demo::new(true)?;
    print!("{}", demo.shell(":scope notes create\nfirst unsaved note\n:done\n:scope notes run-selected\n:scope quit\n")?);
    // Retained public metadata supplies the ID; never assume allocation order.
    let first = demo.scratches.selected()?.id();
    print!("{}", demo.shell(":scope notes create\nsecond unsaved note\n:done\n:scope notes run-selected\n:scope quit\n")?);
    print!(
        "{}",
        demo.shell(&format!(
            ":scope semantic-target {first}\n:scope downstream demo inspect\n:scope quit\n"
        ))?
    );
    println!(
        "Caller retains {} inspections and both unsaved scratches.",
        demo.host.records.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compositions_switch_actual_ids_and_manual_target_stays_independent() -> Result<()> {
        let mut demo = Demo::new(true)?;
        demo.shell(":scope notes create\nfirst\n:done\n:scope notes run-selected\n:scope quit\n")?;
        let first = demo.scratches.selected()?.id();
        assert_eq!(demo.scratches.get(first)?.text(), "first\n");
        demo.shell(":scope notes create\nsecond\n:done\n:scope notes run-selected\n:scope quit\n")?;
        let second = demo.scratches.selected()?.id();
        assert_ne!(first, second);
        assert_eq!(demo.scratches.get(first)?.text(), "first\n");
        assert_eq!(demo.scratches.get(second)?.text(), "second\n");
        demo.shell(&format!(":scope buffer {first}\n:scope notes run-selected\n:scope buffer {second}\n:scope semantic-target {first}\n:scope downstream demo inspect\n:scope quit\n"))?;
        assert_eq!(demo.scratches.selected()?.id(), second);
        let expected = [first, second, first, first];
        assert_eq!(demo.host.records.len(), expected.len());
        for (record, id) in demo.host.records.iter().zip(expected) {
            let buffer = demo.scratches.get(id)?;
            assert_eq!(record.metadata, SelectedMetadata::from_buffer(buffer));
            assert_eq!(record.text, buffer.text());
        }
        Ok(())
    }

    #[test]
    fn host_denial_does_not_inspect_or_modify_source() -> Result<()> {
        let mut demo = Demo::new(false)?;
        demo.shell(":scope notes create\nprivate\n:done\n:scope quit\n")?;
        let id = demo.scratches.selected()?.id();
        let before = SelectedMetadata::from_buffer(demo.scratches.get(id)?);
        let output =
            demo.shell(":scope notes run-selected\n:scope downstream demo inspect\n:scope quit\n")?;
        assert!(output.contains("inspection not authorized"), "{output}");
        assert!(demo.host.records.is_empty());
        assert_eq!(
            SelectedMetadata::from_buffer(demo.scratches.get(id)?),
            before
        );
        assert_eq!(demo.scratches.get(id)?.text(), "private\n");
        Ok(())
    }
}
