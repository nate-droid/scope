//! Opt-in registered Document discovery. No filesystem discovery or native execution.
//!
//! The artifact is a trusted host/test-owned *handle*, not a verified loaded binary.
//! The default dispatch is an inert bounded counter. Typed trusted providers are opt-in. It
//! does not establish executable provenance, sandboxing, or production execution.
use crate::{
    closure_inspection::{
        ClosureCapture, ClosureInspectionBinding, ClosureInspectionError, ClosureInspectionHost,
        ClosureInspectionResult, ClosureOwner, ClosureRejection, ClosureSource,
    },
    document::Document,
    source_edit::SourceId as EditSourceId,
    tool::semantic::{HostAuthorizationRequest, HostSemanticPolicy, authorize_host_request},
};
use scope_language::{
    RevisionId, SemanticOperationDescription, SourceId, SourceResolver, SourceSnapshot,
    SourceUnavailable,
};
use serde_json::{Value, json};
/// Requested/effective compiler budgets, not permission to execute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub source_bytes: usize,
    pub tokens: usize,
    pub nodes: usize,
    pub depth: usize,
    pub steps: usize,
    pub recursion: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 65536,
            tokens: 8192,
            nodes: 4096,
            depth: 64,
            steps: 100000,
            recursion: 64,
        }
    }
}
impl Limits {
    fn value(self) -> Value {
        json!({"source_bytes":self.source_bytes,"tokens":self.tokens,"nodes":self.nodes,
        "depth":self.depth,"steps":self.steps,"recursion":self.recursion})
    }
    fn no_greater_than(self, requested: Self) -> bool {
        self.source_bytes <= requested.source_bytes
            && self.tokens <= requested.tokens
            && self.nodes <= requested.nodes
            && self.depth <= requested.depth
            && self.steps <= requested.steps
            && self.recursion <= requested.recursion
    }
}

/// Host-owned ABI identities; all fields are sealed, not interpreted as authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvocationContract {
    pub schema_version: String,
    pub entry: String,
    pub output: String,
    pub effect: String,
}
impl InvocationContract {
    fn validate(&self) -> Result<(), ClosureInspectionError> {
        if [
            &self.schema_version,
            &self.entry,
            &self.output,
            &self.effect,
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > 1024)
        {
            return Err(ClosureInspectionError::InvalidCapture);
        }
        Ok(())
    }
}

/// Registry-assigned labels are not paths, DocumentIds, or authorization.
#[derive(Clone, PartialEq, Eq)]
pub struct RegisteredSource {
    pub identity: String,
    pub module: String,
    pub revision: String,
    pub text: String,
    pub language: String,
    pub display: Option<String>,
    pub origin: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryPhase {
    Registration,
    Acquisition,
    Validation,
    Compilation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryCategory {
    Bounds,
    DuplicateOrCapacity,
    MissingSource,
    Language,
    SourceAdmission,
    GraphOrMetadata,
    Compilation,
}
/// Deliberately contains no compiler message or source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscoveryError {
    pub phase: DiscoveryPhase,
    pub category: DiscoveryCategory,
}
impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "discovery {:?}/{:?}", self.phase, self.category)
    }
}
impl std::error::Error for DiscoveryError {}

/// Discovery categories remain source-free. Optional trusted compiler diagnostics
/// are presentation evidence, separate from generic capture validation. The
/// optional root is resolved only from this registry.
/// Authorization/freshness failures during invocation remain `ClosureRejection`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegisteredCaptureRejection {
    Discovery {
        error: DiscoveryError,
        root: Option<SourceId>,
    },
    Diagnostics {
        error: DiscoveryError,
        root: Option<SourceId>,
        diagnostics: Vec<DiscoveryDiagnostic>,
    },
    Inspection(ClosureInspectionError),
}
impl From<ClosureInspectionError> for RegisteredCaptureRejection {
    fn from(error: ClosureInspectionError) -> Self {
        Self::Inspection(error)
    }
}
impl RegisteredCaptureRejection {
    fn into_compatibility_error(self) -> ClosureInspectionError {
        match self {
            Self::Discovery { error, .. } | Self::Diagnostics { error, .. } => {
                ClosureInspectionError::Host(error.to_string())
            }
            Self::Inspection(error) => error,
        }
    }
}

/// Owned transfer from a trusted read-only language facade. `compiler_sources`
/// maps compiler source index to registered identity, in exact compiler order.
/// Portable identity may contain source text; never render it as review metadata.
pub struct DiscoveryEvidence {
    pub root: String,
    pub sources: Vec<RegisteredSource>,
    pub graph: Vec<(String, Vec<String>)>,
    pub exports: Vec<(String, Vec<String>)>,
    pub compiler_sources: Vec<String>,
    pub compiler_identity: String,
    pub identity_version: u32,
    pub portable_identity: Vec<u8>,
    pub limits: Limits,
}

/// Admitted immutable evidence; owned input allocation is the facade's
/// responsibility. Scope bounds retention and validates against registered input.
pub struct DiscoveredClosure(DiscoveryEvidence);
impl DiscoveredClosure {
    pub const MAX_BYTES: usize = ClosureInspectionBinding::MAX_BYTES;
    pub fn new(evidence: DiscoveryEvidence) -> Result<Self, DiscoveryError> {
        use std::collections::BTreeSet;
        let invalid = || DiscoveryError {
            phase: DiscoveryPhase::Validation,
            category: DiscoveryCategory::GraphOrMetadata,
        };
        let bounds = || DiscoveryError {
            phase: DiscoveryPhase::Validation,
            category: DiscoveryCategory::Bounds,
        };
        if evidence.sources.is_empty()
            || evidence.sources.len() > ClosureInspectionBinding::MAX_SOURCES
            || evidence.graph.len() != evidence.sources.len()
            || evidence.exports.len() != evidence.sources.len()
            || evidence.compiler_sources.len() != evidence.sources.len()
        {
            return Err(invalid());
        }
        let mut bytes = evidence.portable_identity.len();
        let mut admit = |text: &str| -> Result<(), DiscoveryError> {
            bytes = bytes.checked_add(text.len()).ok_or_else(bounds)?;
            if bytes > Self::MAX_BYTES {
                return Err(bounds());
            }
            Ok(())
        };
        admit(&evidence.root)?;
        admit(&evidence.compiler_identity)?;
        if evidence.compiler_identity.is_empty()
            || evidence.identity_version == 0
            || evidence.portable_identity.is_empty()
        {
            return Err(invalid());
        }
        let mut modules = BTreeSet::new();
        let mut identities = BTreeSet::new();
        for source in &evidence.sources {
            if source.identity.is_empty()
                || source.module.is_empty()
                || source.revision.is_empty()
                || source.language.is_empty()
                || !modules.insert(source.module.as_str())
                || !identities.insert(source.identity.as_str())
            {
                return Err(invalid());
            }
            for text in [
                &source.identity,
                &source.module,
                &source.revision,
                &source.text,
                &source.language,
            ] {
                admit(text)?;
            }
            for text in [&source.display, &source.origin].into_iter().flatten() {
                admit(text)?;
            }
        }
        if !modules.contains(evidence.root.as_str()) {
            return Err(invalid());
        }
        let mut indices = BTreeSet::new();
        for identity in &evidence.compiler_sources {
            admit(identity)?;
            if !identities.contains(identity.as_str()) || !indices.insert(identity.as_str()) {
                return Err(invalid());
            }
        }
        for (table, is_graph) in [(&evidence.graph, true), (&evidence.exports, false)] {
            let mut seen = BTreeSet::new();
            let mut count = 0usize;
            for (module, values) in table {
                admit(module)?;
                if !modules.contains(module.as_str()) || !seen.insert(module.as_str()) {
                    return Err(invalid());
                }
                let mut unique = BTreeSet::new();
                for value in values {
                    count = count.checked_add(1).ok_or_else(bounds)?;
                    if count > ClosureInspectionBinding::MAX_EDGES {
                        return Err(bounds());
                    }
                    admit(value)?;
                    if value.is_empty() || !unique.insert(value.as_str()) {
                        return Err(invalid());
                    }
                    if is_graph && !modules.contains(value.as_str()) {
                        return Err(invalid());
                    }
                }
            }
        }
        Ok(Self(evidence))
    }
    pub fn root(&self) -> &str {
        &self.0.root
    }
    pub fn sources(&self) -> &[RegisteredSource] {
        &self.0.sources
    }
    pub fn graph(&self) -> &[(String, Vec<String>)] {
        &self.0.graph
    }
    pub fn exports(&self) -> &[(String, Vec<String>)] {
        &self.0.exports
    }
    pub fn compiler_sources(&self) -> &[String] {
        &self.0.compiler_sources
    }
    pub fn compiler_identity(&self) -> &str {
        &self.0.compiler_identity
    }
    pub fn identity_version(&self) -> u32 {
        self.0.identity_version
    }
    pub fn portable_identity(&self) -> &[u8] {
        &self.0.portable_identity
    }
    pub fn limits(&self) -> Limits {
        self.0.limits
    }
}

/// Trusted compiler presentation evidence, separate from source-free error categories.
/// Offsets are UTF-8 byte offsets in the supplied document snapshot; line/column
/// are one-based (column counts Unicode scalar values). Never authority evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryDiagnostic {
    pub module: String,
    pub path: Option<String>,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryFailure {
    pub error: DiscoveryError,
    pub diagnostics: Vec<DiscoveryDiagnostic>,
}
/// Trusted host-owned language facade, NOT a policy provider. Must perform only
/// bounded read-only discovery over supplied sources; no evaluator, filesystem
/// fallback, approval, or externally mutable execution state. Production language
/// crates implement this trait without Scope depending on the language crate.
pub trait Discovery {
    /// One discovery pass, with optional compiler-derived presentation evidence.
    /// Existing adapters remain source-free. Must obey the same capture contract.
    fn capture_with_diagnostics(
        &self,
        root: &str,
        sources: Vec<RegisteredSource>,
        limits: Limits,
    ) -> Result<DiscoveredClosure, DiscoveryFailure> {
        self.capture(root, sources, limits)
            .map_err(|error| DiscoveryFailure {
                error,
                diagnostics: Vec::new(),
            })
    }
    fn capture(
        &self,
        root: &str,
        sources: Vec<RegisteredSource>,
        limits: Limits,
    ) -> Result<DiscoveredClosure, DiscoveryError>;
}
use std::{collections::BTreeMap, sync::Arc};

/// Opaque, non-forgeable process-local handle, assigned by a trusted host. No
/// claim that the handle's label authenticates code or that code is executable.
#[derive(Clone)]
pub struct TrustedLoadedArtifact(Arc<ArtifactImplementation>);

/// Trusted host assertion about an admitted component. A digest identifies bytes,
/// not authenticity or faithful language implementation. The host must derive this
/// from loaded bytes and retain the corresponding immutable prepared handle.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedImplementationDescriptor {
    pub sha256: String,
    pub abi: String,
    pub execution_policy: Value,
}

struct ArtifactImplementation {
    descriptor: Option<PreparedImplementationDescriptor>,
    _pin: Box<dyn std::any::Any + Send + Sync>,
}

impl TrustedLoadedArtifact {
    pub fn prepared(
        descriptor: PreparedImplementationDescriptor,
        pin: impl std::any::Any + Send + Sync,
    ) -> Result<Self, ClosureInspectionError> {
        if descriptor.sha256.len() != 64
            || !descriptor.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || descriptor.abi.is_empty()
            || descriptor.abi.len() > 256
            || !descriptor.execution_policy.is_object()
        {
            return Err(ClosureInspectionError::InvalidCapture);
        }
        Ok(Self(Arc::new(ArtifactImplementation {
            descriptor: Some(descriptor),
            _pin: Box::new(pin),
        })))
    }

    pub fn descriptor(&self) -> Option<&PreparedImplementationDescriptor> {
        self.0.descriptor.as_ref()
    }
}
impl Default for TrustedLoadedArtifact {
    fn default() -> Self {
        Self(Arc::new(ArtifactImplementation {
            descriptor: None,
            _pin: Box::new(()),
        }))
    }
}

struct Entry {
    document: Document,
    language: String,
    evaluation_id: SourceId,
    binding_id: EditSourceId,
    registration: u64,
}

/// Owns actual editable Documents and all mutable dispatch metadata. No shared
/// interior mutability: an exclusive synchronous borrow covers final check and
/// dispatch. DocumentId is never used as an evaluation or owner identity.
pub struct RegisteredDocuments<P = InertProvider> {
    provider: P,
    discovery: Box<dyn Discovery>,
    owner: ClosureOwner,
    entries: BTreeMap<String, Entry>,
    next_registration: u64,
    artifact: TrustedLoadedArtifact,
    artifact_generation: u64,
    policy: Value,
    catalog: Vec<SemanticOperationDescription>,
    limits: Limits,
    dispatches: u64,
    max_dispatches: u64,
}
impl RegisteredDocuments<InertProvider> {
    pub fn new(
        discovery: impl Discovery + 'static,
        artifact: TrustedLoadedArtifact,
        catalog: Vec<SemanticOperationDescription>,
        policy: Value,
        limits: Limits,
        max_dispatches: u64,
    ) -> Self {
        Self {
            provider: InertProvider,
            discovery: Box::new(discovery),
            owner: ClosureOwner::default(),
            entries: BTreeMap::new(),
            next_registration: 0,
            artifact,
            artifact_generation: 0,
            policy,
            catalog,
            limits,
            dispatches: 0,
            max_dispatches,
        }
    }
    pub fn set_artifact(
        &mut self,
        artifact: TrustedLoadedArtifact,
    ) -> Result<(), ClosureInspectionError> {
        self.replace_artifact(artifact)
    }
}
impl<P: TrustedClosureProvider> RegisteredDocuments<P> {
    /// Compatibility shorthand: the trusted caller asserts this Document is
    /// Sprout source. This does not detect language from text or file extension.
    pub fn register(
        &mut self,
        module: String,
        document: Document,
    ) -> Result<Option<Document>, ClosureInspectionError> {
        self.register_with_language(module, document, "sprout".into())
    }

    /// Register the host's explicit language assertion, passed unchanged to
    /// discovery. The facade must reject languages it does not support.
    pub fn register_with_language(
        &mut self,
        module: String,
        document: Document,
        language: String,
    ) -> Result<Option<Document>, ClosureInspectionError> {
        if language.is_empty() {
            return Err(ClosureInspectionError::InvalidCapture);
        }
        if language.len() > 128 {
            return Err(ClosureInspectionError::LimitExceeded);
        }
        if module.is_empty()
            || module.len() > 128
            || (!self.entries.contains_key(&module)
                && self.entries.len() >= ClosureInspectionBinding::MAX_SOURCES)
        {
            return Err(ClosureInspectionError::LimitExceeded);
        }
        if self.entries.iter().any(|(name, entry)| {
            name != &module && &entry.evaluation_id == document.evaluation_source_id()
        }) {
            return Err(ClosureInspectionError::InvalidCapture);
        }
        self.next_registration = self
            .next_registration
            .checked_add(1)
            .ok_or(ClosureInspectionError::LimitExceeded)?;
        let registration = self.next_registration;
        // Explicit mapping from the actual Arc-backed evaluation identity to a
        // registry-local generic binding identity. Neither is a DocumentId cast.
        let evaluation_id = document.evaluation_source_id().clone();
        let entry = Entry {
            document,
            language,
            evaluation_id,
            binding_id: EditSourceId::Scratch(registration),
            registration,
        };
        Ok(self.entries.insert(module, entry).map(|e| e.document))
    }
    /// Insert without replacement, preserving ownership on every admission error.
    pub fn try_register_with_language(
        &mut self,
        module: String,
        document: Document,
        language: String,
    ) -> Result<(), Box<(ClosureInspectionError, Document)>> {
        let error = if language.is_empty()
            || self.entries.contains_key(&module)
            || self.entries.values().any(|entry| {
                entry.evaluation_id == *document.evaluation_source_id()
                    || entry.document.path() == document.path()
            }) {
            Some(ClosureInspectionError::InvalidCapture)
        } else if module.is_empty()
            || module.len() > 128
            || language.len() > 128
            || self.entries.len() >= ClosureInspectionBinding::MAX_SOURCES
            || self.next_registration == u64::MAX
        {
            Some(ClosureInspectionError::LimitExceeded)
        } else {
            None
        };
        if let Some(error) = error {
            return Err(Box::new((error, document)));
        }
        // All fallible admission checks precede moving the document.
        self.register_with_language(module, document, language)
            .expect("admission was checked under the same exclusive borrow");
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn documents(&self) -> impl Iterator<Item = (&str, &Document)> {
        self.entries
            .iter()
            .map(|(name, entry)| (name.as_str(), &entry.document))
    }

    pub fn document(&self, module: &str) -> Option<&Document> {
        self.entries.get(module).map(|e| &e.document)
    }
    pub fn document_mut(&mut self, module: &str) -> Option<&mut Document> {
        self.entries.get_mut(module).map(|e| &mut e.document)
    }
    pub fn remove(&mut self, module: &str) -> Option<Document> {
        self.entries.remove(module).map(|e| e.document)
    }
    pub fn set_policy(&mut self, policy: Value) {
        self.policy = policy;
    }
    pub fn set_catalog(&mut self, catalog: Vec<SemanticOperationDescription>) {
        self.catalog = catalog;
    }
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }
    fn replace_artifact(
        &mut self,
        artifact: TrustedLoadedArtifact,
    ) -> Result<(), ClosureInspectionError> {
        if !Arc::ptr_eq(&self.artifact.0, &artifact.0) {
            self.artifact_generation = self
                .artifact_generation
                .checked_add(1)
                .ok_or(ClosureInspectionError::LimitExceeded)?;
            self.artifact = artifact;
        }
        Ok(())
    }
    pub fn dispatches(&self) -> u64 {
        self.dispatches
    }
    pub fn binding_source_id(
        &self,
        module: &str,
        evaluation_id: &SourceId,
    ) -> Option<EditSourceId> {
        self.entries
            .get(module)
            .filter(|e| {
                &e.evaluation_id == evaluation_id
                    && e.document.evaluation_source_id() == evaluation_id
            })
            .map(|e| e.binding_id)
    }
    pub fn capture(
        &self,
        root: &str,
        operation: &str,
    ) -> Result<RegisteredClosureBinding, ClosureInspectionError> {
        self.capture_with_rejection(root, operation)
            .map_err(RegisteredCaptureRejection::into_compatibility_error)
    }

    /// Structured capture diagnostics. No authorization is requested here and
    /// no discovery failure is converted to a generic host-error string.
    /// Presentation coordination: registered TUI may render `Diagnostics` directly;
    /// it must not rediscover sources to format a failure.
    pub fn capture_with_rejection(
        &self,
        root: &str,
        operation: &str,
    ) -> Result<RegisteredClosureBinding, RegisteredCaptureRejection> {
        self.capture_call(root, operation, json!({}), None)
    }

    /// Programmatic call-v1 entry point. No authority is granted by capture.
    pub fn capture_invocation(
        &self,
        root: &str,
        operation: &str,
        arguments: Value,
        contract: InvocationContract,
    ) -> Result<RegisteredClosureBinding, RegisteredCaptureRejection> {
        contract.validate()?;
        self.capture_call(root, operation, arguments, Some(contract))
    }

    fn capture_call(
        &self,
        root: &str,
        operation: &str,
        arguments: Value,
        contract: Option<InvocationContract>,
    ) -> Result<RegisteredClosureBinding, RegisteredCaptureRejection> {
        let mut arguments = crate::closure_inspection::IterativeJson(arguments);
        // Bound copying before constructing Sprout inputs. Registry Documents
        // remain host-owned; discovery admission does not bound their storage.
        let total = self
            .entries
            .iter()
            .try_fold(0usize, |total, (module, entry)| {
                total
                    .checked_add(module.len())?
                    .checked_add(entry.document.text().len())?
                    .checked_add(entry.language.len())
            })
            .ok_or(ClosureInspectionError::LimitExceeded)?;
        if total > ClosureInspectionBinding::MAX_BYTES {
            return Err(ClosureInspectionError::LimitExceeded.into());
        }
        let mut ids = std::collections::BTreeSet::new();
        if self
            .catalog
            .iter()
            .any(|o| o.id.is_empty() || !ids.insert(&o.id))
        {
            return Err(ClosureInspectionError::InvalidCapture.into());
        }
        let operation = self
            .catalog
            .iter()
            .find(|o| o.id == operation)
            .ok_or(ClosureInspectionError::InvalidCapture)?;
        ClosureInspectionBinding::validate_arguments(operation, &arguments.0)?;
        let sources = self
            .entries
            .iter()
            .map(|(module, entry)| {
                // document_mut can replace a whole Document; reject the stale mapping
                // rather than silently equating its numeric DocumentId.
                if entry.document.evaluation_source_id() != &entry.evaluation_id {
                    return Err(ClosureInspectionError::SnapshotMismatch);
                }
                Ok(RegisteredSource {
                    identity: format!("registered-{}", entry.registration),
                    module: module.clone(),
                    revision: entry.document.version().to_string(),
                    text: entry.document.text().into(),
                    language: entry.language.clone(),
                    display: Some(entry.document.path().display().to_string()),
                    origin: Some("registered-document".into()),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let discovered = self
            .discovery
            .capture_with_diagnostics(root, sources.clone(), self.limits)
            .map_err(|failure| {
                let root = self
                    .entries
                    .get(root)
                    .map(|entry| entry.evaluation_id.clone());
                if failure.diagnostics.is_empty() {
                    RegisteredCaptureRejection::Discovery {
                        error: failure.error,
                        root,
                    }
                } else {
                    RegisteredCaptureRejection::Diagnostics {
                        error: failure.error,
                        root,
                        diagnostics: failure.diagnostics,
                    }
                }
            })?;
        if discovered.root() != root
            || !discovered.limits().no_greater_than(self.limits)
            || discovered
                .sources()
                .iter()
                .any(|source| !sources.contains(source))
        {
            return Err(ClosureInspectionError::InvalidCapture.into());
        }
        let id = |module: &str| {
            self.entries
                .get(module)
                .map(|e| e.binding_id)
                .ok_or(ClosureInspectionError::InvalidCapture)
        };
        let mut graph = Vec::new();
        for (module, deps) in discovered.graph() {
            for dep in deps {
                graph.push((id(module)?, id(dep)?));
            }
        }
        let retained = discovered
            .sources()
            .iter()
            .map(|source| {
                let entry = &self.entries[&source.module];
                SourceSnapshot {
                    source_id: entry.evaluation_id.clone(),
                    revision_id: RevisionId::from_version(entry.document.version()),
                    text: Arc::from(source.text.as_str()),
                }
            })
            .collect();
        let sources = discovered
            .sources()
            .iter()
            .map(|source| {
                let entry = &self.entries[&source.module];
                ClosureSource {
                    id: entry.binding_id,
                    text: source.text.clone(),
                    version: entry.document.version(),
                    language: source.language.clone(),
                    module: source.module.clone(),
                    origin: source.origin.clone().unwrap_or_default(),
                }
            })
            .collect();
        let limits = discovered.limits();
        let generic = ClosureInspectionBinding::capture_with_arguments(
            ClosureCapture {
                owner: self.owner.clone(),
                sources,
                root: id(root)?,
                graph,
                compilation_identity: discovered.compiler_identity().into(),
                compilation_options: json!({"exports": discovered.exports(), "catalog": self.catalog,
                "identity_version":discovered.identity_version(), "compiler_sources":discovered.compiler_sources(),
                "requested_limits": self.limits.value(), "invocation_contract": contract.as_ref().map(|c| json!({"schema_version":c.schema_version,"entry":c.entry,"output":c.output,"effect":c.effect}))}),
                compilation_limits: limits.value(),
                operation: operation.clone(),
                execution_policy: json!({"host":self.policy,"dispatch":self.provider.identity(), "maximum":self.max_dispatches,
                "prepared": self.artifact.descriptor().map(|d| json!({"abi":d.abi,"policy":d.execution_policy}))}),
                artifact_identity: match self.artifact.descriptor() {
                    Some(d) => format!(
                        "prepared-sha256:{}:selection-{}",
                        d.sha256, self.artifact_generation
                    ),
                    None => format!("trusted-handle-{}", self.artifact_generation),
                },
            },
            std::mem::take(&mut arguments.0),
        )?;
        Ok(RegisteredClosureBinding {
            generic,
            discovered,
            retained,
            artifact: self.artifact.clone(),
            freshness: Freshness {
                catalog: self.catalog.clone(),
                policy: self.policy.clone(),
                limits: self.limits,
                generation: self.artifact_generation,
                maximum: self.max_dispatches,
                provider: self.provider.identity().to_owned(),
            },
        })
    }
    /// Consumes a selection, never a reusable grant. Every invocation rechecks
    /// the exact complete closure and asks the shared host policy afresh.
    pub fn invoke(
        &mut self,
        binding: RegisteredClosureBinding,
        policy: &mut impl HostSemanticPolicy,
    ) -> Result<RegisteredClosureResult<P::Output>, ClosureRejection> {
        self.invoke_authorized(binding, policy, None)
    }

    /// Approve recurring implementation/schema/policy scope, excluding argument values.
    /// Each request still requires separate host policy authorization.
    pub fn approve_service(
        &self,
        binding: &RegisteredClosureBinding,
        identity: ServiceGrantIdentity,
        request_ref: String,
        expires_at: std::time::Instant,
        policy: &mut impl HostSemanticPolicy,
    ) -> Result<RegisteredServiceGrant, ClosureInspectionError> {
        self.validate_sealed(binding)?;
        if identity.id.is_empty()
            || identity.id.len() > 1024
            || identity.version == 0
            || request_ref.is_empty()
            || request_ref.len() > 1024
            || expires_at <= std::time::Instant::now()
        {
            return Err(ClosureInspectionError::InvalidCapture);
        }
        let review = binding.generic.review()?;
        if !authorize_host_request(
            policy,
            HostAuthorizationRequest::RecurringClosure {
                grant_id: &identity.id,
                grant_version: identity.version,
                binding: &binding.generic,
                review: &review,
                request_ref: &request_ref,
                expires_at,
            },
        ) {
            return Err(ClosureInspectionError::Denied);
        }
        self.validate_sealed(binding)?;
        if expires_at <= std::time::Instant::now() {
            return Err(ClosureInspectionError::Denied);
        }
        Ok(RegisteredServiceGrant {
            identity,
            binding: binding.generic.clone(),
            request_ref,
            expires_at,
            revoked: Arc::new(std::sync::Mutex::new(false)),
        })
    }

    /// Each consumed tick may have different validated arguments but requires its
    /// own trusted request reference and host authorization of the actual call.
    pub fn invoke_service(
        &mut self,
        binding: RegisteredClosureBinding,
        grant: &RegisteredServiceGrant,
        trusted_request_ref: &str,
        policy: &mut impl HostSemanticPolicy,
    ) -> Result<RegisteredClosureResult<P::Output>, ClosureRejection> {
        self.invoke_authorized(binding, policy, Some((grant, trusted_request_ref)))
    }

    fn invoke_authorized(
        &mut self,
        binding: RegisteredClosureBinding,
        policy: &mut impl HostSemanticPolicy,
        service: Option<(&RegisteredServiceGrant, &str)>,
    ) -> Result<RegisteredClosureResult<P::Output>, ClosureRejection> {
        let review = binding.generic.review();
        let mut host = RegisteredHost {
            registry: self,
            sealed: &binding,
            grant: service.map(|(grant, _)| grant),
        };
        let result = binding.generic.invocation().invoke(&mut host, |request| {
            let Ok(review) = &review else {
                return false;
            };
            let authorization = match service {
                Some((grant, request_ref)) => {
                    if request_ref.is_empty()
                        || request_ref.len() > 1024
                        || !grant.permits(request.binding)
                    {
                        return false;
                    }
                    HostAuthorizationRequest::ServiceClosure {
                        binding: request.binding,
                        review,
                        grant_id: &grant.identity.id,
                        grant_version: grant.identity.version,
                        request_ref,
                    }
                }
                None => HostAuthorizationRequest::Closure {
                    binding: request.binding,
                    review,
                },
            };
            authorize_host_request(policy, authorization)
        })?;
        Ok(RegisteredClosureResult {
            service: service.map(|(grant, request_ref)| ServiceInvocationEvidence {
                grant: grant.identity.clone(),
                request_ref: request_ref.into(),
            }),
            service_revoked: service.map(|(grant, _)| grant.revoked.clone()),
            service_expires_at: service.map(|(grant, _)| grant.expires_at),
            result,
            discovered: binding.discovered,
            retained: binding.retained,
            _artifact: binding.artifact,
        })
    }
}

/// Trusted synchronous adapter. Implementations must bound their output and work,
/// enforce the supplied effective policy, and never resolve replacement sources.
/// This interface grants no authority: only the registry calls it after policy.
pub trait TrustedClosureProvider {
    type Output;
    fn identity(&self) -> &str;
    fn dispatch(&mut self, request: ProviderRequest<'_>) -> Result<Self::Output, ProviderFailure>;
}

/// Read-only original sealed input, including compiler source-index mapping.
pub struct ProviderRequest<'a> {
    sealed: &'a RegisteredClosureBinding,
    dispatch_number: u64,
}
impl ProviderRequest<'_> {
    pub fn closure(&self) -> &DiscoveredClosure {
        &self.sealed.discovered
    }
    pub fn binding(&self) -> &ClosureInspectionBinding {
        &self.sealed.generic
    }
    pub fn arguments(&self) -> &Value {
        self.binding().arguments()
    }
    pub fn operation(&self) -> &SemanticOperationDescription {
        &self.binding().snapshot().operation
    }
    pub fn execution_policy(&self) -> &Value {
        &self.binding().snapshot().execution_policy
    }
    pub fn compiler_source(&self, index: usize) -> Option<&SourceSnapshot> {
        let identity = self.closure().compiler_sources().get(index)?;
        let index = self
            .closure()
            .sources()
            .iter()
            .position(|s| &s.identity == identity)?;
        self.sealed.retained.get(index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderFailure {
    Rejected,
    LimitExceeded,
    Failed,
}

pub struct InertProvider;
impl TrustedClosureProvider for InertProvider {
    type Output = u64;
    fn identity(&self) -> &str {
        "inert-counter"
    }
    fn dispatch(&mut self, request: ProviderRequest<'_>) -> Result<u64, ProviderFailure> {
        Ok(request.dispatch_number)
    }
}

/// A typed prepared implementation and its dispatch function are selected together
/// by the trusted host. No Any downcast or path-based reload is available here.
pub struct PreparedProvider<T, O> {
    prepared: Arc<T>,
    identity: String,
    dispatch: fn(&T, ProviderRequest<'_>) -> Result<O, ProviderFailure>,
}
impl<T, O> TrustedClosureProvider for PreparedProvider<T, O> {
    type Output = Result<O, ProviderFailure>;
    fn identity(&self) -> &str {
        &self.identity
    }
    fn dispatch(&mut self, request: ProviderRequest<'_>) -> Result<Self::Output, ProviderFailure> {
        Ok((self.dispatch)(&self.prepared, request))
    }
}
impl<T: Send + Sync + 'static, O> RegisteredDocuments<PreparedProvider<T, O>> {
    /// Pins this exact typed implementation in selections and historical results.
    /// The host must supply a bounded, synchronous dispatch function and an
    /// immutable prepared code/configuration identity. Internal runtime machinery
    /// may be mutable, but must not replace the approved code or effective policy.
    /// This is a trusted adapter contract, not a sandbox for arbitrary Rust code.
    /// There is deliberately no artifact-only setter on a prepared registry.
    pub fn with_prepared_provider(
        registry: RegisteredDocuments,
        descriptor: PreparedImplementationDescriptor,
        prepared: T,
        dispatch: fn(&T, ProviderRequest<'_>) -> Result<O, ProviderFailure>,
    ) -> Result<Self, ClosureInspectionError> {
        Self::try_with_prepared_provider(registry, descriptor, prepared, dispatch)
            .map_err(|(error, _)| error)
    }

    /// Ownership-preserving transfer. Invalid descriptors or exhausted artifact
    /// generations return the original registry, including every live Document.
    pub fn try_with_prepared_provider(
        mut registry: RegisteredDocuments,
        descriptor: PreparedImplementationDescriptor,
        prepared: T,
        dispatch: fn(&T, ProviderRequest<'_>) -> Result<O, ProviderFailure>,
    ) -> Result<Self, (ClosureInspectionError, RegisteredDocuments)> {
        let prepared = Arc::new(prepared);
        let identity = format!("prepared-provider:{}", descriptor.abi);
        let artifact = match TrustedLoadedArtifact::prepared(descriptor, prepared.clone()) {
            Ok(artifact) => artifact,
            Err(error) => return Err((error, registry)),
        };
        if let Err(error) = registry.set_artifact(artifact) {
            return Err((error, registry));
        }
        Ok(Self {
            provider: PreparedProvider {
                prepared,
                identity,
                dispatch,
            },
            discovery: registry.discovery,
            owner: registry.owner,
            entries: registry.entries,
            next_registration: registry.next_registration,
            artifact: registry.artifact,
            artifact_generation: registry.artifact_generation,
            policy: registry.policy,
            catalog: registry.catalog,
            limits: registry.limits,
            dispatches: registry.dispatches,
            max_dispatches: registry.max_dispatches,
        })
    }
}

struct Freshness {
    catalog: Vec<SemanticOperationDescription>,
    policy: Value,
    limits: Limits,
    generation: u64,
    maximum: u64,
    provider: String,
}
impl<P: TrustedClosureProvider> RegisteredDocuments<P> {
    /// Compare live registry state to the original graph, without discovery or
    /// constructing compiler inputs. Unrelated valid registrations are irrelevant.
    fn validate_sealed(
        &self,
        sealed: &RegisteredClosureBinding,
    ) -> Result<(), ClosureInspectionError> {
        let f = &sealed.freshness;
        if self.owner != sealed.generic.snapshot().owner
            || self.catalog != f.catalog
            || self.policy != f.policy
            || self.limits != f.limits
            || self.artifact_generation != f.generation
            || self.max_dispatches != f.maximum
            || self.provider.identity() != f.provider
            || !Arc::ptr_eq(&self.artifact.0, &sealed.artifact.0)
        {
            return Err(ClosureInspectionError::SnapshotMismatch);
        }
        // document_mut permits replacing a Document: even an unrelated broken
        // identity mapping violates the registry's uniqueness/admission invariant.
        if self
            .entries
            .values()
            .any(|e| e.document.evaluation_source_id() != &e.evaluation_id)
        {
            return Err(ClosureInspectionError::SnapshotMismatch);
        }
        for (source, retained) in sealed.discovered.sources().iter().zip(&sealed.retained) {
            let e = self
                .entries
                .get(&source.module)
                .ok_or(ClosureInspectionError::SnapshotMismatch)?;
            let generic = sealed
                .generic
                .snapshot()
                .sources
                .iter()
                .find(|s| s.module == source.module)
                .ok_or(ClosureInspectionError::SnapshotMismatch)?;
            if e.evaluation_id != retained.source_id
                || e.binding_id != generic.id
                || format!("registered-{}", e.registration) != source.identity
                || e.document.version() != generic.version
                || e.document.text() != source.text
                || e.language != source.language
            {
                return Err(ClosureInspectionError::SnapshotMismatch);
            }
        }
        Ok(())
    }
}
struct RegisteredHost<'a, P> {
    registry: &'a mut RegisteredDocuments<P>,
    sealed: &'a RegisteredClosureBinding,
    grant: Option<&'a RegisteredServiceGrant>,
}
impl<P: TrustedClosureProvider> ClosureInspectionHost for RegisteredHost<'_, P> {
    type Output = P::Output;
    fn recapture(&mut self) -> Result<ClosureInspectionBinding, ClosureInspectionError> {
        self.registry.validate_sealed(self.sealed)?;
        Ok(self.sealed.generic.clone())
    }
    fn dispatch(
        &mut self,
        binding: &ClosureInspectionBinding,
    ) -> Result<Self::Output, ClosureInspectionError> {
        if &self.recapture()? != binding {
            return Err(ClosureInspectionError::SnapshotMismatch);
        }
        if self.registry.dispatches >= self.registry.max_dispatches {
            return Err(ClosureInspectionError::LimitExceeded);
        }
        if self.grant.is_some_and(|grant| !grant.permits(binding)) {
            return Err(ClosureInspectionError::Denied);
        }
        self.registry.dispatches += 1;
        self.registry
            .provider
            .dispatch(ProviderRequest {
                sealed: self.sealed,
                dispatch_number: self.registry.dispatches,
            })
            .map_err(|e| match e {
                ProviderFailure::LimitExceeded => ClosureInspectionError::LimitExceeded,
                _ => ClosureInspectionError::Host("provider failure".into()),
            })
    }
}

/// Trusted host-assigned audit identity, not a bearer credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceGrantIdentity {
    pub id: String,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceInvocationEvidence {
    pub grant: ServiceGrantIdentity,
    pub request_ref: String,
}

/// Opt-in recurring scope excluding argument values. Not Clone; revocation is sticky.
/// Request references must come from the trusted host, never model/tool arguments.
pub struct RegisteredServiceGrant {
    identity: ServiceGrantIdentity,
    binding: ClosureInspectionBinding,
    request_ref: String,
    expires_at: std::time::Instant,
    revoked: Arc<std::sync::Mutex<bool>>,
}
impl RegisteredServiceGrant {
    pub fn identity(&self) -> &ServiceGrantIdentity {
        &self.identity
    }
    pub fn request_ref(&self) -> &str {
        &self.request_ref
    }
    pub fn expires_at(&self) -> std::time::Instant {
        self.expires_at
    }
    /// Serializes with commits derived from this service. On return, any already
    /// admitted commit has finished. Never call from inside a commit callback.
    /// Dispatch admission remains a separate check, not a guest-duration lock.
    pub fn revoke(&self) {
        if let Ok(mut revoked) = self.revoked.lock() {
            *revoked = true;
        }
    }
    fn permits(&self, binding: &ClosureInspectionBinding) -> bool {
        self.revoked.lock().is_ok_and(|revoked| !*revoked)
            && std::time::Instant::now() < self.expires_at
            && self.binding.snapshot() == binding.snapshot()
    }
}

/// Consumed selection; neither cloning nor replay is available.
/// ```compile_fail,E0382
/// use scope_ide::{registered_document_closure::{RegisteredDocuments, RegisteredClosureBinding}, tool::semantic::HostSemanticPolicy};
/// fn replay(registry: &mut RegisteredDocuments, selection: RegisteredClosureBinding, policy: &mut impl HostSemanticPolicy) {
///     let _ = registry.invoke(selection, policy);
///     let _ = registry.invoke(selection, policy);
/// }
/// ```
pub struct RegisteredClosureBinding {
    generic: ClosureInspectionBinding,
    discovered: DiscoveredClosure,
    retained: Vec<SourceSnapshot>,
    artifact: TrustedLoadedArtifact,
    freshness: Freshness,
}
impl RegisteredClosureBinding {
    pub fn binding(&self) -> &ClosureInspectionBinding {
        &self.generic
    }
    pub fn discovered(&self) -> &DiscoveredClosure {
        &self.discovered
    }
}
/// Passive exact historical sources and discovery results. Reading/projecting
/// this value never recaptures, asks policy, or increments the dispatch counter.
/// The binding/output cannot be replaced independently of discovery and sources.
///
/// ```compile_fail,E0616
/// use scope_ide::{
///     closure_inspection::ClosureInspectionResult,
///     registered_document_closure::RegisteredClosureResult,
/// };
/// fn replace(observation: &mut RegisteredClosureResult, replacement: ClosureInspectionResult<u64>) {
///     observation.result = replacement;
/// }
/// ```
pub struct RegisteredClosureResult<T = u64> {
    service: Option<ServiceInvocationEvidence>,
    service_revoked: Option<Arc<std::sync::Mutex<bool>>>,
    service_expires_at: Option<std::time::Instant>,
    result: ClosureInspectionResult<T>,
    discovered: DiscoveredClosure,
    retained: Vec<SourceSnapshot>,
    _artifact: TrustedLoadedArtifact,
}
impl<T> RegisteredClosureResult<T> {
    pub fn service(&self) -> Option<&ServiceInvocationEvidence> {
        self.service.as_ref()
    }
    pub fn result(&self) -> &ClosureInspectionResult<T> {
        &self.result
    }
    pub fn binding(&self) -> &ClosureInspectionBinding {
        &self.result.binding
    }
    pub fn output(&self) -> &T {
        &self.result.output
    }
    /// Resolve the original compiler index, never the order of generic sources.
    pub fn compiler_source(&self, index: usize) -> Option<&SourceSnapshot> {
        let identity = self.discovered.compiler_sources().get(index)?;
        let source_index = self
            .discovered
            .sources()
            .iter()
            .position(|s| &s.identity == identity)?;
        self.retained.get(source_index)
    }
    pub fn discovered(&self) -> &DiscoveredClosure {
        &self.discovered
    }
    pub fn review(
        &self,
    ) -> Result<crate::closure_inspection::ClosureReview, ClosureInspectionError> {
        self.result.binding.review()
    }
}
impl<T> SourceResolver for RegisteredClosureResult<T> {
    fn resolve(
        &self,
        source: &SourceId,
        revision: RevisionId,
    ) -> Result<SourceSnapshot, SourceUnavailable> {
        self.retained
            .iter()
            .find(|s| &s.source_id == source && s.revision_id == revision)
            .cloned()
            .ok_or(SourceUnavailable::UnknownOrExpired)
    }
}

/// Explicit host commit review. Neither execution approval nor an effect string
/// authorizes this request. The host must validate the checked provider output,
/// its exact proposal, principal, resource scope, and trusted request correlation.
pub struct CommitAuthorizationRequest<'a, T, P> {
    pub invocation: &'a RegisteredClosureResult<T>,
    pub proposal: &'a P,
    pub identity: &'a ServiceGrantIdentity,
    pub request_ref: &'a str,
    pub expires_at: std::time::Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitDenied {
    NotDomainProposal,
    InvalidRequest,
    Denied,
    Revoked,
    Expired,
    Consumed,
    Poisoned,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommitError<E> {
    Authority(CommitDenied),
    Host(E),
}

/// One exact host-validated proposal, not a reusable service write capability.
/// Share with Arc if needed. Failed host attempts also consume the authority.
/// Revocation and commit serialize on this grant and its parent service lock.
/// Expiry is checked at commit admission; an admitted transaction may finish later.
pub struct RegisteredCommitGrant<P> {
    proposal: P,
    binding: ClosureInspectionBinding,
    service: Option<ServiceInvocationEvidence>,
    service_revoked: Option<Arc<std::sync::Mutex<bool>>>,
    service_expires_at: Option<std::time::Instant>,
    identity: ServiceGrantIdentity,
    request_ref: String,
    expires_at: std::time::Instant,
    state: std::sync::Mutex<CommitState>,
}
#[derive(Default)]
struct CommitState {
    revoked: bool,
    consumed: bool,
}
impl<T> RegisteredClosureResult<T> {
    /// Consumes invocation evidence, preventing minting multiple grants from it.
    /// The separate, explicit callback must opt in to commit (legacy execution
    /// policies are deliberately not consulted). Provider success alone does not
    /// establish a checked, accepted domain proposal, especially for nested Results.
    ///
    /// ```compile_fail,E0382
    /// use scope_ide::registered_document_closure::{RegisteredClosureResult, ServiceGrantIdentity};
    /// fn duplicate(result: RegisteredClosureResult) {
    ///     let expiry = std::time::Instant::now() + std::time::Duration::from_secs(60);
    ///     let id = || ServiceGrantIdentity { id: "commit".into(), version: 1 };
    ///     let _ = result.authorize_commit(1, id(), "request".into(), expiry, |_| true);
    ///     let _ = result.authorize_commit(1, id(), "request".into(), expiry, |_| true);
    /// }
    /// ```
    pub fn authorize_commit<P>(
        self,
        proposal: P,
        identity: ServiceGrantIdentity,
        request_ref: String,
        expires_at: std::time::Instant,
        authorize: impl FnOnce(CommitAuthorizationRequest<'_, T, P>) -> bool,
    ) -> Result<RegisteredCommitGrant<P>, CommitDenied> {
        if self.binding().snapshot().operation.effect
            != scope_language::SemanticOperationEffect::ProposeDomain
        {
            return Err(CommitDenied::NotDomainProposal);
        }
        if identity.id.is_empty()
            || identity.id.len() > 1024
            || identity.version == 0
            || request_ref.is_empty()
            || request_ref.len() > 1024
        {
            return Err(CommitDenied::InvalidRequest);
        }
        if std::time::Instant::now() >= expires_at {
            return Err(CommitDenied::Expired);
        }
        if !authorize(CommitAuthorizationRequest {
            invocation: &self,
            proposal: &proposal,
            identity: &identity,
            request_ref: &request_ref,
            expires_at,
        }) {
            return Err(CommitDenied::Denied);
        }
        let grant = RegisteredCommitGrant {
            proposal,
            binding: self.binding().clone(),
            service: self.service,
            service_revoked: self.service_revoked,
            service_expires_at: self.service_expires_at,
            identity,
            request_ref,
            expires_at,
            state: std::sync::Mutex::new(CommitState::default()),
        };
        grant.check_parent()?;
        if std::time::Instant::now() >= expires_at {
            return Err(CommitDenied::Expired);
        }
        Ok(grant)
    }
}
impl<P> RegisteredCommitGrant<P> {
    pub fn identity(&self) -> &ServiceGrantIdentity {
        &self.identity
    }
    pub fn request_ref(&self) -> &str {
        &self.request_ref
    }
    pub fn binding(&self) -> &ClosureInspectionBinding {
        &self.binding
    }
    pub fn service(&self) -> Option<&ServiceInvocationEvidence> {
        self.service.as_ref()
    }
    pub fn expires_at(&self) -> std::time::Instant {
        self.expires_at
    }

    /// On return, any already admitted commit has finished. Poisoning fails closed.
    /// Do not call revoke (or parent service revoke) from a commit callback.
    pub fn revoke(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.revoked = true;
        }
    }
    fn check_parent(&self) -> Result<(), CommitDenied> {
        if let Some(revoked) = &self.service_revoked {
            if *revoked.lock().map_err(|_| CommitDenied::Poisoned)? {
                return Err(CommitDenied::Revoked);
            }
        }
        if self
            .service_expires_at
            .is_some_and(|expiry| std::time::Instant::now() >= expiry)
        {
            return Err(CommitDenied::Expired);
        }
        Ok(())
    }
    /// Host transaction boundary. While authority locks are held, the callback
    /// must recheck current principal/resource authorization, revision and narrow
    /// preconditions, then atomically persist the proposal or change nothing.
    /// Scope cannot roll back arbitrary host writes. Do not perform guest execution,
    /// async work, or reentrant grant operations here; keep the transaction short.
    /// All writers must obey the host repository's own transaction/CAS protocol.
    pub fn commit<O, E>(
        &self,
        transaction: impl FnOnce(&P) -> Result<O, E>,
    ) -> Result<O, CommitError<E>> {
        let parent = self
            .service_revoked
            .as_ref()
            .map(|revoked| revoked.lock())
            .transpose()
            .map_err(|_| CommitError::Authority(CommitDenied::Poisoned))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| CommitError::Authority(CommitDenied::Poisoned))?;
        let denied = if state.revoked || parent.as_ref().is_some_and(|revoked| **revoked) {
            Some(CommitDenied::Revoked)
        } else if state.consumed {
            Some(CommitDenied::Consumed)
        } else if std::time::Instant::now() >= self.expires_at
            || self
                .service_expires_at
                .is_some_and(|expiry| std::time::Instant::now() >= expiry)
        {
            Some(CommitDenied::Expired)
        } else {
            None
        };
        if let Some(reason) = denied {
            return Err(CommitError::Authority(reason));
        }
        state.consumed = true;
        let result = transaction(&self.proposal).map_err(CommitError::Host);
        // Explicit drops document that neither revocation lock ends before write.
        drop(state);
        drop(parent);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::DocumentId,
        text::{Position, TextEdit},
        tool::semantic::SemanticAuthorization,
    };
    use scope_language::{SemanticOperationEffect, SemanticTargetRequirement};

    struct DomainPolicy;
    impl HostSemanticPolicy for DomainPolicy {
        fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
            let binding = match request {
                HostAuthorizationRequest::Closure { binding, .. }
                | HostAuthorizationRequest::RecurringClosure { binding, .. }
                | HostAuthorizationRequest::ServiceClosure { binding, .. } => binding,
                _ => return false,
            };
            binding.snapshot().operation.effect == SemanticOperationEffect::ProposeDomain
        }
    }
    fn domain_fixture() -> RegisteredDocuments {
        let mut registry = parameterized_fixture();
        registry.catalog[0].effect = SemanticOperationEffect::ProposeDomain;
        registry
    }
    fn domain_capture(registry: &RegisteredDocuments, quest: &str) -> RegisteredClosureBinding {
        let mut contract = call_contract();
        contract.effect = "domain-proposal-v1".into();
        registry
            .capture_invocation("root", "inspect", json!({"quest":quest}), contract)
            .unwrap()
    }
    fn future() -> std::time::Instant {
        std::time::Instant::now() + std::time::Duration::from_secs(60)
    }
    fn domain_result() -> RegisteredClosureResult {
        let mut registry = domain_fixture();
        let selected = domain_capture(&registry, "q1");
        registry.invoke(selected, &mut DomainPolicy).unwrap()
    }
    fn commit_grant() -> RegisteredCommitGrant<u64> {
        domain_result()
            .authorize_commit(1, grant_identity(), "commit-1".into(), future(), |r| {
                assert_eq!(r.invocation.binding().arguments(), &json!({"quest":"q1"}));
                assert_eq!(*r.invocation.output(), 1);
                assert_eq!(*r.proposal, 1);
                assert_eq!(r.request_ref, "commit-1");
                true
            })
            .unwrap()
    }
    fn domain_service() -> (RegisteredServiceGrant, RegisteredClosureResult) {
        let mut registry = domain_fixture();
        let selection = domain_capture(&registry, "q1");
        let service = registry
            .approve_service(
                &selection,
                grant_identity(),
                "approval".into(),
                future(),
                &mut DomainPolicy,
            )
            .unwrap();
        let result = registry
            .invoke_service(selection, &service, "request-1", &mut DomainPolicy)
            .unwrap();
        (service, result)
    }

    #[test]
    fn domain_proposal_admission_preserves_source_edit_and_selection_rejection() {
        let mut registry = domain_fixture();
        let selected = domain_capture(&registry, "q1");
        assert!(registry.invoke(selected, &mut DomainPolicy).is_ok());
        registry.catalog[0].effect = SemanticOperationEffect::ProposeChanges;
        assert!(
            registry
                .capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
                .is_err()
        );
        registry.catalog[0].effect = SemanticOperationEffect::ProposeDomain;
        registry.catalog[0].target = SemanticTargetRequirement::Selection;
        assert!(
            registry
                .capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
                .is_err()
        );
    }
    #[test]
    fn domain_service_requires_explicit_approval_and_each_exact_request() {
        let mut registry = domain_fixture();
        let selection = domain_capture(&registry, "q1");
        let mut legacy = |_: SemanticAuthorization<'_>| true;
        assert!(
            registry
                .approve_service(
                    &selection,
                    grant_identity(),
                    "approval".into(),
                    future(),
                    &mut legacy
                )
                .is_err()
        );
        let service = registry
            .approve_service(
                &selection,
                grant_identity(),
                "approval".into(),
                future(),
                &mut DomainPolicy,
            )
            .unwrap();
        assert!(
            registry
                .invoke_service(selection, &service, "request-1", &mut legacy)
                .is_err()
        );
        for quest in ["q1", "q2"] {
            let selected = domain_capture(&registry, quest);
            let result = registry
                .invoke_service(selected, &service, quest, &mut DomainPolicy)
                .unwrap();
            assert_eq!(result.binding().arguments(), &json!({"quest":quest}));
            assert_eq!(result.service().unwrap().request_ref, quest);
        }
        let selected = domain_capture(&registry, "q1");
        service.revoke();
        assert!(
            registry
                .invoke_service(selected, &service, "request-3", &mut DomainPolicy)
                .is_err()
        );
        assert_eq!(registry.dispatches(), 2);
    }
    #[test]
    fn commit_authority_is_not_implied_by_read_or_proposal_execution() {
        let mut registry = parameterized_fixture();
        let selected = registry
            .capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
            .unwrap();
        let read = registry.invoke(selected, &mut CallPolicy).unwrap();
        assert!(matches!(
            read.authorize_commit(1, grant_identity(), "c".into(), future(), |_| panic!(
                "read must not request commit"
            )),
            Err(CommitDenied::NotDomainProposal)
        ));
        assert!(matches!(
            domain_result().authorize_commit(1, grant_identity(), "c".into(), future(), |_| false),
            Err(CommitDenied::Denied)
        ));
        assert!(matches!(
            domain_result().authorize_commit(1, grant_identity(), "".into(), future(), |_| panic!(
                "invalid"
            )),
            Err(CommitDenied::InvalidRequest)
        ));
        assert!(matches!(
            domain_result().authorize_commit(
                1,
                grant_identity(),
                "c".into(),
                std::time::Instant::now(),
                |_| panic!("expired")
            ),
            Err(CommitDenied::Expired)
        ));
    }
    #[test]
    fn commit_is_single_attempt_including_host_failure() {
        let grant = commit_grant();
        assert_eq!(grant.commit(|p| Ok::<_, ()>(*p + 1)), Ok(2));
        assert_eq!(
            grant.commit(|_| -> Result<(), ()> { panic!("replay") }),
            Err(CommitError::Authority(CommitDenied::Consumed))
        );
        let grant = commit_grant();
        assert_eq!(
            grant.commit(|_| Err::<(), _>("conflict")),
            Err(CommitError::Host("conflict"))
        );
        assert_eq!(
            grant.commit(|_| Ok::<_, ()>(())),
            Err(CommitError::Authority(CommitDenied::Consumed))
        );
    }
    #[test]
    fn commit_rechecks_own_and_parent_revocation_and_expiry() {
        let grant = commit_grant();
        grant.revoke();
        assert_eq!(
            grant.commit(|_| Ok::<_, ()>(())),
            Err(CommitError::Authority(CommitDenied::Revoked))
        );
        let mut grant = commit_grant();
        grant.expires_at = std::time::Instant::now();
        assert_eq!(
            grant.commit(|_| Ok::<_, ()>(())),
            Err(CommitError::Authority(CommitDenied::Expired))
        );
        let (service, result) = domain_service();
        let grant = result
            .authorize_commit(1, grant_identity(), "c".into(), future(), |_| true)
            .unwrap();
        service.revoke();
        assert_eq!(
            grant.commit(|_| Ok::<_, ()>(())),
            Err(CommitError::Authority(CommitDenied::Revoked))
        );
        let (_, result) = domain_service();
        let mut grant = result
            .authorize_commit(1, grant_identity(), "c".into(), future(), |_| true)
            .unwrap();
        grant.service_expires_at = Some(std::time::Instant::now());
        assert_eq!(
            grant.commit(|_| Ok::<_, ()>(())),
            Err(CommitError::Authority(CommitDenied::Expired))
        );
        let (service, result) = domain_service();
        service.revoke();
        assert!(matches!(
            result.authorize_commit(1, grant_identity(), "c".into(), future(), |_| true),
            Err(CommitDenied::Revoked)
        ));
    }
    #[test]
    fn commit_authorization_rechecks_parent_after_callback() {
        let (service, result) = domain_service();
        assert!(matches!(
            result.authorize_commit(1, grant_identity(), "c".into(), future(), |_| {
                service.revoke();
                true
            }),
            Err(CommitDenied::Revoked)
        ));
    }
    #[test]
    fn commit_and_revocation_have_a_shared_linearization_boundary() {
        use std::sync::mpsc::channel;
        // Exercise both independent commit revocation and parent service revocation.
        for parent in [false, true] {
            let (service, result) = domain_service();
            let grant = result
                .authorize_commit(1, grant_identity(), "c".into(), future(), |_| true)
                .unwrap();
            let (entered_tx, entered_rx) = channel();
            let (finish_tx, finish_rx) = channel();
            let (revoking_tx, revoking_rx) = channel();
            let (revoked_tx, revoked_rx) = channel();
            std::thread::scope(|scope| {
                let g = &grant;
                let commit = scope.spawn(move || {
                    g.commit(|_| {
                        entered_tx.send(()).unwrap();
                        finish_rx.recv().unwrap();
                        Ok::<_, ()>("written")
                    })
                });
                entered_rx.recv().unwrap();
                // Deterministically prove that both authority locks cover the callback.
                assert!(grant.state.try_lock().is_err());
                assert!(service.revoked.try_lock().is_err());
                let g = &grant;
                let s = &service;
                let revoke = scope.spawn(move || {
                    revoking_tx.send(()).unwrap();
                    if parent {
                        s.revoke();
                    } else {
                        g.revoke();
                    }
                    revoked_tx.send(()).unwrap();
                });
                revoking_rx.recv().unwrap();
                assert!(revoked_rx.try_recv().is_err());
                finish_tx.send(()).unwrap();
                assert_eq!(commit.join().unwrap(), Ok("written"));
                revoke.join().unwrap();
                revoked_rx.recv().unwrap();
            });
            assert_eq!(
                grant.commit(|_| Ok::<_, ()>(())),
                Err(CommitError::Authority(CommitDenied::Revoked))
            );
        }
    }
    #[test]
    fn concurrent_proposals_require_host_cas_and_current_authorization() {
        let board = std::sync::Mutex::new((1_u64, false));
        let first = commit_grant();
        let second = commit_grant();
        let apply = |expected: &u64| {
            let mut state = board.lock().unwrap();
            if state.0 != *expected || state.1 {
                return Err("conflict");
            }
            let next = state.0.checked_add(1).ok_or("overflow")?;
            *state = (next, true);
            Ok(())
        };
        std::thread::scope(|scope| {
            let a = scope.spawn(|| first.commit(apply));
            let b = scope.spawn(|| second.commit(apply));
            let results = [a.join().unwrap(), b.join().unwrap()];
            assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
            assert_eq!(
                results
                    .iter()
                    .filter(|r| **r == Err(CommitError::Host("conflict")))
                    .count(),
                1
            );
        });
        assert_eq!(*board.lock().unwrap(), (2, true));
        assert_eq!(
            commit_grant().commit(|_| Err::<(), _>("principal revoked")),
            Err(CommitError::Host("principal revoked"))
        );
    }
    #[test]
    fn panicking_transaction_poisons_authority_instead_of_replaying() {
        let grant = commit_grant();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<(), CommitError<()>> = grant.commit(|_| panic!("host failure"));
        }));
        assert_eq!(
            grant.commit(|_| Ok::<_, ()>(())),
            Err(CommitError::Authority(CommitDenied::Poisoned))
        );
    }

    fn operation() -> SemanticOperationDescription {
        SemanticOperationDescription {
            id: "inspect".into(),
            description: "Inert closure inspection".into(),
            arguments: vec![],
            effect: SemanticOperationEffect::Read,
            target: SemanticTargetRequirement::Document,
            constraints: None,
        }
    }
    // Unit support only: fixed graph fixture, not a parser or production facade.
    struct StaticDiscovery;
    impl Discovery for StaticDiscovery {
        fn capture(
            &self,
            root: &str,
            sources: Vec<RegisteredSource>,
            limits: Limits,
        ) -> Result<DiscoveredClosure, DiscoveryError> {
            if sources.iter().any(|source| source.language != "sprout") {
                return Err(DiscoveryError {
                    phase: DiscoveryPhase::Registration,
                    category: DiscoveryCategory::Language,
                });
            }
            if root != "root"
                || !["root", "dep"]
                    .iter()
                    .all(|module| sources.iter().any(|source| &source.module == module))
            {
                return Err(DiscoveryError {
                    phase: DiscoveryPhase::Acquisition,
                    category: DiscoveryCategory::MissingSource,
                });
            }
            let sources: Vec<_> = sources
                .into_iter()
                .filter(|s| s.module == "root" || s.module == "dep")
                .collect();
            let mut portable = Vec::new();
            for source in &sources {
                for value in [
                    &source.identity,
                    &source.module,
                    &source.revision,
                    &source.text,
                ] {
                    portable.extend_from_slice(&value.len().to_le_bytes());
                    portable.extend_from_slice(value.as_bytes());
                }
            }
            let compiler_sources = sources.iter().rev().map(|s| s.identity.clone()).collect();
            DiscoveredClosure::new(DiscoveryEvidence {
                root: root.into(),
                sources,
                graph: vec![("dep".into(), vec![]), ("root".into(), vec!["dep".into()])],
                exports: vec![
                    ("dep".into(), vec!["value".into()]),
                    ("root".into(), vec![]),
                ],
                compiler_sources,
                compiler_identity: "static-unit-fixture-v1".into(),
                identity_version: 1,
                portable_identity: portable,
                limits,
            })
        }
    }
    fn evidence() -> DiscoveryEvidence {
        let sources = ["dep", "root"]
            .into_iter()
            .map(|module| RegisteredSource {
                identity: module.into(),
                module: module.into(),
                revision: "0".into(),
                text: "fixture".into(),
                language: "sprout".into(),
                display: None,
                origin: None,
            })
            .collect();
        StaticDiscovery
            .capture("root", sources, Limits::default())
            .unwrap()
            .0
    }

    #[test]
    fn discovery_dto_bounds_and_source_table_admission() {
        for mutation in 0..6 {
            let mut evidence = evidence();
            match mutation {
                0 => evidence.compiler_sources[0] = evidence.compiler_sources[1].clone(),
                1 => evidence.graph[0].1.push("unregistered".into()),
                2 => evidence.sources[0].identity = evidence.sources[1].identity.clone(),
                3 => evidence.exports.pop().map(|_| ()).unwrap(),
                4 => evidence.identity_version = 0,
                _ => evidence.portable_identity = vec![0; DiscoveredClosure::MAX_BYTES + 1],
            }
            assert!(
                DiscoveredClosure::new(evidence).is_err(),
                "mutation {mutation}"
            );
        }
    }

    struct ChangedDiscovery(std::rc::Rc<std::cell::Cell<usize>>);
    impl Discovery for ChangedDiscovery {
        fn capture(
            &self,
            root: &str,
            sources: Vec<RegisteredSource>,
            limits: Limits,
        ) -> Result<DiscoveredClosure, DiscoveryError> {
            let mut evidence = StaticDiscovery.capture(root, sources, limits)?.0;
            match self.0.get() {
                1 => evidence.compiler_sources.reverse(),
                2 => evidence.compiler_identity.push_str("-changed"),
                3 => evidence.identity_version += 1,
                4 => evidence.portable_identity.push(1),
                5 => evidence.exports[0].1.push("other_export".into()),
                6 => evidence.sources[0].text.push_str(" changed"),
                7 => evidence.limits.steps += 1,
                _ => {}
            }
            DiscoveredClosure::new(evidence)
        }
    }
    #[test]
    fn compiler_evidence_is_sealed_not_rediscovered_and_fabricated_sources_reject() {
        for mutation in 1..8 {
            let mut registry = fixture();
            let state = std::rc::Rc::new(std::cell::Cell::new(0));
            registry.discovery = Box::new(ChangedDiscovery(state.clone()));
            let selected = registry.capture("root", "inspect").unwrap();
            let identity = selected.discovered.portable_identity().to_vec();
            state.set(mutation);
            let mut policy = Policy {
                calls: 0,
                allow: true,
            };
            let result = registry.invoke(selected, &mut policy).unwrap();
            assert_eq!(result.discovered().portable_identity(), identity);
            assert_eq!(policy.calls, 1);
            if mutation == 6 {
                assert!(registry.capture("root", "inspect").is_err());
            }
        }
    }
    #[test]
    fn safe_discovery_errors_preserve_phase_and_category_without_source() {
        let mut registry = fixture();
        registry.remove("dep");
        let error = registry.capture("root", "inspect").err().unwrap();
        assert_eq!(
            error,
            ClosureInspectionError::Host("discovery Acquisition/MissingSource".into())
        );
        let root = registry
            .document("root")
            .unwrap()
            .evaluation_source_id()
            .clone();
        assert_eq!(
            registry
                .capture_with_rejection("root", "inspect")
                .err()
                .unwrap(),
            RegisteredCaptureRejection::Discovery {
                error: DiscoveryError {
                    phase: DiscoveryPhase::Acquisition,
                    category: DiscoveryCategory::MissingSource,
                },
                root: Some(root.clone())
            }
        );

        struct CompileFailure;
        impl Discovery for CompileFailure {
            fn capture(
                &self,
                _: &str,
                _: Vec<RegisteredSource>,
                _: Limits,
            ) -> Result<DiscoveredClosure, DiscoveryError> {
                Err(DiscoveryError {
                    phase: DiscoveryPhase::Compilation,
                    category: DiscoveryCategory::Compilation,
                })
            }
        }
        registry.discovery = Box::new(CompileFailure);
        assert_eq!(
            registry
                .capture_with_rejection("root", "inspect")
                .err()
                .unwrap(),
            RegisteredCaptureRejection::Discovery {
                error: DiscoveryError {
                    phase: DiscoveryPhase::Compilation,
                    category: DiscoveryCategory::Compilation,
                },
                root: Some(root)
            }
        );
        let unknown = registry
            .capture_with_rejection("sensitive-unknown-module", "inspect")
            .err()
            .unwrap();
        assert_eq!(
            unknown,
            RegisteredCaptureRejection::Discovery {
                error: DiscoveryError {
                    phase: DiscoveryPhase::Compilation,
                    category: DiscoveryCategory::Compilation,
                },
                root: None
            }
        );
        assert!(!format!("{unknown:?}").contains("sensitive-unknown-module"));
        registry.set_catalog(vec![]);
        assert_eq!(
            registry
                .capture_with_rejection("root", "inspect")
                .err()
                .unwrap(),
            RegisteredCaptureRejection::Inspection(ClosureInspectionError::InvalidCapture)
        );
    }

    #[test]
    fn registered_language_is_forwarded_and_mismatch_or_relabeling_rejects() {
        let mut registry = fixture();
        let dep = registry.remove("dep").unwrap();
        registry
            .register_with_language("dep".into(), dep, "not-sprout".into())
            .unwrap();
        let root = registry
            .document("root")
            .unwrap()
            .evaluation_source_id()
            .clone();
        assert_eq!(
            registry
                .capture_with_rejection("root", "inspect")
                .err()
                .unwrap(),
            RegisteredCaptureRejection::Discovery {
                error: DiscoveryError {
                    phase: DiscoveryPhase::Registration,
                    category: DiscoveryCategory::Language,
                },
                root: Some(root)
            }
        );
        struct RelabelDiscovery;
        impl Discovery for RelabelDiscovery {
            fn capture(
                &self,
                root: &str,
                mut sources: Vec<RegisteredSource>,
                limits: Limits,
            ) -> Result<DiscoveredClosure, DiscoveryError> {
                assert!(sources.iter().any(|source| source.language == "not-sprout"));
                for source in &mut sources {
                    source.language = "sprout".into();
                }
                StaticDiscovery.capture(root, sources, limits)
            }
        }
        registry.discovery = Box::new(RelabelDiscovery);
        assert_eq!(
            registry
                .capture_with_rejection("root", "inspect")
                .err()
                .unwrap(),
            RegisteredCaptureRejection::Inspection(ClosureInspectionError::InvalidCapture)
        );
        assert_eq!(registry.dispatches(), 0);
        // The compatibility shorthand explicitly restores the caller's Sprout assertion.
        let dep = registry.remove("dep").unwrap();
        registry.register("dep".into(), dep).unwrap();
        registry.discovery = Box::new(StaticDiscovery);
        let fresh = registry.capture_with_rejection("root", "inspect").unwrap();
        assert!(
            fresh
                .discovered()
                .sources()
                .iter()
                .all(|source| source.language == "sprout")
        );
    }

    fn fixture() -> RegisteredDocuments {
        let mut registry = RegisteredDocuments::new(
            StaticDiscovery,
            TrustedLoadedArtifact::default(),
            vec![operation()],
            json!({"allow_counter":true}),
            Limits::default(),
            2,
        );
        registry
            .register(
                "root".into(),
                Document::new(
                    DocumentId::new(7),
                    "root.sprout",
                    "import dep; fn main() -> Int32 { dep.value() / 0 }",
                ),
            )
            .unwrap();
        // Deliberately overlapping DocumentIds: they must not alias.
        registry
            .register(
                "dep".into(),
                Document::new(
                    DocumentId::new(7),
                    "dep.sprout",
                    "pub fn value() -> Int32 { 42 }",
                ),
            )
            .unwrap();
        registry
    }
    #[test]
    fn app_owned_documents_capture_live_edits_and_preserve_sealed_freshness() {
        use crate::{app::App, test_support::TempWorkspace};

        let root = TempWorkspace::new("app_registry_capture_freshness");
        root.write_file(
            "root.sprout",
            "import dep; fn main() -> Int32 { dep.value() / 0 }",
        );
        root.write_file("dep.sprout", "pub fn value() -> Int32 { 42 }");
        root.write_file("other.sprout", "unrelated");
        let mut app = App::new(root.workspace()).unwrap();
        assert!(
            app.enable_registered_session(RegisteredDocuments::new(
                StaticDiscovery,
                TrustedLoadedArtifact::default(),
                vec![operation()],
                json!({"allow_counter": true}),
                Limits::default(),
                2,
            ))
            .is_ok()
        );
        for module in ["root", "dep", "other"] {
            app.open_path(format!("{module}.sprout")).unwrap();
            app.enroll_open_document(module.into(), "sprout".into())
                .unwrap();
        }
        let original = app
            .registered_documents()
            .unwrap()
            .capture("root", "inspect")
            .unwrap();
        app.select_registered_document("dep").unwrap();
        let identity = app.open_document().unwrap().evaluation_source_id().clone();
        app.open_document_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), " "))
            .unwrap();
        let edited_text = app.open_document().unwrap().text().to_owned();
        let version = app.open_document().unwrap().version();
        assert!(app.open_document().unwrap().is_dirty());
        assert_ne!(root.read_file("dep.sprout"), edited_text);
        let registry = app.registered_documents().unwrap();
        assert!(matches!(
            registry.validate_sealed(&original),
            Err(ClosureInspectionError::SnapshotMismatch)
        ));
        let fresh = registry.capture("root", "inspect").unwrap();
        let source = fresh
            .discovered()
            .sources()
            .iter()
            .find(|source| source.module == "dep")
            .unwrap();
        assert_eq!(source.text, edited_text);
        assert_eq!(source.revision, version.to_string());
        let retained = fresh
            .retained
            .iter()
            .find(|source| source.source_id == identity)
            .unwrap();
        assert_eq!(&*retained.text, edited_text);
        assert_eq!(retained.revision_id, RevisionId::from_version(version));
        assert!(registry.binding_source_id("dep", &identity).is_some());

        app.select_registered_document("other").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), " "))
            .unwrap();
        assert!(
            app.registered_documents()
                .unwrap()
                .validate_sealed(&fresh)
                .is_ok()
        );

        // Saving retains identity/version, but closing and reopening the same bytes does not.
        app.select_registered_document("dep").unwrap();
        app.save_open_document().unwrap();
        assert!(
            app.registered_documents()
                .unwrap()
                .validate_sealed(&fresh)
                .is_ok()
        );
        app.close_open_document().unwrap();
        assert!(
            app.registered_documents()
                .unwrap()
                .validate_sealed(&fresh)
                .is_err()
        );
        app.open_path("dep.sprout").unwrap();
        assert_eq!(app.open_document().unwrap().text(), edited_text);
        assert_ne!(
            app.open_document().unwrap().evaluation_source_id(),
            &identity
        );
        app.enroll_open_document("dep".into(), "sprout".into())
            .unwrap();
        assert!(
            app.registered_documents()
                .unwrap()
                .validate_sealed(&fresh)
                .is_err()
        );
        let reopened = app
            .registered_documents()
            .unwrap()
            .capture("root", "inspect")
            .unwrap();
        assert!(
            app.registered_documents()
                .unwrap()
                .validate_sealed(&reopened)
                .is_ok()
        );

        // Mutable access must not let equal text/numeric IDs impersonate an enrolled source.
        app.select_registered_document("other").unwrap();
        let document = app.open_document().unwrap();
        let replacement = Document::new(document.id(), document.path().clone(), document.text());
        *app.open_document_mut().unwrap() = replacement;
        let registry = app.registered_documents().unwrap();
        assert!(matches!(
            registry.validate_sealed(&reopened),
            Err(ClosureInspectionError::SnapshotMismatch)
        ));
        assert!(matches!(
            registry.capture("root", "inspect"),
            Err(ClosureInspectionError::SnapshotMismatch)
        ));
    }

    struct Policy {
        calls: usize,
        allow: bool,
    }
    impl HostSemanticPolicy for Policy {
        fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
            self.calls += 1;
            let HostAuthorizationRequest::Closure { binding, review } = request else {
                panic!("wrong authority domain")
            };
            assert_eq!(review.as_str(), binding.review().unwrap().as_str());
            assert_eq!(binding.snapshot().sources.len(), 2);
            assert_eq!(binding.snapshot().operation, operation());
            self.allow && binding.snapshot().execution_policy["host"]["allow_counter"] == true
        }
    }
    #[test]
    fn real_documents_explicit_mapping_passive_history_denial_recovery_and_no_rerun() {
        struct CountDiscovery(std::rc::Rc<std::cell::Cell<usize>>);
        impl Discovery for CountDiscovery {
            fn capture(
                &self,
                root: &str,
                sources: Vec<RegisteredSource>,
                limits: Limits,
            ) -> Result<DiscoveredClosure, DiscoveryError> {
                self.0.set(self.0.get() + 1);
                StaticDiscovery.capture(root, sources, limits)
            }
        }
        let mut registry = fixture();
        let discovery_calls = std::rc::Rc::new(std::cell::Cell::new(0));
        registry.discovery = Box::new(CountDiscovery(discovery_calls.clone()));
        let root = registry
            .document("root")
            .unwrap()
            .evaluation_source_id()
            .clone();
        let dep = registry
            .document("dep")
            .unwrap()
            .evaluation_source_id()
            .clone();
        assert_ne!(root, dep);
        assert_ne!(
            registry.binding_source_id("root", &root),
            registry.binding_source_id("dep", &dep)
        );
        assert!(registry.binding_source_id("root", &dep).is_none());
        let mut policy = Policy {
            calls: 0,
            allow: false,
        };
        let binding = registry.capture("root", "inspect").unwrap();
        assert!(registry.invoke(binding, &mut policy).is_err());
        assert_eq!(registry.dispatches(), 0);
        policy.allow = true;
        let binding = registry.capture("root", "inspect").unwrap();
        let result = registry.invoke(binding, &mut policy).unwrap();
        assert_eq!(*result.output(), 1); // static unit discovery has no evaluator.
        assert_eq!(result.result().output, *result.output());
        assert_eq!(result.binding(), &result.result().binding);
        let before = result.review().unwrap();
        let calls_before_display = discovery_calls.get();
        registry.remove("dep");
        for _ in 0..3 {
            assert_eq!(result.review().unwrap().as_str(), before.as_str());
            assert_eq!(
                &*result
                    .resolve(&dep, RevisionId::from_version(0))
                    .unwrap()
                    .text,
                "pub fn value() -> Int32 { 42 }"
            );
            assert_eq!(result.discovered().sources().len(), 2);
        }
        assert!(result.resolve(&dep, RevisionId::from_version(1)).is_err());
        assert_eq!(registry.dispatches(), 1);
        assert_eq!(policy.calls, 2);
        assert_eq!(discovery_calls.get(), calls_before_display);
        assert_eq!(
            discovery_calls.get(),
            2,
            "only the two initial captures discover"
        );
        assert_eq!(result.compiler_source(0).unwrap().source_id, root);
        assert_eq!(result.compiler_source(1).unwrap().source_id, dep);
        assert!(result.compiler_source(2).is_none());
    }
    #[test]
    fn mutations_reject_before_policy_and_fresh_selection_recovers() {
        for mutation in 0..7 {
            let mut registry = fixture();
            let binding = registry.capture("root", "inspect").unwrap();
            match mutation {
                0 => registry
                    .document_mut("dep")
                    .unwrap()
                    .apply_edit(TextEdit::insert(Position::new(0, 0), " "))
                    .unwrap(),
                1 => {
                    registry
                        .register(
                            "root".into(),
                            Document::new(
                                DocumentId::new(7),
                                "root.sprout",
                                registry.document("root").unwrap().text(),
                            ),
                        )
                        .unwrap();
                }
                2 => {
                    registry.remove("dep");
                }
                3 => registry.set_policy(json!({"allow_counter":false})),
                4 => {
                    let mut op = operation();
                    op.description.push_str(" changed");
                    registry.set_catalog(vec![op]);
                }
                5 => registry
                    .set_artifact(TrustedLoadedArtifact::default())
                    .unwrap(),
                _ => registry.set_limits(Limits {
                    steps: 9,
                    ..Limits::default()
                }),
            }
            let mut policy = Policy {
                calls: 0,
                allow: true,
            };
            assert!(
                registry.invoke(binding, &mut policy).is_err(),
                "mutation {mutation}"
            );
            assert_eq!(policy.calls, 0);
            assert_eq!(registry.dispatches(), 0);
            // Restore metadata/dependency, then capture and authorize afresh.
            registry.set_policy(json!({"allow_counter":true}));
            registry.set_catalog(vec![operation()]);
            if registry.document("dep").is_none() {
                registry
                    .register(
                        "dep".into(),
                        Document::new(
                            DocumentId::new(7),
                            "dep.sprout",
                            "pub fn value() -> Int32 { 42 }",
                        ),
                    )
                    .unwrap();
            }
            let fresh = registry.capture("root", "inspect").unwrap();
            assert!(registry.invoke(fresh, &mut policy).is_ok());
            assert_eq!(policy.calls, 1);
        }
    }
    #[test]
    fn owner_replacement_mapping_catalog_and_counter_bounds_fail_closed() {
        let mut registry = fixture();
        let binding = registry.capture("root", "inspect").unwrap();
        let mut other = fixture();
        let mut policy = Policy {
            calls: 0,
            allow: true,
        };
        assert!(other.invoke(binding, &mut policy).is_err());
        assert_eq!(policy.calls, 0);
        let replacement = Document::new(
            DocumentId::new(7),
            "dep.sprout",
            registry.document("dep").unwrap().text(),
        );
        *registry.document_mut("dep").unwrap() = replacement;
        assert!(registry.capture("root", "inspect").is_err());
        let mut registry = fixture();
        registry.set_catalog(vec![operation(), operation()]);
        assert!(registry.capture("root", "inspect").is_err());
        registry.set_catalog(vec![operation()]);
        let mut legacy_calls = 0;
        let mut legacy = |_: SemanticAuthorization<'_>| {
            legacy_calls += 1;
            true
        };
        let binding = registry.capture("root", "inspect").unwrap();
        assert!(registry.invoke(binding, &mut legacy).is_err());
        assert_eq!(legacy_calls, 0);
        for _ in 0..2 {
            let binding = registry.capture("root", "inspect").unwrap();
            registry.invoke(binding, &mut policy).unwrap();
        }
        let binding = registry.capture("root", "inspect").unwrap();
        assert!(registry.invoke(binding, &mut policy).is_err());
        assert_eq!(registry.dispatches(), 2);
    }

    struct PreparedFixture {
        calls: Arc<std::sync::atomic::AtomicUsize>,
        output: Result<String, ProviderFailure>,
    }

    fn prepared_descriptor() -> PreparedImplementationDescriptor {
        PreparedImplementationDescriptor {
            sha256: "ab".repeat(32),
            abi: "unit-prepared-v1".into(),
            execution_policy: json!({"prepared": true}),
        }
    }

    fn prepared_fixture(
        calls: Arc<std::sync::atomic::AtomicUsize>,
        output: Result<String, ProviderFailure>,
    ) -> RegisteredDocuments<PreparedProvider<PreparedFixture, String>> {
        RegisteredDocuments::with_prepared_provider(
            fixture(),
            prepared_descriptor(),
            PreparedFixture { calls, output },
            |prepared, request| {
                prepared
                    .calls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                assert_eq!(request.operation(), &operation());
                assert_eq!(request.closure().sources().len(), 2);
                assert_eq!(request.execution_policy()["host"]["allow_counter"], true);
                assert_eq!(
                    request.compiler_source(0).unwrap().text.as_ref(),
                    "import dep; fn main() -> Int32 { dep.value() / 0 }"
                );
                assert_eq!(
                    request.compiler_source(1).unwrap().text.as_ref(),
                    "pub fn value() -> Int32 { 42 }"
                );
                assert!(request.compiler_source(2).is_none());
                prepared.output.clone()
            },
        )
        .unwrap()
    }

    #[test]
    fn prepared_provider_denied_never_called_and_allowed_returns_typed_output_once() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut registry =
            prepared_fixture(calls.clone(), Ok("typed output, not dispatch 1".into()));
        let mut policy = Policy {
            calls: 0,
            allow: false,
        };
        let selection = registry.capture("root", "inspect").unwrap();
        assert!(registry.invoke(selection, &mut policy).is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(registry.dispatches(), 0);
        assert_eq!(policy.calls, 1);
        policy.allow = true;
        let selection = registry.capture("root", "inspect").unwrap();
        let result = registry.invoke(selection, &mut policy).unwrap();
        assert_eq!(result.output(), &Ok("typed output, not dispatch 1".into()));
        assert_eq!(&result.result().output, result.output());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(registry.dispatches(), 1);
        assert_eq!(policy.calls, 2);
    }

    #[test]
    fn prepared_app_invocation_retains_typed_historical_evidence() {
        use crate::{app::App, test_support::TempWorkspace};
        let root = TempWorkspace::new("prepared_app_evidence");
        let mut registry = fixture();
        let documents: Vec<_> = ["root", "dep"]
            .into_iter()
            .map(|module| (module, registry.remove(module).unwrap()))
            .collect();
        let mut app = App::new(root.workspace()).unwrap();
        assert!(app.enable_registered_session(registry).is_ok());
        for (module, document) in documents {
            app.registered_documents_mut()
                .unwrap()
                .register(module.into(), document)
                .unwrap();
        }
        let mut app = app
            .try_with_prepared_provider(prepared_descriptor(), 42u32, |value, _| Ok(*value))
            .unwrap();
        let binding = app
            .capture_registered_execution("root", "inspect")
            .unwrap()
            .unwrap();
        let mut policy = Policy {
            calls: 0,
            allow: true,
        };
        let result = app
            .invoke_registered_execution(binding, &mut policy)
            .unwrap()
            .unwrap();
        assert_eq!(result.output(), &Ok(42u32));
        let source = result.discovered().sources()[0].text.clone();
        let stale = app
            .capture_registered_execution("root", "inspect")
            .unwrap()
            .unwrap();
        app.select_registered_document("root").unwrap();
        app.open_document_mut()
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), " "))
            .unwrap();
        assert!(
            app.invoke_registered_execution(stale, &mut policy)
                .unwrap()
                .is_err()
        );
        assert_eq!(policy.calls, 1);
        drop(app);
        assert_eq!(result.output(), &Ok(42u32));
        assert_eq!(result.discovered().sources()[0].text, source);
    }

    #[test]
    fn prepared_provider_stale_selections_never_call_provider_or_policy() {
        for mutation in 0..7 {
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut registry = prepared_fixture(calls.clone(), Ok("must not run".into()));
            let selection = registry.capture("root", "inspect").unwrap();
            match mutation {
                0 | 1 => registry
                    .document_mut(if mutation == 0 { "root" } else { "dep" })
                    .unwrap()
                    .apply_edit(TextEdit::insert(Position::new(0, 0), " "))
                    .unwrap(),
                2 => {
                    let dep = registry.remove("dep").unwrap();
                    registry.register("dep".into(), dep).unwrap();
                }
                3 => {
                    let text = registry.document("root").unwrap().text();
                    let replacement = Document::new(DocumentId::new(7), "root.sprout", text);
                    *registry.document_mut("root").unwrap() = replacement;
                }
                4 => {
                    let mut changed = operation();
                    changed.description.push_str(" changed");
                    registry.set_catalog(vec![changed]);
                }
                5 => registry.set_policy(json!({"allow_counter": false})),
                _ => registry
                    .replace_artifact(
                        TrustedLoadedArtifact::prepared(prepared_descriptor(), ()).unwrap(),
                    )
                    .unwrap(),
            }
            let mut policy = Policy {
                calls: 0,
                allow: true,
            };
            assert!(
                registry.invoke(selection, &mut policy).is_err(),
                "mutation {mutation}"
            );
            assert_eq!(
                calls.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "mutation {mutation}"
            );
            assert_eq!(policy.calls, 0, "mutation {mutation}");
            assert_eq!(registry.dispatches(), 0);
        }
    }

    #[test]
    fn prepared_provider_cross_owner_selection_never_dispatches() {
        let first_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let other_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let first = prepared_fixture(first_calls.clone(), Ok("first".into()));
        let mut other = prepared_fixture(other_calls.clone(), Ok("other".into()));
        let selection = first.capture("root", "inspect").unwrap();
        let mut policy = Policy {
            calls: 0,
            allow: true,
        };
        assert!(other.invoke(selection, &mut policy).is_err());
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(other_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(first.dispatches(), 0);
        assert_eq!(other.dispatches(), 0);
        assert_eq!(policy.calls, 0);
    }

    struct PreparedCountDiscovery(Arc<std::sync::atomic::AtomicUsize>);
    impl Discovery for PreparedCountDiscovery {
        fn capture(
            &self,
            root: &str,
            sources: Vec<RegisteredSource>,
            limits: Limits,
        ) -> Result<DiscoveredClosure, DiscoveryError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            StaticDiscovery.capture(root, sources, limits)
        }
    }

    struct PreparedCountingPolicy {
        discovery_calls: Arc<std::sync::atomic::AtomicUsize>,
        policy: Policy,
    }
    impl HostSemanticPolicy for PreparedCountingPolicy {
        fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
            assert_eq!(
                self.discovery_calls
                    .load(std::sync::atomic::Ordering::SeqCst),
                1
            );
            self.policy.authorize(request)
        }
    }

    #[test]
    fn prepared_failures_retain_historical_mapping_and_reads_do_not_rediscover_or_dispatch() {
        for failure in [
            ProviderFailure::Rejected,
            ProviderFailure::LimitExceeded,
            ProviderFailure::Failed,
        ] {
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let discovery_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut registry = prepared_fixture(calls.clone(), Err(failure));
            registry.discovery = Box::new(PreparedCountDiscovery(discovery_calls.clone()));
            let root = registry
                .document("root")
                .unwrap()
                .evaluation_source_id()
                .clone();
            let dep = registry
                .document("dep")
                .unwrap()
                .evaluation_source_id()
                .clone();
            let selection = registry.capture("root", "inspect").unwrap();
            let mut policy = PreparedCountingPolicy {
                discovery_calls: discovery_calls.clone(),
                policy: Policy {
                    calls: 0,
                    allow: true,
                },
            };
            let result = registry.invoke(selection, &mut policy).unwrap();
            let review = result.review().unwrap();
            registry
                .document_mut("root")
                .unwrap()
                .apply_edit(TextEdit::insert(Position::new(0, 0), " changed "))
                .unwrap();
            registry.remove("dep");
            for _ in 0..3 {
                assert_eq!(result.output(), &Err(failure));
                assert_eq!(&result.result().output, &Err(failure));
                assert_eq!(result.binding(), &result.result().binding);
                assert_eq!(result.review().unwrap().as_str(), review.as_str());
                assert_eq!(result.discovered().sources().len(), 2);
                for (index, id, text) in [
                    (
                        0,
                        &root,
                        "import dep; fn main() -> Int32 { dep.value() / 0 }",
                    ),
                    (1, &dep, "pub fn value() -> Int32 { 42 }"),
                ] {
                    let source = result.compiler_source(index).unwrap();
                    assert_eq!(&source.source_id, id);
                    assert_eq!(source.revision_id, RevisionId::from_version(0));
                    assert_eq!(source.text.as_ref(), text);
                    assert_eq!(
                        result
                            .resolve(id, source.revision_id)
                            .unwrap()
                            .text
                            .as_ref(),
                        text
                    );
                    assert!(result.resolve(id, RevisionId::from_version(1)).is_err());
                }
                assert!(result.compiler_source(2).is_none());
            }
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(discovery_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(policy.policy.calls, 1);
            assert_eq!(registry.dispatches(), 1);
        }
    }

    #[test]
    fn prepared_provider_valid_unrelated_edit_preserves_selection_without_rediscovery() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let discovery_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut registry = prepared_fixture(calls.clone(), Ok("unrelated edit accepted".into()));
        registry
            .register(
                "unrelated".into(),
                Document::new(DocumentId::new(8), "unrelated.sprout", "fn unused() {}"),
            )
            .unwrap();
        registry.discovery = Box::new(PreparedCountDiscovery(discovery_calls.clone()));
        let selection = registry.capture("root", "inspect").unwrap();
        assert_eq!(selection.discovered().sources().len(), 2);
        registry
            .document_mut("unrelated")
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), " "))
            .unwrap();
        let mut policy = PreparedCountingPolicy {
            discovery_calls: discovery_calls.clone(),
            policy: Policy {
                calls: 0,
                allow: true,
            },
        };
        let result = registry.invoke(selection, &mut policy).unwrap();
        assert_eq!(result.output(), &Ok("unrelated edit accepted".into()));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(discovery_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(policy.policy.calls, 1);
        assert_eq!(registry.dispatches(), 1);
    }
    struct CallPolicy;
    impl HostSemanticPolicy for CallPolicy {
        fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
            matches!(request, HostAuthorizationRequest::Closure { binding, .. }
                if binding.arguments() == &json!({"quest":"q1"}))
        }
    }
    fn call_contract() -> InvocationContract {
        InvocationContract {
            schema_version: "call-v1".into(),
            entry: "quests.tick".into(),
            output: "quests-result-v1".into(),
            effect: "read-v1".into(),
        }
    }
    fn parameterized_fixture() -> RegisteredDocuments {
        let mut registry = fixture();
        let mut op = operation();
        op.arguments = vec![scope_language::SemanticArgument {
            name: "quest".into(),
            kind: scope_language::SemanticArgumentKind::String,
            required: true,
            description: "quest identity".into(),
        }];
        registry.set_catalog(vec![op]);
        registry
    }
    struct ServicePolicy {
        calls: usize,
    }
    impl HostSemanticPolicy for ServicePolicy {
        fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
            self.calls += 1;
            match request {
                HostAuthorizationRequest::RecurringClosure {
                    binding,
                    review,
                    request_ref,
                    expires_at,
                    grant_id,
                    grant_version,
                } => {
                    assert_eq!(grant_id, "service-7");
                    assert_eq!(grant_version, 1);
                    assert_eq!(request_ref, "trusted-request-7");
                    assert!(expires_at > std::time::Instant::now());
                    assert_eq!(binding.arguments(), &json!({"quest":"q1"}));
                    assert_eq!(binding.review().unwrap().as_str(), review.as_str());
                    true
                }
                _ => false,
            }
        }
    }
    #[test]
    fn parameterized_arguments_reject_before_discovery_and_seal_exact_review() {
        let mut registry = parameterized_fixture();
        for args in [
            json!({}),
            json!({"quest":1}),
            json!({"quest":"q1","extra":true}),
            json!({"quest":[]}),
            json!({"quest":null}),
            json!([]),
        ] {
            assert!(
                registry
                    .capture_invocation("root", "inspect", args, call_contract())
                    .is_err()
            );
        }
        assert!(
            registry
                .capture_invocation(
                    "root",
                    "inspect",
                    json!({"quest":"x".repeat(16384)}),
                    call_contract()
                )
                .is_err()
        );
        let selected = registry
            .capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
            .unwrap();
        let other = registry
            .capture_invocation("root", "inspect", json!({"quest":"q2"}), call_contract())
            .unwrap();
        assert_ne!(selected.binding(), other.binding());
        let review = selected.binding().review().unwrap();
        let json: Value = serde_json::from_str(review.as_str()).unwrap();
        assert_eq!(json["arguments"], json!({"quest":"q1"}));
        assert_eq!(
            json["compilation_options"]["invocation_contract"]["schema_version"],
            "call-v1"
        );
        let result = registry.invoke(selected, &mut CallPolicy).unwrap();
        assert_eq!(result.binding().arguments(), &json!({"quest":"q1"}));
        registry.remove("dep");
        assert_eq!(result.review().unwrap().as_str(), review.as_str());
        assert_eq!(registry.dispatches(), 1);
    }
    #[test]
    fn parameterized_schema_rejects_duplicate_empty_and_container_fields() {
        use scope_language::SemanticArgumentKind;
        for kind in [SemanticArgumentKind::Array, SemanticArgumentKind::Object] {
            let mut registry = parameterized_fixture();
            registry.catalog[0].arguments[0].kind = kind;
            assert!(
                registry
                    .capture_invocation("root", "inspect", json!({"quest":null}), call_contract())
                    .is_err()
            );
        }
        let mut registry = parameterized_fixture();
        let duplicate = registry.catalog[0].arguments[0].clone();
        registry.catalog[0].arguments.push(duplicate);
        assert!(
            registry
                .capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
                .is_err()
        );
        registry.catalog[0].arguments.truncate(1);
        registry.catalog[0].arguments[0].name.clear();
        assert!(
            registry
                .capture_invocation("root", "inspect", json!({"":"q1"}), call_contract())
                .is_err()
        );
    }
    #[test]
    fn structured_early_capture_admission_rejects_deep_owned_json() {
        use crate::closure_inspection::IterativeJson;
        fn deep() -> Value {
            let mut v = Value::Null;
            for _ in 0..20_000 {
                v = Value::Array(vec![v]);
            }
            v
        }
        let mut registry = parameterized_fixture();
        for keyword in ["const", "enum"] {
            let mut schema = json!({"type":"object"});
            schema[keyword] = if keyword == "enum" {
                Value::Array(vec![deep()])
            } else {
                deep()
            };
            registry.catalog[0].constraints = Some(Value::Object(serde_json::Map::from_iter([(
                "scope_argument_schema".into(),
                schema,
            )])));
            assert!(
                registry
                    .capture_invocation("root", "inspect", deep(), call_contract())
                    .is_err()
            );
            assert_eq!(registry.dispatches(), 0);
            drop(IterativeJson(
                registry.catalog[0].constraints.take().unwrap(),
            ));
        }
        // Missing operation also rejects without recursively dropping owned input.
        assert!(
            registry
                .capture_invocation("root", "missing", deep(), call_contract())
                .is_err()
        );
    }
    #[test]
    fn structured_capture_requires_schema_and_binds_exact_values() {
        let mut registry = parameterized_fixture();
        registry.catalog[0].arguments[0].kind = scope_language::SemanticArgumentKind::Array;
        let args = json!({"quest":[{"tag":"Quest","id":"q1"}]});
        assert!(
            registry
                .capture_invocation("root", "inspect", args.clone(), call_contract())
                .is_err()
        );
        registry.catalog[0].constraints = Some(json!({"scope_argument_schema":{
            "type":"object","additionalProperties":false,"required":["quest"],"properties":{
                "quest":{"type":"array","maxItems":2,"items":{
                    "type":"object","additionalProperties":false,"required":["tag","id"],"properties":{
                        "tag":{"type":"string","const":"Quest"},"id":{"type":"string","minLength":1}
                    }}}
            }
        }}));
        let first = registry
            .capture_invocation("root", "inspect", args.clone(), call_contract())
            .unwrap();
        assert_eq!(first.binding().arguments(), &args);
        let second = registry
            .capture_invocation(
                "root",
                "inspect",
                json!({"quest":[{"tag":"Quest","id":"q2"}]}),
                call_contract(),
            )
            .unwrap();
        assert_ne!(
            first.binding().review().unwrap().as_str(),
            second.binding().review().unwrap().as_str()
        );
        assert!(
            registry
                .capture_invocation(
                    "root",
                    "inspect",
                    json!({"quest":[{"tag":"Other","id":"q1"}]}),
                    call_contract()
                )
                .is_err()
        );
        assert_eq!(registry.dispatches(), 0);
    }

    fn grant_identity() -> ServiceGrantIdentity {
        ServiceGrantIdentity {
            id: "service-7".into(),
            version: 1,
        }
    }
    fn approve(
        registry: &RegisteredDocuments,
        selected: &RegisteredClosureBinding,
    ) -> RegisteredServiceGrant {
        registry
            .approve_service(
                selected,
                grant_identity(),
                "trusted-request-7".into(),
                std::time::Instant::now() + std::time::Duration::from_secs(60),
                &mut ServicePolicy { calls: 0 },
            )
            .unwrap()
    }
    struct TickPolicy {
        calls: usize,
        allow: bool,
        seen: Vec<Value>,
    }
    impl HostSemanticPolicy for TickPolicy {
        fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
            self.calls += 1;
            match request {
                HostAuthorizationRequest::ServiceClosure {
                    binding,
                    review,
                    grant_id,
                    grant_version,
                    request_ref,
                } => {
                    assert_eq!(grant_id, "service-7");
                    assert_eq!(grant_version, 1);
                    assert!(request_ref.starts_with("tick-"));
                    assert_eq!(binding.review().unwrap().as_str(), review.as_str());
                    self.seen.push(binding.arguments().clone());
                    self.allow
                }
                _ => panic!("expected per-request service authorization"),
            }
        }
    }
    #[test]
    fn recurring_service_different_arguments_same_grant_each_request_authorized() {
        let mut registry = parameterized_fixture();
        let capture = |r: &RegisteredDocuments, quest| {
            r.capture_invocation("root", "inspect", json!({"quest":quest}), call_contract())
                .unwrap()
        };
        let selected = capture(&registry, "q1");
        let expiry = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut legacy = |_: crate::tool::semantic::SemanticAuthorization<'_>| true;
        assert!(
            registry
                .approve_service(
                    &selected,
                    grant_identity(),
                    "trusted-request-7".into(),
                    expiry,
                    &mut legacy
                )
                .is_err()
        );
        let grant = approve(&registry, &selected);
        assert_eq!(grant.identity(), &grant_identity());
        assert_eq!(grant.request_ref(), "trusted-request-7");
        let mut policy = TickPolicy {
            calls: 0,
            allow: true,
            seen: vec![],
        };
        let first = registry
            .invoke_service(selected, &grant, "tick-1", &mut policy)
            .unwrap();
        let second = registry
            .invoke_service(capture(&registry, "q2"), &grant, "tick-2", &mut policy)
            .unwrap();
        assert_eq!(policy.calls, 2);
        assert_eq!(
            policy.seen,
            vec![json!({"quest":"q1"}), json!({"quest":"q2"})]
        );
        assert_eq!(registry.dispatches(), 2);
        assert_eq!(first.binding().arguments(), &json!({"quest":"q1"}));
        assert_eq!(second.binding().arguments(), &json!({"quest":"q2"}));
        assert_eq!(
            second.service().unwrap(),
            &ServiceInvocationEvidence {
                grant: grant_identity(),
                request_ref: "tick-2".into()
            }
        );
    }
    #[test]
    fn recurring_per_request_denial_and_revocation_during_policy_never_dispatch() {
        let mut registry = parameterized_fixture();
        let capture = |r: &RegisteredDocuments| {
            r.capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
                .unwrap()
        };
        let selected = capture(&registry);
        let grant = approve(&registry, &selected);
        let mut denied = TickPolicy {
            calls: 0,
            allow: false,
            seen: vec![],
        };
        assert!(
            registry
                .invoke_service(selected, &grant, "tick-denied", &mut denied)
                .is_err()
        );
        assert_eq!(denied.calls, 1);
        assert_eq!(registry.dispatches(), 0);
        struct RevokePolicy<'a>(&'a RegisteredServiceGrant, usize);
        impl HostSemanticPolicy for RevokePolicy<'_> {
            fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
                assert!(matches!(
                    request,
                    HostAuthorizationRequest::ServiceClosure { .. }
                ));
                self.1 += 1;
                self.0.revoke();
                true
            }
        }
        let mut policy = RevokePolicy(&grant, 0);
        assert!(
            registry
                .invoke_service(capture(&registry), &grant, "tick-revoke", &mut policy)
                .is_err()
        );
        assert_eq!(policy.1, 1);
        assert_eq!(registry.dispatches(), 0);
        let mut allowed = TickPolicy {
            calls: 0,
            allow: true,
            seen: vec![],
        };
        assert!(
            registry
                .invoke_service(capture(&registry), &grant, "tick-after", &mut allowed)
                .is_err()
        );
        assert_eq!(allowed.calls, 0);
    }
    #[test]
    fn recurring_scope_matches_everything_except_argument_values() {
        let mut registry = parameterized_fixture();
        let selected = registry
            .capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
            .unwrap();
        let grant = approve(&registry, &selected);
        let mut policy = TickPolicy {
            calls: 0,
            allow: true,
            seen: vec![],
        };
        for field in ["schema", "entry", "output", "effect"] {
            let mut contract = call_contract();
            match field {
                "schema" => contract.schema_version.push('2'),
                "entry" => contract.entry.push('2'),
                "output" => contract.output.push('2'),
                _ => contract.effect.push('2'),
            }
            let other = registry
                .capture_invocation("root", "inspect", json!({"quest":"q2"}), contract)
                .unwrap();
            assert!(
                registry
                    .invoke_service(other, &grant, "tick-mismatch", &mut policy)
                    .is_err()
            );
        }
        for change in ["schema", "operation", "policy", "artifact"] {
            let mut other = parameterized_fixture();
            let original = other
                .capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
                .unwrap();
            let approved = approve(&other, &original);
            match change {
                "schema" => other.catalog[0].arguments[0].description.push('2'),
                "operation" => other.catalog[0].description.push('2'),
                "policy" => other.set_policy(json!({"changed":true})),
                _ => other
                    .set_artifact(TrustedLoadedArtifact::default())
                    .unwrap(),
            }
            let fresh = other
                .capture_invocation("root", "inspect", json!({"quest":"q2"}), call_contract())
                .unwrap();
            assert!(
                other
                    .invoke_service(fresh, &approved, "tick-mismatch", &mut policy)
                    .is_err()
            );
        }
        assert_eq!(policy.calls, 0);
        assert_eq!(registry.dispatches(), 0);
    }
    #[test]
    fn recurring_dispatch_rechecks_expiry_revocation_and_freshness() {
        let mut registry = parameterized_fixture();
        let selected = registry
            .capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
            .unwrap();
        let mut grant = approve(&registry, &selected);
        grant.expires_at = std::time::Instant::now();
        let mut host = RegisteredHost {
            registry: &mut registry,
            sealed: &selected,
            grant: Some(&grant),
        };
        assert_eq!(
            host.dispatch(selected.binding()),
            Err(ClosureInspectionError::Denied)
        );
        grant.expires_at = std::time::Instant::now() + std::time::Duration::from_secs(60);
        grant.revoke();
        let mut host = RegisteredHost {
            registry: &mut registry,
            sealed: &selected,
            grant: Some(&grant),
        };
        assert_eq!(
            host.dispatch(selected.binding()),
            Err(ClosureInspectionError::Denied)
        );
        assert_eq!(registry.dispatches(), 0);
        let fresh_grant = approve(&registry, &selected);
        registry
            .document_mut("dep")
            .unwrap()
            .apply_edit(TextEdit::insert(Position::new(0, 0), " "))
            .unwrap();
        let mut policy = TickPolicy {
            calls: 0,
            allow: true,
            seen: vec![],
        };
        assert!(
            registry
                .invoke_service(selected, &fresh_grant, "tick-stale", &mut policy)
                .is_err()
        );
        assert_eq!(policy.calls, 0);
        assert_eq!(registry.dispatches(), 0);
    }
    #[test]
    fn app_parameterized_capture_uses_authoritative_session() {
        use crate::{app::App, test_support::TempWorkspace};
        let root = TempWorkspace::new("app_parameterized_capture");
        root.write_file("root.sprout", "root");
        root.write_file("dep.sprout", "dep");
        let mut app = App::new(root.workspace()).unwrap();
        assert!(
            app.capture_registered_invocation("root", "inspect", json!({}), call_contract())
                .is_none()
        );
        let mut registry = RegisteredDocuments::new(
            StaticDiscovery,
            TrustedLoadedArtifact::default(),
            vec![operation()],
            json!({}),
            Limits::default(),
            2,
        );
        registry.set_catalog(parameterized_fixture().catalog);
        assert!(app.enable_registered_session(registry).is_ok());
        for module in ["root", "dep"] {
            app.open_path(format!("{module}.sprout")).unwrap();
            app.enroll_open_document(module.into(), "sprout".into())
                .unwrap();
        }
        let selected = app
            .capture_registered_invocation(
                "root",
                "inspect",
                json!({"quest":"q1"}),
                call_contract(),
            )
            .unwrap()
            .unwrap();
        let result = app
            .invoke_registered_execution(selected, &mut CallPolicy)
            .unwrap()
            .unwrap();
        assert_eq!(result.binding().arguments(), &json!({"quest":"q1"}));
        assert!(result.service().is_none());
    }
    #[test]
    fn prepared_provider_receives_exact_parameterized_call() {
        fn dispatch(_: &(), request: ProviderRequest<'_>) -> Result<Value, ProviderFailure> {
            assert_eq!(request.operation().id, "inspect");
            assert_eq!(
                request.binding().snapshot().compilation_options["invocation_contract"]["entry"],
                "quests.tick"
            );
            Ok(request.arguments().clone())
        }
        let mut registry = RegisteredDocuments::with_prepared_provider(
            parameterized_fixture(),
            PreparedImplementationDescriptor {
                sha256: "a".repeat(64),
                abi: "call-v1".into(),
                execution_policy: json!({"bounded":true}),
            },
            (),
            dispatch,
        )
        .unwrap();
        let selected = registry
            .capture_invocation("root", "inspect", json!({"quest":"q1"}), call_contract())
            .unwrap();
        let result = registry.invoke(selected, &mut CallPolicy).unwrap();
        assert_eq!(result.output(), &Ok(json!({"quest":"q1"})));
        assert_eq!(registry.dispatches(), 1);
    }
}
