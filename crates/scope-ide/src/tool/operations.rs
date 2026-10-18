//! Language-neutral, revision-bound operations without transport or implicit authority.
//!
//! Register providers once, discover their descriptors with `descriptors`, then call
//! `invoke` with an explicit policy checking the effect, every declared permission,
//! and the call arguments. Policies also see the validated snapshot.
//! Provider/action identities are separate components, never concatenated names.
//! Limits bound accepted data and validation work, not provider allocations, runtime,
//! panics, or external side effects. Providers are trusted in-process implementations.

use std::collections::{BTreeMap, BTreeSet};

use super::{ToolArguments, ToolEffect, ToolValue};
use crate::document::DocumentId;
use crate::document::session::{DocumentError, DocumentSession, DocumentSnapshot, Revision};

/// Closed recursive schemas: undeclared object fields are rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationSchema {
    Null,
    Boolean,
    Integer,
    String,
    Array(Box<OperationSchema>),
    Object {
        fields: BTreeMap<String, OperationSchema>,
        required: BTreeSet<String>,
    },
}

/// Execution requirements are declarations, not a scheduler or capability grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationExecution {
    Immediate,
    Blocking,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationDescriptor {
    pub action_id: String,
    pub description: String,
    pub input: OperationSchema,
    pub output: OperationSchema,
    pub effect: ToolEffect,
    pub execution: OperationExecution,
    /// Host-defined permission names; the policy must interpret these explicitly.
    pub authorization: BTreeSet<String>,
}

/// Provider-local errors are bounded before they cross the registry boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationFailure {
    pub message: String,
}

pub trait OperationProvider {
    fn provider_id(&self) -> &str;
    fn descriptors(&self) -> &[OperationDescriptor];
    fn invoke(
        &self,
        action_id: &str,
        snapshot: &DocumentSnapshot,
        arguments: &ToolArguments,
    ) -> Result<ToolValue, OperationFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OperationId {
    pub provider_id: String,
    pub action_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationCall {
    pub operation: OperationId,
    pub arguments: ToolArguments,
}

pub struct OperationAuthorization<'a> {
    pub call: &'a OperationCall,
    pub descriptor: &'a OperationDescriptor,
    pub snapshot: &'a DocumentSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationResult {
    pub document_id: DocumentId,
    pub revision: Revision,
    pub value: ToolValue,
}

/// Byte budgets count UTF-8 text and object keys; node budgets count values/schema
/// nodes. Metadata is cumulative across the registry. Depth is capped at 128 even
/// when configured higher, keeping recursive schema matching stack-bounded.
#[derive(Debug, Clone, Copy)]
pub struct OperationLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_bytes: usize,
    pub max_metadata_nodes: usize,
    pub max_metadata_bytes: usize,
    pub max_diagnostic_bytes: usize,
}

impl Default for OperationLimits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_nodes: 16_384,
            max_bytes: 1_048_576,
            max_metadata_nodes: 16_384,
            max_metadata_bytes: 262_144,
            max_diagnostic_bytes: 1_024,
        }
    }
}

/// Fixed diagnostics avoid reflecting arbitrarily large keys or input values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationValidation {
    LimitExceeded,
    TypeMismatch,
    MissingRequiredField,
    UnknownField,
    InvalidRequiredField,
    InputMustBeObject,
}

#[derive(Debug)]
pub enum OperationError {
    InvalidIdentifier,
    DuplicateProvider,
    DuplicateAction,
    InvalidMetadata(OperationValidation),
    UnknownOperation,
    InvalidInput(OperationValidation),
    StaleRevision,
    Document(DocumentError),
    Unauthorized,
    ProviderFailure(OperationFailure),
    InvalidOutput(OperationValidation),
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentifier => f.write_str("invalid operation identifier"),
            Self::DuplicateProvider => f.write_str("duplicate operation provider"),
            Self::DuplicateAction => f.write_str("duplicate provider-local action"),
            Self::InvalidMetadata(reason) => write!(f, "invalid operation metadata: {reason:?}"),
            Self::UnknownOperation => f.write_str("unknown operation"),
            Self::InvalidInput(reason) => write!(f, "invalid operation input: {reason:?}"),
            Self::StaleRevision => f.write_str("stale document revision"),
            // Do not reflect potentially unbounded filesystem diagnostics.
            Self::Document(_) => f.write_str("document snapshot unavailable"),
            Self::Unauthorized => f.write_str("operation unauthorized"),
            Self::ProviderFailure(error) => {
                write!(f, "operation provider failed: {}", error.message)
            }
            Self::InvalidOutput(reason) => write!(f, "invalid operation output: {reason:?}"),
        }
    }
}

impl std::error::Error for OperationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Document(error) => Some(error),
            _ => None,
        }
    }
}

struct RegisteredProvider {
    provider: Box<dyn OperationProvider>,
    descriptors: BTreeMap<String, OperationDescriptor>,
}

pub struct OperationRegistry {
    providers: BTreeMap<String, RegisteredProvider>,
    limits: OperationLimits,
    metadata: Budget,
}

impl OperationRegistry {
    pub fn new(limits: OperationLimits) -> Self {
        Self {
            providers: BTreeMap::new(),
            limits,
            metadata: Budget::new(limits.max_metadata_nodes, limits.max_metadata_bytes),
        }
    }

    /// Atomic registration: failures leave both the catalog and budgets unchanged.
    /// IDs and field/permission names use ASCII `[A-Za-z_][A-Za-z0-9_-]*`,
    /// with at most 128 bytes. Dots, separators, and empty names are rejected.
    pub fn register(&mut self, provider: Box<dyn OperationProvider>) -> Result<(), OperationError> {
        let id = provider.provider_id();
        identifier(id)?;
        if self.providers.contains_key(id) {
            return Err(OperationError::DuplicateProvider);
        }
        let mut budget = self.metadata;
        budget
            .take(1, id.len())
            .map_err(OperationError::InvalidMetadata)?;
        let mut descriptors = BTreeMap::new();
        for descriptor in provider.descriptors() {
            identifier(&descriptor.action_id)?;
            if descriptors.contains_key(&descriptor.action_id) {
                return Err(OperationError::DuplicateAction);
            }
            budget
                .take(1, descriptor.action_id.len())
                .map_err(OperationError::InvalidMetadata)?;
            budget
                .take(0, descriptor.description.len())
                .map_err(OperationError::InvalidMetadata)?;
            for permission in &descriptor.authorization {
                identifier(permission)?;
                budget
                    .take(1, permission.len())
                    .map_err(OperationError::InvalidMetadata)?;
            }
            if !matches!(descriptor.input, OperationSchema::Object { .. }) {
                return Err(OperationError::InvalidMetadata(
                    OperationValidation::InputMustBeObject,
                ));
            }
            schema_budget(
                &descriptor.input,
                0,
                self.limits.max_depth.min(128),
                &mut budget,
            )
            .map_err(OperationError::InvalidMetadata)?;
            schema_budget(
                &descriptor.output,
                0,
                self.limits.max_depth.min(128),
                &mut budget,
            )
            .map_err(OperationError::InvalidMetadata)?;
            descriptors.insert(descriptor.action_id.clone(), descriptor.clone());
        }
        let id = id.to_owned();
        self.providers.insert(
            id,
            RegisteredProvider {
                provider,
                descriptors,
            },
        );
        self.metadata = budget;
        Ok(())
    }

    pub fn descriptors(&self) -> impl Iterator<Item = (OperationId, &OperationDescriptor)> {
        self.providers.iter().flat_map(|(provider_id, provider)| {
            provider
                .descriptors
                .iter()
                .map(move |(action_id, descriptor)| {
                    (
                        OperationId {
                            provider_id: provider_id.clone(),
                            action_id: action_id.clone(),
                        },
                        descriptor,
                    )
                })
        })
    }

    /// No default policy exists. Unknown, malformed, stale, and denied calls never
    /// dispatch. Source is revalidated after dispatch before accepting the result.
    pub fn invoke(
        &self,
        session: &mut DocumentSession,
        expected: &Revision,
        call: &OperationCall,
        authorize: impl FnOnce(OperationAuthorization<'_>) -> bool,
    ) -> Result<OperationResult, OperationError> {
        identifier(&call.operation.provider_id)?;
        identifier(&call.operation.action_id)?;
        let provider = self
            .providers
            .get(&call.operation.provider_id)
            .ok_or(OperationError::UnknownOperation)?;
        let descriptor = provider
            .descriptors
            .get(&call.operation.action_id)
            .ok_or(OperationError::UnknownOperation)?;
        let mut budget = Budget::new(self.limits.max_nodes, self.limits.max_bytes);
        budget.take(1, 0).map_err(OperationError::InvalidInput)?;
        for (key, value) in &call.arguments {
            budget
                .take(0, key.len())
                .map_err(OperationError::InvalidInput)?;
            value_budget(value, 1, self.limits.max_depth.min(128), &mut budget)
                .map_err(OperationError::InvalidInput)?;
        }
        if let OperationSchema::Object { fields, required } = &descriptor.input {
            match_object(fields, required, &call.arguments)
                .map_err(OperationError::InvalidInput)?;
        }
        let snapshot = session.snapshot_at(expected).map_err(document_error)?;
        if !authorize(OperationAuthorization {
            call,
            descriptor,
            snapshot: &snapshot,
        }) {
            return Err(OperationError::Unauthorized);
        }
        let value = provider
            .provider
            .invoke(&descriptor.action_id, &snapshot, &call.arguments)
            .map_err(|error| {
                OperationError::ProviderFailure(OperationFailure {
                    message: truncate(error.message, self.limits.max_diagnostic_bytes),
                })
            })?;
        value_budget(
            &value,
            0,
            self.limits.max_depth.min(128),
            &mut Budget::new(self.limits.max_nodes, self.limits.max_bytes),
        )
        .map_err(OperationError::InvalidOutput)?;
        match_value(&descriptor.output, &value).map_err(OperationError::InvalidOutput)?;
        session.snapshot_at(expected).map_err(document_error)?;
        Ok(OperationResult {
            document_id: snapshot.document_id,
            revision: snapshot.revision,
            value,
        })
    }
}

fn document_error(error: DocumentError) -> OperationError {
    match error {
        DocumentError::StaleRevision => OperationError::StaleRevision,
        other => OperationError::Document(other),
    }
}

fn identifier(value: &str) -> Result<(), OperationError> {
    let mut bytes = value.bytes();
    if value.len() > 128
        || !bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        || !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(OperationError::InvalidIdentifier);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Budget {
    nodes: usize,
    bytes: usize,
}
impl Budget {
    fn new(nodes: usize, bytes: usize) -> Self {
        Self { nodes, bytes }
    }
    fn take(&mut self, nodes: usize, bytes: usize) -> Result<(), OperationValidation> {
        self.nodes = self
            .nodes
            .checked_sub(nodes)
            .ok_or(OperationValidation::LimitExceeded)?;
        self.bytes = self
            .bytes
            .checked_sub(bytes)
            .ok_or(OperationValidation::LimitExceeded)?;
        Ok(())
    }
}

fn schema_budget(
    schema: &OperationSchema,
    depth: usize,
    max_depth: usize,
    budget: &mut Budget,
) -> Result<(), OperationValidation> {
    if depth > max_depth {
        return Err(OperationValidation::LimitExceeded);
    }
    budget.take(1, 0)?;
    match schema {
        OperationSchema::Array(item) => schema_budget(item, depth + 1, max_depth, budget)?,
        OperationSchema::Object { fields, required } => {
            for name in required {
                budget.take(1, name.len())?;
                if !fields.contains_key(name) {
                    return Err(OperationValidation::InvalidRequiredField);
                }
            }
            for (name, field) in fields {
                identifier(name).map_err(|_| OperationValidation::UnknownField)?;
                budget.take(0, name.len())?;
                schema_budget(field, depth + 1, max_depth, budget)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn value_budget(
    value: &ToolValue,
    depth: usize,
    max_depth: usize,
    budget: &mut Budget,
) -> Result<(), OperationValidation> {
    if depth > max_depth {
        return Err(OperationValidation::LimitExceeded);
    }
    budget.take(1, 0)?;
    match value {
        ToolValue::String(text) => budget.take(0, text.len())?,
        ToolValue::Array(items) => {
            for item in items {
                value_budget(item, depth + 1, max_depth, budget)?;
            }
        }
        ToolValue::Object(fields) => {
            for (key, value) in fields {
                budget.take(0, key.len())?;
                value_budget(value, depth + 1, max_depth, budget)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn match_object(
    fields: &BTreeMap<String, OperationSchema>,
    required: &BTreeSet<String>,
    values: &ToolArguments,
) -> Result<(), OperationValidation> {
    if required.iter().any(|key| !values.contains_key(key)) {
        return Err(OperationValidation::MissingRequiredField);
    }
    for (key, value) in values {
        let schema = fields.get(key).ok_or(OperationValidation::UnknownField)?;
        match_value(schema, value)?;
    }
    Ok(())
}

fn match_value(schema: &OperationSchema, value: &ToolValue) -> Result<(), OperationValidation> {
    match (schema, value) {
        (OperationSchema::Null, ToolValue::Null)
        | (OperationSchema::Boolean, ToolValue::Boolean(_))
        | (OperationSchema::Integer, ToolValue::Integer(_))
        | (OperationSchema::String, ToolValue::String(_)) => Ok(()),
        (OperationSchema::Array(schema), ToolValue::Array(items)) => {
            for item in items {
                match_value(schema, item)?;
            }
            Ok(())
        }
        (OperationSchema::Object { fields, required }, ToolValue::Object(values)) => {
            match_object(fields, required, values)
        }
        _ => Err(OperationValidation::TypeMismatch),
    }
}

fn truncate(mut text: String, max: usize) -> String {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempWorkspace;
    use std::cell::Cell;
    use std::rc::Rc;

    struct Neutral {
        id: String,
        descriptors: Vec<OperationDescriptor>,
        calls: Rc<Cell<usize>>,
        output: Result<ToolValue, OperationFailure>,
    }
    impl OperationProvider for Neutral {
        fn provider_id(&self) -> &str {
            &self.id
        }
        fn descriptors(&self) -> &[OperationDescriptor] {
            &self.descriptors
        }
        fn invoke(
            &self,
            _: &str,
            snapshot: &DocumentSnapshot,
            _: &ToolArguments,
        ) -> Result<ToolValue, OperationFailure> {
            assert_eq!(snapshot.text, "neutral source");
            self.calls.set(self.calls.get() + 1);
            self.output.clone()
        }
    }
    fn object() -> OperationSchema {
        OperationSchema::Object {
            fields: BTreeMap::from([("item".into(), OperationSchema::String)]),
            required: BTreeSet::from(["item".into()]),
        }
    }
    fn provider(id: &str, calls: Rc<Cell<usize>>) -> Neutral {
        Neutral {
            id: id.into(),
            calls,
            descriptors: ["inspect", "describe_item"]
                .into_iter()
                .map(|action| OperationDescriptor {
                    action_id: action.into(),
                    description: "Neutral inspection".into(),
                    input: object(),
                    output: OperationSchema::String,
                    effect: ToolEffect::Read,
                    execution: OperationExecution::Immediate,
                    authorization: BTreeSet::from(["inspect_source".into()]),
                })
                .collect(),
            output: Ok(ToolValue::String("description".into())),
        }
    }
    fn call() -> OperationCall {
        OperationCall {
            operation: OperationId {
                provider_id: "alpha".into(),
                action_id: "inspect".into(),
            },
            arguments: BTreeMap::from([("item".into(), "example".into())]),
        }
    }
    fn session(fixture: &TempWorkspace) -> DocumentSession {
        fixture.write_file("neutral.txt", "neutral source");
        DocumentSession::open(fixture.workspace(), DocumentId::new(17), "neutral.txt").unwrap()
    }

    #[test]
    fn namespaces_revision_and_call_aware_read_only_policy() {
        let calls = Rc::new(Cell::new(0));
        let mut registry = OperationRegistry::new(OperationLimits::default());
        registry
            .register(Box::new(provider("alpha", calls.clone())))
            .unwrap();
        registry
            .register(Box::new(provider("beta", calls.clone())))
            .unwrap();
        assert_eq!(registry.descriptors().count(), 4);
        assert!(matches!(
            registry.register(Box::new(provider("alpha", calls.clone()))),
            Err(OperationError::DuplicateProvider)
        ));
        let fixture = TempWorkspace::new("operations-neutral");
        fixture.write_file("neutral.txt", "neutral source");
        let mut session = DocumentSession::open(
            crate::workspace::Workspace::read_only(fixture.root().to_string_lossy()).unwrap(),
            DocumentId::new(17),
            "neutral.txt",
        )
        .unwrap();
        let snapshot = session.snapshot().unwrap();
        for namespace in ["alpha", "beta"] {
            for action in ["inspect", "describe_item"] {
                let mut call = call();
                call.operation.provider_id = namespace.into();
                call.operation.action_id = action.into();
                let result = registry
                    .invoke(&mut session, &snapshot.revision, &call, |context| {
                        context.descriptor.effect == ToolEffect::Read
                            && context.descriptor.authorization.contains("inspect_source")
                            && context.call.arguments["item"] == ToolValue::from("example")
                    })
                    .unwrap();
                assert_eq!(result.document_id, snapshot.document_id);
                assert_eq!(result.revision, snapshot.revision);
                assert_eq!(result.value, ToolValue::from("description"));
            }
        }
        assert_eq!(calls.get(), 4);
    }

    #[test]
    fn discovery_does_not_grant_write_or_execution() {
        let calls = Rc::new(Cell::new(0));
        let mut fake = provider("alpha", calls.clone());
        fake.descriptors[0].effect = ToolEffect::Write;
        fake.descriptors[1].authorization.insert("execute".into());
        let mut registry = OperationRegistry::new(OperationLimits::default());
        registry.register(Box::new(fake)).unwrap();
        assert_eq!(registry.descriptors().count(), 2);
        let fixture = TempWorkspace::new("operations-authority");
        let mut session = session(&fixture);
        let revision = session.snapshot().unwrap().revision;
        for action in ["inspect", "describe_item"] {
            let mut call = call();
            call.operation.action_id = action.into();
            assert!(matches!(
                registry.invoke(&mut session, &revision, &call, |context| {
                    context.descriptor.effect == ToolEffect::Read
                        && context
                            .descriptor
                            .authorization
                            .iter()
                            .all(|permission| permission == "inspect_source")
                }),
                Err(OperationError::Unauthorized)
            ));
        }
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn source_changes_during_invocation_invalidate_result() {
        let calls = Rc::new(Cell::new(0));
        let mut registry = OperationRegistry::new(OperationLimits::default());
        registry
            .register(Box::new(provider("alpha", calls.clone())))
            .unwrap();
        let fixture = TempWorkspace::new("operations-result-stale");
        let mut session = session(&fixture);
        let revision = session.snapshot().unwrap().revision;
        assert!(matches!(
            registry.invoke(&mut session, &revision, &call(), |_| {
                fixture.write_file("neutral.txt", "changed during admission");
                true
            }),
            Err(OperationError::StaleRevision)
        ));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn rejected_calls_never_dispatch() {
        let calls = Rc::new(Cell::new(0));
        let mut registry = OperationRegistry::new(OperationLimits::default());
        registry
            .register(Box::new(provider("alpha", calls.clone())))
            .unwrap();
        let fixture = TempWorkspace::new("operations-rejected");
        let mut session = session(&fixture);
        let revision = session.snapshot().unwrap().revision;
        let mut bad = call();
        bad.operation.action_id = "unknown".into();
        assert!(matches!(
            registry.invoke(&mut session, &revision, &bad, |_| panic!()),
            Err(OperationError::UnknownOperation)
        ));
        bad.operation.action_id = "bad.id".into();
        assert!(matches!(
            registry.invoke(&mut session, &revision, &bad, |_| panic!()),
            Err(OperationError::InvalidIdentifier)
        ));
        for arguments in [
            BTreeMap::new(),
            BTreeMap::from([("item".into(), ToolValue::Integer(1))]),
            BTreeMap::from([("extra".into(), ToolValue::Null)]),
        ] {
            bad = call();
            bad.arguments = arguments;
            assert!(matches!(
                registry.invoke(&mut session, &revision, &bad, |_| panic!()),
                Err(OperationError::InvalidInput(_))
            ));
        }
        assert!(matches!(
            registry.invoke(&mut session, &revision, &call(), |_| false),
            Err(OperationError::Unauthorized)
        ));
        fixture.write_file("neutral.txt", "changed");
        assert!(matches!(
            registry.invoke(&mut session, &revision, &call(), |_| panic!()),
            Err(OperationError::StaleRevision)
        ));
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn output_schema_payload_and_diagnostics_are_bounded() {
        let fixture = TempWorkspace::new("operations-output");
        let mut session = session(&fixture);
        let revision = session.snapshot().unwrap().revision;
        for (output, expected) in [
            (Ok(ToolValue::Integer(1)), OperationValidation::TypeMismatch),
            (
                Ok(ToolValue::String("x".repeat(100))),
                OperationValidation::LimitExceeded,
            ),
        ] {
            let mut fake = provider("alpha", Rc::new(Cell::new(0)));
            fake.output = output;
            let mut registry = OperationRegistry::new(OperationLimits {
                max_bytes: 32,
                ..OperationLimits::default()
            });
            registry.register(Box::new(fake)).unwrap();
            assert!(
                matches!(registry.invoke(&mut session, &revision, &call(), |_| true),
                Err(OperationError::InvalidOutput(reason)) if reason == expected)
            );
        }
        let mut fake = provider("alpha", Rc::new(Cell::new(0)));
        fake.output = Err(OperationFailure {
            message: "🦀".repeat(100),
        });
        let mut registry = OperationRegistry::new(OperationLimits {
            max_diagnostic_bytes: 5,
            ..OperationLimits::default()
        });
        registry.register(Box::new(fake)).unwrap();
        match registry.invoke(&mut session, &revision, &call(), |_| true) {
            Err(OperationError::ProviderFailure(error)) => assert_eq!(error.message, "🦀"),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn recursive_schema_and_atomic_metadata_limits() {
        let schema = OperationSchema::Array(Box::new(object()));
        let good = ToolValue::Array(vec![ToolValue::Object(call().arguments)]);
        assert_eq!(match_value(&schema, &good), Ok(()));
        assert_eq!(
            value_budget(&good, 0, 1, &mut Budget::new(100, 100)),
            Err(OperationValidation::LimitExceeded)
        );
        assert_eq!(
            value_budget(&good, 0, 10, &mut Budget::new(1, 100)),
            Err(OperationValidation::LimitExceeded)
        );
        let mut registry = OperationRegistry::new(OperationLimits {
            max_metadata_nodes: 1,
            ..OperationLimits::default()
        });
        for _ in 0..2 {
            assert!(matches!(
                registry.register(Box::new(provider("alpha", Rc::new(Cell::new(0))))),
                Err(OperationError::InvalidMetadata(
                    OperationValidation::LimitExceeded
                ))
            ));
            assert_eq!(registry.descriptors().count(), 0);
        }
        let mut registry = OperationRegistry::new(OperationLimits::default());
        let mut fake = provider("alpha", Rc::new(Cell::new(0)));
        fake.descriptors.push(fake.descriptors[0].clone());
        assert!(matches!(
            registry.register(Box::new(fake)),
            Err(OperationError::DuplicateAction)
        ));
        assert_eq!(registry.descriptors().count(), 0);
    }
}
