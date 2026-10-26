//! Continuous source collection, not a persistent interpreter.
use super::{ReadOnlyCommand, ScriptSnapshotAdmission};
use crate::{
    scratch::ScratchBuffers,
    text::{Span, TextBuffer, TextEdit},
};
use std::io::Write;

#[cfg(test)]
#[path = "script_context_tests.rs"]
mod tests;

#[cfg(test)]
mod ux3_tests {
    use super::*;

    #[test]
    fn accumulation_preserves_comments_strings_and_hard_boundaries() {
        for (source, expected) in [
            ("", ""),
            (" \n", " \n"),
            ("1", "1;\n"),
            ("1;", "1;\n"),
            ("let n=8;  \n\t", "let n=8;  \n\t\n"),
            ("1; # retained", "1; # retained\n"),
            ("1 # not a terminator;", "1 # not a terminator;\n;\n"),
            ("\";\"", "\";\";\n"),
            ("1 # retained", "1 # retained\n;\n"),
            ("# retained\n1", "# retained\n1;\n"),
            ("\"# not a comment\"", "\"# not a comment\";\n"),
            (
                "\"escaped \\\" # still string\"",
                "\"escaped \\\" # still string\";\n",
            ),
        ] {
            assert_eq!(ScriptContext::append_prefix(source), expected);
        }
    }
}

#[derive(Default)]
pub(super) struct ScriptContext {
    active: bool,
    pending: Option<(scope_language::SourceId, u64, String)>,
}

impl ScriptContext {
    pub(super) fn compact(
        &self,
        proposals: &mut super::source_proposals::Proposals,
        scratches: &mut ScratchBuffers,
        host: Option<&dyn ReadOnlyCommand>,
        output: &mut impl Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.pending.is_some() {
            return Err(
                "incomplete Script input pending; finish it or :cancel before :compact".into(),
            );
        }
        let source = scratches.selected()?;
        if source.language_id() != "scope-script" {
            return Err("selected scratch is not scope-script".into());
        }
        let snapshot = scope_language::SourceSnapshot {
            source_id: source.evaluation_source_id().clone(),
            revision_id: scope_language::RevisionId::from_version(source.version()),
            text: source.text().into(),
        };
        let reduction = host
            .ok_or("no Script reduction provider installed")?
            .reduce_script_snapshot(snapshot.clone())?;
        if reduction.original != snapshot {
            return Err("reduction baseline does not match captured evaluation snapshot".into());
        }
        proposals.compact(source, reduction, output)
    }

    fn append_prefix(source: &str) -> String {
        if source.trim().is_empty() {
            return source.to_owned();
        }
        // Keep a hard statement boundary: newlines alone can continue calls,
        // indexing, or an if/else. A trailing comment must not swallow it.
        let mut quoted = false;
        let mut escaped = false;
        let mut comment = false;
        let mut terminated = false;
        for ch in source.chars() {
            if ch == '\n' {
                comment = false;
            } else if comment {
                continue;
            } else if quoted {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quoted = false;
                }
            } else if ch == '"' {
                quoted = true;
                terminated = false;
            } else if ch == '#' {
                comment = true;
            } else if !matches!(ch, ' ' | '\t' | '\r') {
                terminated = ch == ';';
            }
        }
        if terminated {
            format!("{source}\n")
        } else if comment {
            format!("{source}\n;\n")
        } else {
            format!("{source};\n")
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.active
    }

    pub(super) fn prompt(&self) -> &'static str {
        if self.pending.is_some() {
            "...> "
        } else {
            "script> "
        }
    }

    pub(super) fn complete(
        &self,
        line: &str,
        cursor: usize,
        scratches: &ScratchBuffers,
        host: Option<&dyn ReadOnlyCommand>,
    ) -> Vec<crate::language::CompletionItem> {
        if !line.is_char_boundary(cursor) {
            return Vec::new();
        }
        if line.starts_with(':') {
            if line.starts_with(":scope") {
                return super::complete_commands(line, cursor, true).unwrap_or_default();
            }
            let expanded = format!(":scope {}", &line[1..]);
            return super::complete_commands(&expanded, cursor + 6, true)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|mut item| {
                    if item.replacement.start < 7 {
                        return None;
                    }
                    item.replacement = item.replacement.start - 6..item.replacement.end - 6;
                    Some(item)
                })
                .collect();
        }
        let Ok(source) = scratches.selected() else {
            return Vec::new();
        };
        if source.language_id() != "scope-script" {
            return Vec::new();
        }
        let prefix = if let Some((id, version, draft)) = &self.pending {
            if id != source.evaluation_source_id() || *version != source.version() {
                return Vec::new();
            }
            format!("{draft}\n")
        } else {
            Self::append_prefix(source.text())
        };
        let offset = prefix.len();
        let draft = format!("{prefix}{line}");
        host.map_or_else(Vec::new, |host| {
            host.complete_source(source, &draft, offset + cursor)
        })
        .into_iter()
        .filter_map(|mut item| {
            if item.replacement.start < offset || item.replacement.end > draft.len() {
                return None;
            }
            item.replacement = item.replacement.start - offset..item.replacement.end - offset;
            Some(item)
        })
        .take(40)
        .collect()
    }

    /// None consumes the submission; Some always returns through the shell dispatcher.
    pub(super) fn route(
        &mut self,
        line: &str,
        scratches: &mut ScratchBuffers,
        host: Option<&dyn ReadOnlyCommand>,
        output: &mut impl Write,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let trimmed = line.trim();
        if trimmed == ":scope script leave" || (self.active && trimmed == ":script leave") {
            self.active = false;
            self.pending = None;
            writeln!(output, "Left Script context (scratch retained).")?;
            return Ok(None);
        }
        if trimmed == ":scope script enter" || (self.active && trimmed == ":script enter") {
            if !host.is_some_and(|host| {
                host.admit_script_snapshot("") == ScriptSnapshotAdmission::CompletePure
            }) {
                return Err("Script context requires a supported Script host".into());
            }
            match scratches.selected() {
                Ok(source) if source.language_id() != "scope-script" => {
                    return Err(
                        "selected scratch is not scope-script; select a Script scratch first"
                            .into(),
                    );
                }
                Ok(_) => {}
                Err(_) => {
                    let existing = scratches
                        .list()
                        .find(|source| source.language_id() == "scope-script")
                        .map(|source| source.id());
                    if let Some(id) = existing {
                        scratches.select(id)?;
                    } else {
                        scratches.create("scope-script", "");
                    }
                }
            }
            self.active = true;
            writeln!(
                output,
                "Script context: source appends to selected scratch; :run evaluates the full snapshot; :cancel clears pending; :scope script leave exits. Effects require :run. Use :edit for the existing editor."
            )?;
            return Ok(None);
        }
        if !self.active {
            return Ok(Some(line.to_owned()));
        }
        if trimmed == ":cancel" {
            self.pending = None;
            writeln!(output, "Pending Script input canceled.")?;
            return Ok(None);
        }
        if trimmed.starts_with(':') {
            let command = if trimmed == ":run" {
                ":scope script run".to_owned()
            } else if trimmed == ":scope" || trimmed.starts_with(":scope ") {
                trimmed.to_owned()
            } else {
                format!(":scope {}", &trimmed[1..])
            };
            if command == ":scope script run" && self.pending.is_some() {
                return Err(
                    "incomplete Script input pending; finish it or :cancel before :run".into(),
                );
            }
            if command == ":scope show" {
                if let Some((id, version, draft)) = &self.pending {
                    let source = scratches.selected()?;
                    if source.language_id() != "scope-script"
                        || id != source.evaluation_source_id()
                        || *version != source.version()
                    {
                        return Err(
                            "pending Script source changed; :cancel before continuing".into()
                        );
                    }
                    writeln!(output, "Committed Script source:")?;
                    writeln!(output, "{}", source.text())?;
                    writeln!(
                        output,
                        "Pending Script snapshot (not committed; not evaluated):"
                    )?;
                    writeln!(output, "{draft}")?;
                    return Ok(None);
                }
            }
            return Ok(Some(command));
        }
        let source = scratches.selected()?;
        if source.language_id() != "scope-script" {
            return Err("selected scratch is not scope-script".into());
        }
        if let Some((id, version, _)) = &self.pending {
            if id != source.evaluation_source_id() || *version != source.version() {
                return Err("pending Script source changed; :cancel before continuing".into());
            }
        }
        let mut snapshot = if let Some((_, _, draft)) = &self.pending {
            format!("{draft}\n")
        } else {
            Self::append_prefix(source.text())
        };
        snapshot.push_str(line);
        let admission = host
            .ok_or("no Script host installed")?
            .admit_script_snapshot(&snapshot);
        match admission {
            ScriptSnapshotAdmission::Incomplete => {
                self.pending = Some((
                    source.evaluation_source_id().clone(),
                    source.version(),
                    snapshot,
                ));
                writeln!(output, "Script input incomplete; continue or :cancel.")?;
                Ok(None)
            }
            ScriptSnapshotAdmission::Invalid(message) => {
                self.pending = None;
                Err(message.into())
            }
            admission => {
                let range = TextBuffer::new(source.text())
                    .range_for_span(Span::new(0, source.text().len()))?;
                scratches
                    .selected_mut()?
                    .apply_edit(TextEdit::replace(range, snapshot))?;
                self.pending = None;
                if admission == ScriptSnapshotAdmission::CompletePure {
                    Ok(Some(":scope script run".to_owned()))
                } else {
                    writeln!(
                        output,
                        "Source updated. Run required (:run, full snapshot)."
                    )?;
                    Ok(None)
                }
            }
        }
    }
}
