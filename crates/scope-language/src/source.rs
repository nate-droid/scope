//! In-memory source identities and exact-revision resolution. No filesystem authority.
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

/// Process-local identity. Clone to retain identity; `fresh` always creates a new
/// source, including across shells. Not serializable or a filesystem capability.
#[derive(Debug, Clone)]
pub struct SourceId(Arc<u8>);
impl SourceId {
    pub fn fresh() -> Self {
        Self(Arc::new(0))
    }
}
impl PartialEq for SourceId {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for SourceId {}
impl Hash for SourceId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

/// Revision within one source, not a disk conflict token. Never reuse a version
/// for different text under the same SourceId.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RevisionId(u64);
impl RevisionId {
    pub fn from_version(version: u64) -> Self {
        Self(version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationRequest<'a> {
    pub text: &'a str,
    pub source_id: SourceId,
    pub revision_id: RevisionId,
}

/// Coordinates use the same line/column convention as Diagnostic::range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub source_id: SourceId,
    pub revision_id: RevisionId,
    pub range: crate::TextRange,
}

/// Read-only historical text, never an editable buffer or an instruction to open a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    pub source_id: SourceId,
    pub revision_id: RevisionId,
    pub text: Arc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceUnavailable {
    UnknownOrExpired,
}

/// Host-owned retention policy. Implementations must return only the exact pair,
/// never current text or disk as a fallback. Resolution grants no new authority.
pub trait SourceResolver {
    fn resolve(
        &self,
        source: &SourceId,
        revision: RevisionId,
    ) -> Result<SourceSnapshot, SourceUnavailable>;
}
