//! Generic provider contracts; no discovery, host access, or domain semantics.

use crate::MetaGraph;
use std::{error::Error, fmt};

/// Provider identity and human-readable purpose. Identity is producer-assigned,
/// not a trust assertion or an access grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDescription {
    pub id: String,
    pub description: String,
}

impl ProviderDescription {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
        }
    }
}

/// Generic diagnostic, optionally referring to an opaque logical locator.
/// A locator is metadata, never a host path to open or an access grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotWarning {
    pub code: String,
    pub message: String,
    pub locator: Option<String>,
}

impl SnapshotWarning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            locator: None,
        }
    }

    pub fn with_locator(mut self, locator: impl Into<String>) -> Self {
        self.locator = Some(locator.into());
        self
    }
}

/// Completeness is relative to the provider's declared scope, not all knowledge.
/// Structural graph validation does not establish completeness or freshness.
/// An incomplete snapshot may have no detailed warnings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum SnapshotCompleteness {
    #[default]
    Complete,
    Incomplete {
        warnings: Vec<SnapshotWarning>,
    },
}

/// Borrowed, already validated graph with producer-reported completeness.
/// Retrieval need not clone, rebuild, or revalidate the graph. The borrow prevents
/// replacing the provider's graph while the snapshot is in use.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetaSnapshot<'a> {
    pub graph: &'a MetaGraph,
    pub completeness: SnapshotCompleteness,
}

impl<'a> MetaSnapshot<'a> {
    pub fn new(graph: &'a MetaGraph, completeness: SnapshotCompleteness) -> Self {
        Self {
            graph,
            completeness,
        }
    }

    pub fn complete(graph: &'a MetaGraph) -> Self {
        Self::new(graph, SnapshotCompleteness::Complete)
    }

    pub fn incomplete(graph: &'a MetaGraph, warnings: Vec<SnapshotWarning>) -> Self {
        Self::new(graph, SnapshotCompleteness::Incomplete { warnings })
    }
}

/// Small object-safe interface over a provider's existing validated graph.
/// Refresh, caching, capabilities, and authoritative domain validation belong
/// to implementations, not this contract. No thread-safety bound is imposed.
pub trait MetaProvider {
    fn description(&self) -> ProviderDescription;
    fn snapshot(&self) -> Result<MetaSnapshot<'_>, ProviderError>;
}

/// Machine-readable failure category, separate from graph query errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderErrorKind {
    Unavailable,
    InvalidSnapshot,
    Other,
}

/// Provider failure retaining the original error and its source chain.
/// Partial but valid graphs should instead return an incomplete snapshot.
#[derive(Debug)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(
        kind: ProviderErrorKind,
        message: impl Into<String>,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl Error for ProviderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn Error + 'static))
    }
}
