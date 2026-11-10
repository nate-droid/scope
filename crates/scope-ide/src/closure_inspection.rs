//! Opt-in closure inspection contract. No filesystem, native execution, or Sprout adapter.
//! Hosts must recapture from authoritative state and serialize mutation with dispatch.
//! SourceId values belong to one retained owner; never recycle IDs in that owner.
use crate::{
    source_edit::{SourceId, SourceSnapshot},
    tool::semantic::SemanticAuthorization,
};
use scope_language::{
    SemanticOperationDescription, SemanticOperationEffect, SemanticTargetRequirement,
};
use serde_json::Value;
use std::{collections::BTreeSet, io::Write, sync::Arc};

/// Stops serialization at the byte budget, with optional bounded output retention.
struct BoundedJsonWriter {
    limit: usize,
    count: usize,
    output: Option<Vec<u8>>,
    exceeded: bool,
}
impl BoundedJsonWriter {
    fn new(limit: usize, retain: bool) -> Self {
        Self {
            limit,
            count: 0,
            output: retain.then(Vec::new),
            exceeded: false,
        }
    }
    fn result(&self, result: Result<(), serde_json::Error>) -> Result<(), ClosureInspectionError> {
        result.map_err(|_| {
            if self.exceeded {
                ClosureInspectionError::LimitExceeded
            } else {
                ClosureInspectionError::InvalidCapture
            }
        })
    }
    fn serialize(&mut self, value: &Value) -> Result<(), ClosureInspectionError> {
        let result = serde_json::to_writer(&mut *self, value);
        self.result(result)
    }
    fn operation(
        &mut self,
        operation: &SemanticOperationDescription,
    ) -> Result<(), ClosureInspectionError> {
        let result = serde_json::to_writer(&mut *self, operation);
        self.result(result)
    }
    fn string(&mut self, text: &str) -> Result<(), ClosureInspectionError> {
        let result = serde_json::to_writer(&mut *self, text);
        self.result(result)
    }
    fn raw(&mut self, text: &str) -> Result<(), ClosureInspectionError> {
        self.write_all(text.as_bytes())
            .map_err(|_| ClosureInspectionError::LimitExceeded)
    }
}
impl Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit - self.count {
            self.exceeded = true;
            return Err(std::io::Error::other("closure JSON budget exceeded"));
        }
        if let Some(output) = &mut self.output {
            let needed = output.len() + bytes.len();
            if needed > output.capacity() {
                let capacity = needed
                    .max(output.capacity().saturating_mul(2))
                    .min(self.limit);
                output.reserve_exact(capacity - output.len());
            }
            output.extend_from_slice(bytes);
        }
        self.count += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Iterative admission runs before recursive serialization, cloning or comparison.
/// Node and string budgets are shared across all opaque fields, including constraints.
fn admit_metadata(capture: &ClosureCapture) -> Result<(), ClosureInspectionError> {
    let mut pending = vec![
        (&capture.compilation_options, 1usize),
        (&capture.compilation_limits, 1),
        (&capture.execution_policy, 1),
    ];
    if let Some(value) = &capture.operation.constraints {
        pending.push((value, 1));
    }
    admit_json(pending)
}

fn admit_json(mut pending: Vec<(&Value, usize)>) -> Result<(), ClosureInspectionError> {
    let mut scheduled = pending.len();
    let mut string_bytes = 0usize;
    while let Some((value, depth)) = pending.pop() {
        if depth > ClosureInspectionBinding::MAX_JSON_DEPTH {
            return Err(ClosureInspectionError::LimitExceeded);
        }
        let children = match value {
            Value::Array(a) => a.len(),
            Value::Object(o) => o.len(),
            _ => 0,
        };
        scheduled = scheduled
            .checked_add(children)
            .ok_or(ClosureInspectionError::LimitExceeded)?;
        if scheduled > ClosureInspectionBinding::MAX_JSON_NODES {
            return Err(ClosureInspectionError::LimitExceeded);
        }
        let mut add_string = |text: &str| -> Result<(), ClosureInspectionError> {
            string_bytes = string_bytes
                .checked_add(text.len())
                .ok_or(ClosureInspectionError::LimitExceeded)?;
            if string_bytes > ClosureInspectionBinding::MAX_JSON_STRING_BYTES {
                return Err(ClosureInspectionError::LimitExceeded);
            }
            Ok(())
        };
        match value {
            Value::String(text) => add_string(text)?,
            Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => {
                for (key, value) in values {
                    add_string(key)?;
                    pending.push((value, depth + 1));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// Rejected deeply nested owned Values must not recurse through serde_json's Drop.
fn discard_metadata(capture: &mut ClosureCapture) {
    let mut pending = vec![
        std::mem::take(&mut capture.compilation_options),
        std::mem::take(&mut capture.compilation_limits),
        std::mem::take(&mut capture.execution_policy),
    ];
    pending.extend(capture.operation.constraints.take());
    for value in pending {
        drop(IterativeJson(value));
    }
}

/// Own untrusted JSON across early returns without recursive serde_json disposal.
pub(crate) struct IterativeJson(pub(crate) Value);
impl Drop for IterativeJson {
    fn drop(&mut self) {
        let mut pending = vec![std::mem::take(&mut self.0)];
        while let Some(value) = pending.pop() {
            match value {
                Value::Array(values) => pending.extend(values),
                Value::Object(values) => pending.extend(values.into_iter().map(|(_, value)| value)),
                _ => {}
            }
        }
    }
}

/// Process-local host identity. Clone for the same owner, never for another App.
#[derive(Debug, Clone)]
pub struct ClosureOwner(Arc<()>);
impl Default for ClosureOwner {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}
impl PartialEq for ClosureOwner {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for ClosureOwner {}

#[derive(Clone, PartialEq, Eq)]
pub struct ClosureSource {
    pub id: SourceId,
    pub text: String,
    pub version: u64,
    pub language: String,
    pub module: String,
    /// Exact host origin metadata, not a path to open.
    pub origin: String,
}

/// All fields participate in exact equality; opaque JSON is retained losslessly.
#[derive(Clone, PartialEq)]
pub struct ClosureCapture {
    pub owner: ClosureOwner,
    pub sources: Vec<ClosureSource>,
    pub root: SourceId,
    pub graph: Vec<(SourceId, SourceId)>,
    pub compilation_identity: String,
    pub compilation_options: Value,
    pub compilation_limits: Value,
    pub operation: SemanticOperationDescription,
    pub execution_policy: Value,
    pub artifact_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosureInspectionError {
    InvalidCapture,
    LimitExceeded,
    SnapshotMismatch,
    Denied,
    Host(String),
}

/// Immutable bounded snapshot, independent of the existing InspectionBinding.
#[derive(Clone)]
pub struct ClosureInspectionBinding(ClosureCapture, Arc<()>, Value);
// Fresh recaptures compare content, not seals. Location attachment uses the seal.
impl PartialEq for ClosureInspectionBinding {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.2 == other.2
    }
}
impl ClosureInspectionBinding {
    pub const MAX_SOURCES: usize = 128;
    pub const MAX_EDGES: usize = 1024;
    pub const MAX_BYTES: usize = 1024 * 1024;

    pub const MAX_JSON_DEPTH: usize = 32;
    pub const MAX_JSON_NODES: usize = 8192;
    pub const MAX_JSON_STRING_BYTES: usize = Self::MAX_BYTES;

    /// Incoming owned allocations (and any raw ClosureCapture cloning) are the
    /// caller's responsibility. Admission bounds recursion in sealed bindings.
    pub fn capture(capture: ClosureCapture) -> Result<Self, ClosureInspectionError> {
        Self::capture_with_arguments(capture, serde_json::json!({}))
    }

    /// Exact arguments, validated and bounded before retention.
    pub fn capture_with_arguments(
        mut capture: ClosureCapture,
        arguments: Value,
    ) -> Result<Self, ClosureInspectionError> {
        let mut arguments = IterativeJson(arguments);
        use ClosureInspectionError::*;
        if let Err(error) = admit_metadata(&capture) {
            discard_metadata(&mut capture);
            return Err(error);
        }
        Self::validate_arguments(&capture.operation, &arguments.0)?;
        if capture.sources.is_empty()
            || capture.sources.len() > Self::MAX_SOURCES
            || capture.graph.len() > Self::MAX_EDGES
        {
            return Err(LimitExceeded);
        }
        let mut ids = BTreeSet::new();
        let mut modules = BTreeSet::new();
        let mut bytes = 0usize;
        let mut add = |n: usize| -> Result<(), ClosureInspectionError> {
            bytes = bytes.checked_add(n).ok_or(LimitExceeded)?;
            if bytes > Self::MAX_BYTES {
                return Err(LimitExceeded);
            }
            Ok(())
        };
        for source in &capture.sources {
            if !ids.insert(source.id)
                || source.module.is_empty()
                || source.language.is_empty()
                || !modules.insert(&source.module)
            {
                return Err(InvalidCapture);
            }
            for text in [
                &source.text,
                &source.language,
                &source.module,
                &source.origin,
            ] {
                add(text.len())?;
            }
        }
        let root = capture
            .sources
            .iter()
            .find(|s| s.id == capture.root)
            .ok_or(InvalidCapture)?;
        if capture.sources.iter().any(|s| s.language != root.language) {
            return Err(InvalidCapture);
        }
        let edges: BTreeSet<_> = capture.graph.iter().copied().collect();
        if !ids.contains(&capture.root)
            || edges.len() != capture.graph.len()
            || edges
                .iter()
                .any(|(a, b)| !ids.contains(a) || !ids.contains(b))
            || capture.compilation_identity.is_empty()
            || capture.artifact_identity.is_empty()
            || capture.operation.id.is_empty()
            || !matches!(
                capture.operation.effect,
                SemanticOperationEffect::Read | SemanticOperationEffect::ProposeDomain
            )
            || capture.operation.target != SemanticTargetRequirement::Document
        {
            return Err(InvalidCapture);
        }
        // Require every captured source to belong to the root's dependency closure.
        let mut reachable = BTreeSet::from([capture.root]);
        loop {
            let before = reachable.len();
            for (a, b) in &edges {
                if reachable.contains(a) {
                    reachable.insert(*b);
                }
            }
            if reachable.len() == before {
                break;
            }
        }
        if reachable != ids {
            return Err(InvalidCapture);
        }
        // Kahn's algorithm rejects cycles, including self-dependencies.
        let mut remaining = ids.clone();
        while !remaining.is_empty() {
            let ready: Vec<_> = remaining
                .iter()
                .copied()
                .filter(|id| !edges.iter().any(|(a, b)| b == id && remaining.contains(a)))
                .collect();
            if ready.is_empty() {
                return Err(InvalidCapture);
            }
            for id in ready {
                remaining.remove(&id);
            }
        }
        add(capture.compilation_identity.len())?;
        add(capture.artifact_identity.len())?;
        let mut writer = BoundedJsonWriter::new(Self::MAX_BYTES - bytes, false);
        for value in [
            &capture.compilation_options,
            &capture.compilation_limits,
            &capture.execution_policy,
        ] {
            writer.serialize(value)?;
        }
        writer.operation(&capture.operation)?;
        writer.serialize(&arguments.0)?;
        Ok(Self(
            capture,
            Arc::new(()),
            std::mem::take(&mut arguments.0),
        ))
    }
    pub const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
    pub const MAX_ARGUMENT_FIELDS: usize = 64;

    pub(crate) fn validate_arguments(
        operation: &SemanticOperationDescription,
        arguments: &Value,
    ) -> Result<(), ClosureInspectionError> {
        // Both inputs must be admitted before schema comparison or serialization.
        let mut pending = vec![(arguments, 1)];
        if let Some(constraints) = &operation.constraints {
            pending.push((constraints, 1));
        }
        admit_json(pending)?;
        BoundedJsonWriter::new(Self::MAX_ARGUMENT_BYTES, false).serialize(arguments)?;
        if let Some(constraints) = &operation.constraints {
            BoundedJsonWriter::new(Self::MAX_BYTES, false).serialize(constraints)?;
        }
        use scope_language::SemanticArgumentKind::{Array, Object};
        let object = arguments
            .as_object()
            .ok_or(ClosureInspectionError::InvalidCapture)?;
        if operation.arguments.len() > Self::MAX_ARGUMENT_FIELDS
            || object.len() > Self::MAX_ARGUMENT_FIELDS
        {
            return Err(ClosureInspectionError::LimitExceeded);
        }
        let structured = operation
            .constraints
            .as_ref()
            .and_then(|c| c.get("scope_argument_schema"))
            .is_some();
        let mut names = BTreeSet::new();
        if operation.arguments.iter().any(|field| {
            field.name.is_empty()
                || !names.insert(&field.name)
                || (!structured && matches!(field.kind, Array | Object))
        }) || object
            .values()
            .any(|value| !structured && (value.is_array() || value.is_object()))
            || operation.validate_arguments(arguments).is_err()
        {
            return Err(ClosureInspectionError::InvalidCapture);
        }
        Ok(())
    }

    pub fn arguments(&self) -> &Value {
        &self.2
    }

    pub fn snapshot(&self) -> &ClosureCapture {
        &self.0
    }
    /// Selection is not a grant. Every ticket requires its own policy callback.
    pub fn invocation(&self) -> ClosureInvocation {
        ClosureInvocation(self.clone())
    }
}

// Debug must never turn an accidental confirmation/log into a source dump.
impl std::fmt::Debug for ClosureSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClosureSource")
            .field("id", &self.id)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for ClosureCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClosureCapture")
            .field("root", &self.root)
            .field("source_count", &self.sources.len())
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for ClosureInspectionBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Bounded JSON for confirmation. No source text is projected. Opaque metadata
/// is not a secret scrubber: the trusted host must not put source in metadata.
#[derive(Debug, Clone)]
pub struct ClosureReview(String);
impl ClosureReview {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl ClosureInspectionBinding {
    pub const MAX_REVIEW_BYTES: usize = 64 * 1024;
    pub fn review(&self) -> Result<ClosureReview, ClosureInspectionError> {
        let c = &self.0;
        let id = |id: SourceId| format!("{id:?}");
        let mut writer = BoundedJsonWriter::new(Self::MAX_REVIEW_BYTES, true);
        writer.raw("{\"root\":")?;
        writer.string(&id(c.root))?;
        writer.raw(",\"sources\":[")?;
        for (index, source) in c.sources.iter().enumerate() {
            if index != 0 {
                writer.raw(",")?;
            }
            writer.raw("{\"id\":")?;
            writer.string(&id(source.id))?;
            writer.raw(",\"version\":")?;
            writer.raw(&source.version.to_string())?;
            for (name, text) in [
                ("language", &source.language),
                ("module", &source.module),
                ("origin", &source.origin),
            ] {
                writer.raw(",")?;
                writer.string(name)?;
                writer.raw(":")?;
                writer.string(text)?;
            }
            writer.raw(",\"byte_length\":")?;
            writer.raw(&source.text.len().to_string())?;
            writer.raw("}")?;
        }
        writer.raw("],\"graph\":[")?;
        for (index, (a, b)) in c.graph.iter().enumerate() {
            if index != 0 {
                writer.raw(",")?;
            }
            writer.raw("[")?;
            writer.string(&id(*a))?;
            writer.raw(",")?;
            writer.string(&id(*b))?;
            writer.raw("]")?;
        }
        writer.raw("],\"arguments\":")?;
        writer.serialize(self.arguments())?;
        writer.raw(",\"operation\":")?;
        writer.operation(&c.operation)?;
        for (name, text) in [
            ("compilation_identity", &c.compilation_identity),
            ("artifact_identity", &c.artifact_identity),
        ] {
            writer.raw(",")?;
            writer.string(name)?;
            writer.raw(":")?;
            writer.string(text)?;
        }
        for (name, value) in [
            ("compilation_options", &c.compilation_options),
            ("compilation_limits", &c.compilation_limits),
            ("execution_policy", &c.execution_policy),
        ] {
            writer.raw(",")?;
            writer.string(name)?;
            writer.raw(":")?;
            writer.serialize(value)?;
        }
        writer.raw("}")?;
        let text = String::from_utf8(writer.output.expect("retained writer"))
            .map_err(|_| ClosureInspectionError::InvalidCapture)?;
        Ok(ClosureReview(text))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionPhase {
    Capture,
    BeforePolicy,
    AfterPolicy,
    BeforeDispatch,
    Dispatch,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionCategory {
    InvalidCapture,
    LimitExceeded,
    SnapshotMismatch,
    Denied,
    HostFailure,
}

/// Source-local byte offsets, validated against the named historical revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectionLocation {
    source: SourceId,
    module: String,
    revision: u64,
    span: Option<std::ops::Range<usize>>,
}
impl RejectionLocation {
    pub fn source(&self) -> SourceId {
        self.source
    }
    pub fn module(&self) -> &str {
        &self.module
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn span(&self) -> Option<std::ops::Range<usize>> {
        self.span.clone()
    }
}
/// Source-free rejection evidence, separate from a successful observation.
/// Unknown execution after an adapter error is None, never a claimed false.
#[derive(Debug, Clone)]
pub struct ClosureRejection {
    phase: RejectionPhase,
    category: RejectionCategory,
    root: Option<SourceId>,
    location: Option<RejectionLocation>,
    sealed: bool,
    authorization_requested: bool,
    executed: Option<bool>,
    seal: Option<Arc<()>>,
}
impl ClosureRejection {
    pub fn phase(&self) -> RejectionPhase {
        self.phase
    }
    pub fn category(&self) -> RejectionCategory {
        self.category
    }
    pub fn root(&self) -> Option<SourceId> {
        self.root
    }
    pub fn location(&self) -> Option<&RejectionLocation> {
        self.location.as_ref()
    }
    pub fn sealed(&self) -> bool {
        self.sealed
    }
    pub fn authorization_requested(&self) -> bool {
        self.authorization_requested
    }
    pub fn executed(&self) -> Option<bool> {
        self.executed
    }
    /// Attach an explicit host-reported location in a named historical snapshot.
    /// Requires the original immutable seal (or its clone), not a later capture
    /// with overlapping owner/root IDs. Validates UTF-8 offsets, not diagnostic truth.
    pub fn with_location(
        mut self,
        binding: &ClosureInspectionBinding,
        source: SourceId,
        span: Option<std::ops::Range<usize>>,
    ) -> Result<Self, ClosureInspectionError> {
        if !self
            .seal
            .as_ref()
            .is_some_and(|seal| Arc::ptr_eq(seal, &binding.1))
        {
            return Err(ClosureInspectionError::InvalidCapture);
        }
        self.location = Some(binding.validate_location(source, span)?);
        Ok(self)
    }
    fn new(error: ClosureInspectionError, phase: RejectionPhase, root: Option<SourceId>) -> Self {
        let category = match error {
            ClosureInspectionError::InvalidCapture => RejectionCategory::InvalidCapture,
            ClosureInspectionError::LimitExceeded => RejectionCategory::LimitExceeded,
            ClosureInspectionError::SnapshotMismatch => RejectionCategory::SnapshotMismatch,
            ClosureInspectionError::Denied => RejectionCategory::Denied,
            ClosureInspectionError::Host(_) => RejectionCategory::HostFailure,
        };
        Self {
            phase,
            category,
            root,
            location: None,
            seal: None,
            sealed: phase != RejectionPhase::Capture,
            authorization_requested: matches!(
                phase,
                RejectionPhase::AfterPolicy
                    | RejectionPhase::BeforeDispatch
                    | RejectionPhase::Dispatch
            ),
            executed: if phase == RejectionPhase::Dispatch {
                None
            } else {
                Some(false)
            },
        }
    }
}
impl ClosureInspectionBinding {
    /// Failed sealing has no validated root/location. No adapter error strings retained.
    pub fn capture_with_evidence(capture: ClosureCapture) -> Result<Self, ClosureRejection> {
        Self::capture(capture).map_err(|e| ClosureRejection::new(e, RejectionPhase::Capture, None))
    }
    /// Explicit location validation; invocation failures never invent locations.
    pub fn validate_location(
        &self,
        source: SourceId,
        span: Option<std::ops::Range<usize>>,
    ) -> Result<RejectionLocation, ClosureInspectionError> {
        let s = self
            .0
            .sources
            .iter()
            .find(|s| s.id == source)
            .ok_or(ClosureInspectionError::InvalidCapture)?;
        if let Some(range) = &span
            && (range.start > range.end
                || range.end > s.text.len()
                || !s.text.is_char_boundary(range.start)
                || !s.text.is_char_boundary(range.end))
        {
            return Err(ClosureInspectionError::InvalidCapture);
        }
        Ok(RejectionLocation {
            source,
            module: s.module.clone(),
            revision: s.version,
            span,
        })
    }
}

/// New callback contract nesting Scope's semantic policy envelope.
/// This is NOT enforcement by, or integration with, SemanticToolProvider.
pub struct ClosureAuthorization<'a> {
    pub semantic: SemanticAuthorization<'a>,
    pub binding: &'a ClosureInspectionBinding,
}

/// Trusted host adapter, not an untrusted language provider. Recapture must include
/// current operation metadata, artifact identity and policy, not just source revisions.
/// The exclusive borrow must exclude concurrent mutation through dispatch; adapters
/// with interior/external mutability must provide their own lock across this call.
/// The adapter owns complete dependency discovery, stable owner/SourceId mapping,
/// truthful compiler/artifact/policy identities, and enforcement of execution limits.
/// It must dispatch exactly the supplied snapshot to the intended provider and must
/// not execute during recapture. This module cannot police a dishonest adapter.
/// Opaque metadata must be host-controlled and source-free before review display.
pub trait ClosureInspectionHost {
    type Output;
    fn recapture(&mut self) -> Result<ClosureInspectionBinding, ClosureInspectionError>;
    fn dispatch(
        &mut self,
        binding: &ClosureInspectionBinding,
    ) -> Result<Self::Output, ClosureInspectionError>;
}

/// Non-Clone, consumed even on denial/failure; no reusable authorization grant.
#[derive(Debug)]
pub struct ClosureInvocation(ClosureInspectionBinding);
impl ClosureInvocation {
    pub fn invoke<H: ClosureInspectionHost>(
        self,
        host: &mut H,
        mut authorize: impl for<'a> FnMut(ClosureAuthorization<'a>) -> bool,
    ) -> Result<ClosureInspectionResult<H::Output>, ClosureRejection> {
        let binding = self.0;
        let check = |host: &mut H| {
            if host.recapture()? != binding {
                return Err(ClosureInspectionError::SnapshotMismatch);
            }
            Ok(())
        };
        let reject = |error, phase| {
            let mut evidence = ClosureRejection::new(error, phase, Some(binding.0.root));
            evidence.seal = Some(binding.1.clone());
            evidence
        };
        check(host).map_err(|e| reject(e, RejectionPhase::BeforePolicy))?;
        let root = binding
            .0
            .sources
            .iter()
            .find(|s| s.id == binding.0.root)
            .expect("validated root");
        let allowed = authorize(ClosureAuthorization {
            semantic: SemanticAuthorization {
                operation: &binding.0.operation,
                source: SourceSnapshot {
                    id: root.id,
                    text: &root.text,
                    version: root.version,
                    language: &root.language,
                },
                arguments: binding.arguments(),
            },
            binding: &binding,
        });
        check(host).map_err(|e| reject(e, RejectionPhase::AfterPolicy))?;
        if !allowed {
            return Err(reject(
                ClosureInspectionError::Denied,
                RejectionPhase::AfterPolicy,
            ));
        }
        check(host).map_err(|e| reject(e, RejectionPhase::BeforeDispatch))?;
        let output = host
            .dispatch(&binding)
            .map_err(|e| reject(e, RejectionPhase::Dispatch))?;
        Ok(ClosureInspectionResult { binding, output })
    }
}

/// Historical observation only. Retaining/displaying this value confers no grant.
#[derive(Debug)]
pub struct ClosureInspectionResult<T> {
    pub binding: ClosureInspectionBinding,
    pub output: T,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    #[test]
    fn admission_bounds_all_json_fields_before_recursive_work() {
        let fields: [fn(&mut ClosureCapture) -> &mut Value; 4] = [
            |c| &mut c.compilation_options,
            |c| &mut c.compilation_limits,
            |c| &mut c.execution_policy,
            |c| c.operation.constraints.get_or_insert(Value::Null),
        ];
        for field in fields {
            let mut c = capture();
            // Iteratively built, far deeper than serde serialization/Drop can safely recurse.
            let mut deep = Value::Null;
            for _ in 0..20_000 {
                deep = Value::Array(vec![deep]);
            }
            *field(&mut c) = deep;
            assert_eq!(
                ClosureInspectionBinding::capture(c).unwrap_err(),
                ClosureInspectionError::LimitExceeded
            );
            let mut c = capture();
            *field(&mut c) =
                Value::Array(vec![Value::Null; ClosureInspectionBinding::MAX_JSON_NODES]);
            assert_eq!(
                ClosureInspectionBinding::capture(c).unwrap_err(),
                ClosureInspectionError::LimitExceeded
            );
            let mut c = capture();
            *field(&mut c) =
                Value::String("x".repeat(ClosureInspectionBinding::MAX_JSON_STRING_BYTES + 1));
            assert_eq!(
                ClosureInspectionBinding::capture(c).unwrap_err(),
                ClosureInspectionError::LimitExceeded
            );
        }
        // Object keys count toward the aggregate string-byte budget too.
        let mut c = capture();
        c.operation.constraints = Some(Value::Object(serde_json::Map::from_iter([(
            "k".repeat(ClosureInspectionBinding::MAX_JSON_STRING_BYTES + 1),
            Value::Null,
        )])));
        assert_eq!(
            ClosureInspectionBinding::capture(c).unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
        let mut c = capture();
        c.compilation_options = Value::Array(vec![
            Value::Null;
            ClosureInspectionBinding::MAX_JSON_NODES / 2
        ]);
        c.compilation_limits = Value::Array(vec![
            Value::Null;
            ClosureInspectionBinding::MAX_JSON_NODES / 2
        ]);
        assert_eq!(
            ClosureInspectionBinding::capture(c).unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
        let mut c = capture();
        c.compilation_options =
            Value::String("a".repeat(ClosureInspectionBinding::MAX_JSON_STRING_BYTES / 2));
        c.execution_policy =
            Value::String("b".repeat(ClosureInspectionBinding::MAX_JSON_STRING_BYTES / 2 + 1));
        assert_eq!(
            ClosureInspectionBinding::capture(c).unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
    }
    #[test]
    fn structured_admission_precedes_schema_equality_and_disposes_rejections() {
        fn deep() -> Value {
            let mut v = Value::Null;
            for _ in 0..20_000 {
                v = Value::Array(vec![v]);
            }
            v
        }
        for keyword in ["const", "enum"] {
            let mut c = capture();
            let constraint = if keyword == "enum" {
                Value::Array(vec![deep()])
            } else {
                deep()
            };
            let mut schema = serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false});
            schema[keyword] = constraint;
            c.operation.constraints = Some(Value::Object(serde_json::Map::from_iter([(
                "scope_argument_schema".into(),
                schema,
            )])));
            assert_eq!(
                ClosureInspectionBinding::capture_with_arguments(c, deep()).unwrap_err(),
                ClosureInspectionError::LimitExceeded
            );
        }
        let c = capture();
        assert_eq!(
            ClosureInspectionBinding::validate_arguments(&c.operation, &IterativeJson(deep()).0),
            Err(ClosureInspectionError::LimitExceeded)
        );
        assert_eq!(
            ClosureInspectionBinding::capture_with_arguments(c, deep()).unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
        let mut c = capture();
        c.operation.constraints = Some(serde_json::json!({"scope_argument_schema":false}));
        let oversized =
            serde_json::json!({"x":"x".repeat(ClosureInspectionBinding::MAX_ARGUMENT_BYTES)});
        assert_eq!(
            ClosureInspectionBinding::validate_arguments(&c.operation, &oversized),
            Err(ClosureInspectionError::LimitExceeded)
        );
    }
    #[test]
    fn admitted_depth_is_safe_to_clone_compare_and_review() {
        let mut c = capture();
        let mut value = Value::Null;
        for _ in 1..ClosureInspectionBinding::MAX_JSON_DEPTH {
            value = Value::Array(vec![value]);
        }
        c.operation.constraints = Some(value);
        let b = ClosureInspectionBinding::capture(c).unwrap();
        let cloned = b.clone();
        assert_eq!(b, cloned);
        let review = b.review().unwrap();
        let parsed: Value = serde_json::from_str(review.as_str()).unwrap();
        assert_eq!(
            parsed["operation"]["constraints"],
            *b.snapshot().operation.constraints.as_ref().unwrap()
        );
    }
    #[test]
    fn bounded_writer_counts_escaping_and_stops_at_budget() {
        let value = Value::String("\nλ\"".repeat(100));
        let expected = serde_json::to_vec(&value).unwrap();
        for retain in [false, true] {
            let mut writer = BoundedJsonWriter::new(expected.len(), retain);
            writer.serialize(&value).unwrap();
            assert_eq!(writer.count, expected.len());
            if let Some(output) = writer.output {
                assert_eq!(output, expected);
            }
            let mut writer = BoundedJsonWriter::new(expected.len() - 1, retain);
            assert_eq!(
                writer.serialize(&value).unwrap_err(),
                ClosureInspectionError::LimitExceeded
            );
            assert!(writer.count < expected.len());
            if let Some(output) = writer.output {
                assert!(output.len() < expected.len());
            }
        }
        // Raw string bytes fit admission, but escaping exceeds the capture budget.
        let mut c = capture();
        c.operation.constraints = Some(Value::String(
            "\n".repeat(ClosureInspectionBinding::MAX_BYTES / 2),
        ));
        assert_eq!(
            ClosureInspectionBinding::capture(c).unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
        let mut c = capture();
        c.operation.constraints = Some(Value::String(
            "\n".repeat(ClosureInspectionBinding::MAX_REVIEW_BYTES / 2),
        ));
        let b = ClosureInspectionBinding::capture(c).unwrap();
        assert_eq!(
            b.review().unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
    }
    #[test]
    fn rejection_location_requires_original_seal_not_later_owner_root() {
        let original = dependency_capture(1);
        let b = ClosureInspectionBinding::capture(original.clone()).unwrap();
        let evidence = b
            .invocation()
            .invoke(&mut host(original.clone()), |_| false)
            .unwrap_err();
        let cloned = b.clone();
        assert!(
            evidence
                .clone()
                .with_location(&cloned, b.snapshot().root, Some(8..10))
                .is_ok()
        );
        let mut revision = original.clone();
        revision.sources[0].version += 1;
        let mut graph = original.clone();
        graph.sources.push(ClosureSource {
            id: SourceId::Scratch(4),
            module: "new-dependency".into(),
            ..graph.sources[0].clone()
        });
        graph.graph.push((graph.root, SourceId::Scratch(4)));
        // Even an identical fresh capture is not the seal that produced the evidence.
        for later in [revision, graph, original] {
            let later = ClosureInspectionBinding::capture(later).unwrap();
            assert_eq!(later.snapshot().owner, b.snapshot().owner);
            assert_eq!(later.snapshot().root, b.snapshot().root);
            assert_eq!(
                evidence
                    .clone()
                    .with_location(&later, later.snapshot().root, Some(8..10))
                    .unwrap_err(),
                ClosureInspectionError::InvalidCapture
            );
        }
    }
    fn dependency_capture(root: u64) -> ClosureCapture {
        let mut c = capture();
        c.sources[0].id = SourceId::Scratch(root);
        c.root = SourceId::Scratch(root);
        c.sources.push(ClosureSource {
            id: SourceId::Scratch(3),
            module: "shared".into(),
            ..c.sources[0].clone()
        });
        c.graph.push((c.root, SourceId::Scratch(3)));
        c
    }
    #[test]
    fn graph_and_language_rejections_are_specific() {
        let base = dependency_capture(1);
        let mutations: Vec<fn(&mut ClosureCapture)> = vec![
            |c| c.sources.push(c.sources[1].clone()),
            |c| c.sources[1].module = c.sources[0].module.clone(),
            |c| c.graph.push(c.graph[0]),
            |c| c.sources.pop().map(|_| ()).unwrap(),
            |c| c.graph.push((c.root, SourceId::Scratch(99))),
            |c| c.graph.push((SourceId::Scratch(99), c.root)),
            |c| c.graph.push((SourceId::Scratch(3), c.root)),
            |c| c.graph.push((c.root, c.root)),
            |c| c.sources[1].language = "other".into(),
        ];
        for (index, mutate) in mutations.into_iter().enumerate() {
            let mut c = base.clone();
            mutate(&mut c);
            assert_eq!(
                ClosureInspectionBinding::capture(c).unwrap_err(),
                ClosureInspectionError::InvalidCapture,
                "case {index}"
            );
        }
        assert!(ClosureInspectionBinding::capture(base).is_ok());
    }
    #[test]
    fn limits_cover_counts_metadata_and_review_without_truncation() {
        let mut c = capture();
        c.sources = vec![c.sources[0].clone(); ClosureInspectionBinding::MAX_SOURCES + 1];
        assert_eq!(
            ClosureInspectionBinding::capture(c).unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
        let mut c = capture();
        c.graph = vec![(c.root, c.root); ClosureInspectionBinding::MAX_EDGES + 1];
        assert_eq!(
            ClosureInspectionBinding::capture(c).unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
        let mut c = capture();
        c.compilation_limits = Value::String("x".repeat(ClosureInspectionBinding::MAX_BYTES));
        assert_eq!(
            ClosureInspectionBinding::capture(c).unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
        let mut c = capture();
        c.operation.description = "x".repeat(ClosureInspectionBinding::MAX_REVIEW_BYTES);
        let b = ClosureInspectionBinding::capture(c).unwrap();
        assert_eq!(
            b.review().unwrap_err(),
            ClosureInspectionError::LimitExceeded
        );
    }
    #[test]
    fn review_and_debug_do_not_project_source_text() {
        let mut c = dependency_capture(1);
        c.sources[0].text = "SECRET-root-λ".into();
        c.sources[1].text = "SECRET-dependency".into();
        let b = ClosureInspectionBinding::capture(c).unwrap();
        let review = b.review().unwrap();
        let value: Value = serde_json::from_str(review.as_str()).unwrap();
        assert!(review.as_str().len() <= ClosureInspectionBinding::MAX_REVIEW_BYTES);
        assert_eq!(value["sources"].as_array().unwrap().len(), 2);
        assert_eq!(value["graph"].as_array().unwrap().len(), 1);
        assert_eq!(value["operation"]["id"], "inspect");
        assert_eq!(value["compilation_limits"]["fuel"], 10);
        assert_eq!(value["compilation_identity"], "compiler-1");
        assert_eq!(value["artifact_identity"], "artifact-1");
        for source in value["sources"].as_array().unwrap() {
            assert!(source.get("text").is_none());
        }
        for rendered in [
            review.as_str().to_owned(),
            format!("{b:?}"),
            format!("{:?}", b.snapshot()),
            format!("{:?}", b.invocation()),
        ] {
            assert!(!rendered.contains("SECRET"));
        }
    }
    #[test]
    fn spans_are_source_local_utf8_and_revision_bound() {
        let b = ClosureInspectionBinding::capture(capture()).unwrap();
        // 'λ' occupies bytes 8..10 of the unsaved source.
        let location = b
            .validate_location(SourceId::Scratch(1), Some(8..10))
            .unwrap();
        assert_eq!(location.revision, 1);
        assert_eq!(location.module, "main");
        for span in [9..10, 8..9, 0..11, std::ops::Range { start: 10, end: 8 }] {
            assert_eq!(
                b.validate_location(SourceId::Scratch(1), Some(span))
                    .unwrap_err(),
                ClosureInspectionError::InvalidCapture
            );
        }
        assert!(
            b.validate_location(SourceId::Scratch(1), Some(10..10))
                .is_ok()
        );
        assert!(
            b.validate_location(SourceId::Scratch(1), None)
                .unwrap()
                .span
                .is_none()
        );
        assert_eq!(
            b.validate_location(SourceId::Scratch(99), None)
                .unwrap_err(),
            ClosureInspectionError::InvalidCapture
        );
    }
    #[test]
    fn shared_dependency_does_not_alias_roots_and_removal_keeps_history() {
        let first = dependency_capture(1);
        let mut second = dependency_capture(2);
        second.owner = first.owner.clone();
        let binding = ClosureInspectionBinding::capture(first.clone()).unwrap();
        let mut h = host(first);
        let result = binding.invocation().invoke(&mut h, |_| true).unwrap();
        *h.state.borrow_mut() = second.clone();
        let error = binding
            .invocation()
            .invoke(&mut h, |_| panic!("wrong root"))
            .unwrap_err();
        assert_eq!(error.category, RejectionCategory::SnapshotMismatch);
        let second_binding = ClosureInspectionBinding::capture(second).unwrap();
        second_binding
            .invocation()
            .invoke(&mut h, |_| true)
            .unwrap();
        h.state.borrow_mut().sources.clear();
        let error = result
            .binding
            .invocation()
            .invoke(&mut h, |_| panic!("removed"))
            .unwrap_err();
        assert_eq!(error.phase, RejectionPhase::BeforePolicy);
        assert_eq!(error.category, RejectionCategory::LimitExceeded);
        assert!(error.location.is_none());
        assert!(!error.authorization_requested);
        assert_eq!(error.executed, Some(false));
        assert_eq!(result.output, "historical read");
        assert_eq!(result.binding.snapshot().sources.len(), 2);
        assert_eq!(h.dispatches, 2);
    }
    #[test]
    fn rejection_evidence_records_only_observed_facts() {
        let mut invalid = capture();
        invalid.sources.clear();
        let e = ClosureInspectionBinding::capture_with_evidence(invalid).unwrap_err();
        assert_eq!(e.phase, RejectionPhase::Capture);
        assert!(!e.sealed);
        assert!(e.root.is_none());
        assert!(e.location.is_none());
        assert!(!e.authorization_requested);
        assert_eq!(e.executed, Some(false));
        let c = capture();
        let b = ClosureInspectionBinding::capture(c.clone()).unwrap();
        for boundary in 1..=3 {
            let mut h = host(c.clone());
            h.mutate_at = boundary;
            let e = b.invocation().invoke(&mut h, |_| true).unwrap_err();
            assert_eq!(
                e.phase,
                [
                    RejectionPhase::BeforePolicy,
                    RejectionPhase::AfterPolicy,
                    RejectionPhase::BeforeDispatch
                ][boundary - 1]
            );
            assert_eq!(e.category, RejectionCategory::SnapshotMismatch);
            assert_eq!(e.authorization_requested, boundary > 1);
            assert!(e.sealed);
            assert_eq!(e.root, Some(c.root));
            assert!(e.location.is_none());
            assert_eq!(e.executed, Some(false));
        }
        let e = b
            .invocation()
            .invoke(&mut host(c.clone()), |_| false)
            .unwrap_err();
        assert_eq!(e.category, RejectionCategory::Denied);
        assert!(e.authorization_requested);
        assert_eq!(e.executed, Some(false));
        let located = e.clone().with_location(&b, c.root, Some(8..10)).unwrap();
        assert_eq!(located.location().unwrap().span(), Some(8..10));
        assert_eq!(located.location().unwrap().revision(), 1);
        assert_eq!(
            e.clone()
                .with_location(&b, c.root, Some(9..10))
                .unwrap_err(),
            ClosureInspectionError::InvalidCapture
        );
        let foreign = ClosureInspectionBinding::capture(capture()).unwrap();
        assert_eq!(
            e.with_location(&foreign, c.root, None).unwrap_err(),
            ClosureInspectionError::InvalidCapture
        );
        let mut failing = host(c);
        failing.fail_dispatch = true;
        let e = b.invocation().invoke(&mut failing, |_| true).unwrap_err();
        assert_eq!(failing.dispatches, 1);
        assert_eq!(e.phase(), RejectionPhase::Dispatch);
        assert_eq!(e.executed, None);
        assert_eq!(e.category, RejectionCategory::HostFailure);
        assert!(!format!("{e:?}").contains("untrusted"));
    }
    fn capture() -> ClosureCapture {
        ClosureCapture {
            owner: ClosureOwner::default(),
            sources: vec![ClosureSource {
                id: SourceId::Scratch(1),
                text: "unsaved λ".into(),
                version: 1,
                language: "fake".into(),
                module: "main".into(),
                origin: "buffer".into(),
            }],
            root: SourceId::Scratch(1),
            graph: vec![],
            compilation_identity: "compiler-1".into(),
            compilation_options: serde_json::json!({"opt":false}),
            compilation_limits: serde_json::json!({"fuel":10}),
            operation: SemanticOperationDescription {
                id: "inspect".into(),
                description: "read closure".into(),
                arguments: vec![],
                effect: SemanticOperationEffect::Read,
                target: SemanticTargetRequirement::Document,
                constraints: None,
            },
            execution_policy: serde_json::json!({"native":false}),
            artifact_identity: "artifact-1".into(),
        }
    }
    struct Host {
        state: Rc<RefCell<ClosureCapture>>,
        captures: usize,
        dispatches: usize,
        mutate_at: usize,
        fail_dispatch: bool,
    }
    impl ClosureInspectionHost for Host {
        type Output = String;
        fn recapture(&mut self) -> Result<ClosureInspectionBinding, ClosureInspectionError> {
            self.captures += 1;
            if self.captures == self.mutate_at {
                self.state.borrow_mut().artifact_identity.push('!');
            }
            ClosureInspectionBinding::capture(self.state.borrow().clone())
        }
        fn dispatch(
            &mut self,
            binding: &ClosureInspectionBinding,
        ) -> Result<String, ClosureInspectionError> {
            self.dispatches += 1;
            assert_eq!(binding.snapshot(), &*self.state.borrow());
            if self.fail_dispatch {
                return Err(ClosureInspectionError::Host(
                    "untrusted source-bearing error".into(),
                ));
            }
            Ok("historical read".into())
        }
    }
    fn host(c: ClosureCapture) -> Host {
        Host {
            state: Rc::new(RefCell::new(c)),
            captures: 0,
            dispatches: 0,
            mutate_at: 0,
            fail_dispatch: false,
        }
    }
    #[test]
    fn exact_binding_rejects_adversarial_changes_before_policy() {
        let base = capture();
        let binding = ClosureInspectionBinding::capture(base.clone()).unwrap();
        let mutations: Vec<fn(&mut ClosureCapture)> = vec![
            |c| c.owner = ClosureOwner::default(),
            |c| {
                c.sources[0].id = SourceId::Scratch(2);
                c.root = SourceId::Scratch(2);
            },
            |c| c.sources[0].text.push('!'),
            |c| c.sources[0].version += 1,
            |c| c.sources[0].language.push('!'),
            |c| c.sources[0].module.push('!'),
            |c| c.sources[0].origin.push('!'),
            |c| {
                c.sources.push(ClosureSource {
                    id: SourceId::Scratch(2),
                    module: "dep".into(),
                    ..c.sources[0].clone()
                });
                c.graph.push((c.root, SourceId::Scratch(2)));
            },
            |c| c.compilation_identity.push('!'),
            |c| c.compilation_options = Value::Null,
            |c| c.compilation_limits = Value::Null,
            |c| c.operation.description.push('!'),
            |c| c.operation.id.push('!'),
            |c| c.operation.constraints = Some(serde_json::json!({"changed":true})),
            |c| c.execution_policy = Value::Null,
            |c| c.artifact_identity.push('!'),
        ];
        for mutate in mutations {
            let mut changed = base.clone();
            mutate(&mut changed);
            let mut host = host(changed);
            assert_eq!(
                binding
                    .invocation()
                    .invoke(&mut host, |_| panic!("stale policy"))
                    .unwrap_err()
                    .category,
                RejectionCategory::SnapshotMismatch
            );
            assert_eq!(host.dispatches, 0);
        }
    }
    #[test]
    fn denial_and_mutation_at_each_boundary_never_dispatch() {
        let base = capture();
        let binding = ClosureInspectionBinding::capture(base.clone()).unwrap();
        let mut denied = host(base.clone());
        assert_eq!(
            binding
                .invocation()
                .invoke(&mut denied, |_| false)
                .unwrap_err()
                .category,
            RejectionCategory::Denied
        );
        assert_eq!(denied.dispatches, 0);
        for boundary in 1..=3 {
            let mut host = host(base.clone());
            host.mutate_at = boundary;
            assert_eq!(
                binding
                    .invocation()
                    .invoke(&mut host, |_| true)
                    .unwrap_err()
                    .category,
                RejectionCategory::SnapshotMismatch
            );
            assert_eq!(host.dispatches, 0);
        }
        let mut host = host(base);
        let state = host.state.clone();
        assert_eq!(
            binding
                .invocation()
                .invoke(&mut host, |request| {
                    assert_eq!(request.semantic.operation, &binding.snapshot().operation);
                    assert_eq!(request.binding, &binding);
                    state.borrow_mut().sources[0].text.push('!');
                    true
                })
                .unwrap_err()
                .category,
            RejectionCategory::SnapshotMismatch
        );
        assert_eq!(host.dispatches, 0);
    }
    #[test]
    fn retained_result_and_reselection_are_not_reusable_grants() {
        let base = capture();
        let binding = ClosureInspectionBinding::capture(base.clone()).unwrap();
        let mut host = host(base);
        let mut policies = 0;
        let result = binding
            .invocation()
            .invoke(&mut host, |_| {
                policies += 1;
                true
            })
            .unwrap();
        assert_eq!(host.captures, 3);
        assert_eq!(host.dispatches, 1);
        assert_eq!(result.output, "historical read");
        assert_eq!(
            result
                .binding
                .invocation()
                .invoke(&mut host, |_| {
                    policies += 1;
                    false
                })
                .unwrap_err()
                .category,
            RejectionCategory::Denied
        );
        assert_eq!(policies, 2);
        assert_eq!(host.dispatches, 1);
        host.state.borrow_mut().sources[0].version += 1;
        assert_eq!(
            result
                .binding
                .invocation()
                .invoke(&mut host, |_| panic!("historical grant"))
                .unwrap_err()
                .category,
            RejectionCategory::SnapshotMismatch
        );
        assert_eq!(result.output, "historical read");
    }
    #[test]
    fn malformed_and_oversized_captures_fail_closed() {
        let mutations: Vec<fn(&mut ClosureCapture)> = vec![
            |c| c.sources.clear(),
            |c| c.sources.push(c.sources[0].clone()),
            |c| c.root = SourceId::Scratch(99),
            |c| c.graph.push((c.root, SourceId::Scratch(99))),
            |c| c.sources[0].text = "x".repeat(ClosureInspectionBinding::MAX_BYTES + 1),
            |c| c.operation.effect = SemanticOperationEffect::ProposeChanges,
            |c| c.operation.target = SemanticTargetRequirement::Selection,
            |c| c.artifact_identity.clear(),
            |c| {
                c.sources.push(ClosureSource {
                    id: SourceId::Scratch(2),
                    module: "other".into(),
                    ..c.sources[0].clone()
                })
            },
        ];
        for mutate in mutations {
            let mut c = capture();
            mutate(&mut c);
            let expected = if c.sources.is_empty()
                || c.sources[0].text.len() > ClosureInspectionBinding::MAX_BYTES
            {
                ClosureInspectionError::LimitExceeded
            } else {
                ClosureInspectionError::InvalidCapture
            };
            assert_eq!(ClosureInspectionBinding::capture(c).unwrap_err(), expected);
        }
    }
}
