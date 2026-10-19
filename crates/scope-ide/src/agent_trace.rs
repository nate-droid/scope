//! Explicit host-owned raw protocol capture. Never connected to application logs.
use serde_json::{Value, json};
use std::io::Write;

/// Sensitive JSONL capture to a host-selected writer. One trace per session.
/// I/O failures latch `failed` and disable further writes, never changing the run.
/// Writers are trusted synchronous host code; they must not panic or block forever.
/// No files are opened, uploaded, or exposed to language tools by this type.
pub struct ProtocolTrace<W: Write> {
    writer: W,
    failed: bool,
}

impl<W: Write> ProtocolTrace<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            failed: false,
        }
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    /// Record the exact host-rendered text after applying downstream presentation.
    pub fn record_presentation(&mut self, text: &str) {
        self.record(&json!({"event":"presentation", "content":text}));
    }

    pub(crate) fn record(&mut self, event: &Value) {
        if self.failed {
            return;
        }
        let result = (|| -> std::io::Result<()> {
            serde_json::to_writer(&mut self.writer, event)?;
            self.writer.write_all(b"\n")?;
            self.writer.flush()
        })();
        self.failed = result.is_err();
    }
}
