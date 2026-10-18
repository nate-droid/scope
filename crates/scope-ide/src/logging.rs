//! Opt-in native logging gate. Only the trusted host should own the controller.
//!
//! Enable with `scope-ide/logging`. Inspection never grants authority. Cell sinks
//! execute in trusted native code, not a sandbox, and may block or have irreversible
//! effects. This gate is not a security boundary against arbitrary native Rust.
//! Selection under the mutex is admission: revoke/replace publish immediately,
//! but do not cancel or wait for admitted calls. Receipts describe selection, not
//! durable delivery. Context IDs, revisions and labels are advisory, not credentials.

use cell_capability_log::{LOG_INTERFACE, Logger};
use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);

/// Non-secret, process-local identity. Independently constructed bindings differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextId(u64);

/// Safe read-only snapshot; inspecting it neither invokes nor authorizes a sink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggingMetadata {
    pub context: ContextId,
    pub revision: u64,
    pub implementation: Option<String>,
}

/// Immutable admission information; not evidence of durable delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggingReceipt {
    context: ContextId,
    revision: u64,
    implementation: String,
}

impl LoggingReceipt {
    pub fn interface(&self) -> &'static str {
        LOG_INTERFACE
    }
    pub fn operation(&self) -> &'static str {
        "log_info"
    }
    pub fn context(&self) -> ContextId {
        self.context
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn implementation(&self) -> &str {
        &self.implementation
    }
}

/// Safe failure categories. Sink errors are deliberately not retained or exposed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoggingError {
    Denied { context: ContextId, revision: u64 },
    Backend { admission: LoggingReceipt },
    Synchronization,
    InvalidLabel,
    RevisionExhausted,
    ContextExhausted,
}

impl fmt::Display for LoggingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Denied { .. } => "logging is not authorized",
            Self::Backend { .. } => "authorized logging backend failed",
            Self::Synchronization => "logging state synchronization failed",
            Self::InvalidLabel => "implementation label must be 1–128 printable ASCII characters with non-whitespace content",
            Self::RevisionExhausted => "logging revision exhausted",
            Self::ContextExhausted => "logging context identifiers exhausted",
        })
    }
}
impl std::error::Error for LoggingError {}

struct Grant {
    logger: Logger,
    label: String,
}
struct State {
    revision: u64,
    grant: Option<Grant>,
}
struct Shared {
    context: ContextId,
    state: Mutex<State>,
}

/// Cloneable invocation-only authority; never exposes a logger or controller.
#[derive(Clone)]
pub struct LoggingBinding {
    shared: Arc<Shared>,
}
/// Host-only authorization authority. Keep this out of consumer-facing APIs.
pub struct LoggingController {
    shared: Arc<Shared>,
}

impl fmt::Debug for LoggingBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoggingBinding")
            .field("context", &self.shared.context)
            .finish_non_exhaustive()
    }
}
impl fmt::Debug for LoggingController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoggingController")
            .field("context", &self.shared.context)
            .finish_non_exhaustive()
    }
}

impl LoggingBinding {
    /// Construct a denied binding and its separate host controller.
    pub fn new() -> Result<(Self, LoggingController), LoggingError> {
        let id = NEXT_CONTEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| LoggingError::ContextExhausted)?;
        let shared = Arc::new(Shared {
            context: ContextId(id),
            state: Mutex::new(State {
                revision: 0,
                grant: None,
            }),
        });
        Ok((
            Self {
                shared: shared.clone(),
            },
            LoggingController { shared },
        ))
    }

    pub fn inspect(&self) -> Result<LoggingMetadata, LoggingError> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| LoggingError::Synchronization)?;
        Ok(LoggingMetadata {
            context: self.shared.context,
            revision: state.revision,
            implementation: state.grant.as_ref().map(|g| g.label.clone()),
        })
    }

    /// Forward exactly `message` to the logger selected at admission.
    pub fn log_info(&self, message: &str) -> Result<LoggingReceipt, LoggingError> {
        let (logger, admission) = {
            let state = self
                .shared
                .state
                .lock()
                .map_err(|_| LoggingError::Synchronization)?;
            let grant = state.grant.as_ref().ok_or(LoggingError::Denied {
                context: self.shared.context,
                revision: state.revision,
            })?;
            (
                grant.logger.clone(),
                LoggingReceipt {
                    context: self.shared.context,
                    revision: state.revision,
                    implementation: grant.label.clone(),
                },
            )
        };
        logger.log(message).map_err(|_| LoggingError::Backend {
            admission: admission.clone(),
        })?;
        Ok(admission)
    }
}

impl LoggingController {
    /// Authorize or replace a logger. Labels are bounded printable ASCII, not secrets.
    /// Invalid labels or exhausted revisions leave the previous grant unchanged.
    pub fn authorize(&self, logger: Logger, label: impl Into<String>) -> Result<(), LoggingError> {
        let label = label.into();
        if label.is_empty()
            || label.len() > 128
            || label.trim().is_empty()
            || !label.bytes().all(|b| (0x20..=0x7e).contains(&b))
        {
            return Err(LoggingError::InvalidLabel);
        }
        self.publish(Some(Grant { logger, label }))
    }

    /// Publish denial, even if already denied, advancing the revision.
    pub fn revoke(&self) -> Result<(), LoggingError> {
        self.publish(None)
    }

    fn publish(&self, grant: Option<Grant>) -> Result<(), LoggingError> {
        let old = {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| LoggingError::Synchronization)?;
            let revision = state
                .revision
                .checked_add(1)
                .ok_or(LoggingError::RevisionExhausted)?;
            let old = std::mem::replace(&mut state.grant, grant);
            state.revision = revision;
            old
        };
        // A sink's captured values may have host-defined destructors.
        drop(old);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
