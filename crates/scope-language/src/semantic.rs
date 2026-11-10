//! Single-document semantic operations with no mutation or filesystem authority.
use crate::{Diagnostic, LanguageDocument, LanguageService, TextRange};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticOperationDescription {
    /// Stable within a language; not a display label.
    pub id: String,
    pub description: String,
    pub arguments: Vec<SemanticArgument>,
    pub effect: SemanticOperationEffect,
    pub target: SemanticTargetRequirement,
    /// Language-specific metadata. `scope_argument_schema` opts into the narrow
    /// shared schema subset; all other keys remain language-owned.
    /// Object key `scope_operation_purpose` is reserved for [`Self::purpose`].
    pub constraints: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticArgument {
    pub name: String,
    pub kind: SemanticArgumentKind,
    pub required: bool,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticArgumentKind {
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
    Null,
}

impl SemanticArgumentKind {
    fn accepts(self, value: &Value) -> bool {
        match self {
            Self::String => value.is_string(),
            Self::Number => value.is_number(),
            Self::Integer => value.is_i64() || value.is_u64(),
            Self::Boolean => value.is_boolean(),
            Self::Array => value.is_array(),
            Self::Object => value.is_object(),
            Self::Null => value.is_null(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticOperationEffect {
    Read,
    ProposeChanges,
    /// Pure evaluation of a domain-state proposal; never source edits or write authority.
    ProposeDomain,
}

/// Why an operation is invoked, independent of its read/proposal effect.
/// Purpose is discovery metadata, not authorization or a promise of evaluation output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticOperationPurpose {
    #[default]
    Exploration,
    CandidateEvaluation,
}

const PURPOSE_KEY: &str = "scope_operation_purpose";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticTargetRequirement {
    Document,
    Selection,
}

impl SemanticOperationDescription {
    /// Read purpose from the reserved `constraints.scope_operation_purpose` key.
    /// Absent, malformed, or unknown values default to exploration for compatibility.
    /// Neither `effect` nor evaluation-result support implies a purpose.
    pub fn purpose(&self) -> SemanticOperationPurpose {
        self.constraints
            .as_ref()
            .and_then(|value| value.get(PURPOSE_KEY))
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }

    /// Set purpose without adding a required field to existing description literals.
    /// Preserves other constraint keys; refuses non-object constraints rather than
    /// replacing opaque language data. Serialized values are `"Exploration"` and
    /// `"CandidateEvaluation"`.
    pub fn with_purpose(
        mut self,
        purpose: SemanticOperationPurpose,
    ) -> Result<Self, SemanticOperationError> {
        let constraints = self
            .constraints
            .get_or_insert_with(|| Value::Object(Default::default()))
            .as_object_mut()
            .ok_or_else(|| {
                SemanticOperationError::InvalidArguments(
                    "operation purpose requires object constraints".into(),
                )
            })?;
        constraints.insert(
            PURPOSE_KEY.into(),
            Value::String(
                match purpose {
                    SemanticOperationPurpose::Exploration => "Exploration",
                    SemanticOperationPurpose::CandidateEvaluation => "CandidateEvaluation",
                }
                .into(),
            ),
        );
        Ok(self)
    }

    /// Requires an object, rejects unknown keys, and checks required fields and
    /// top-level JSON types. Optional fields may be absent, but not implicitly null.
    /// The optional `constraints.scope_argument_schema` uses the bounded subset
    /// documented by `validate_semantic_json_schema`; other constraints stay opaque.
    pub fn validate_arguments(&self, arguments: &Value) -> Result<(), SemanticOperationError> {
        let object = arguments.as_object().ok_or_else(|| {
            SemanticOperationError::InvalidArguments("arguments must be a JSON object".into())
        })?;
        for name in object.keys() {
            if !self.arguments.iter().any(|field| field.name == *name) {
                return Err(SemanticOperationError::InvalidArguments(format!(
                    "unknown argument: {name}"
                )));
            }
        }
        for field in &self.arguments {
            match object.get(&field.name) {
                None if field.required => {
                    return Err(SemanticOperationError::InvalidArguments(format!(
                        "missing required argument: {}",
                        field.name
                    )));
                }
                Some(value) if !field.kind.accepts(value) => {
                    return Err(SemanticOperationError::InvalidArguments(format!(
                        "argument {} must have type {:?}",
                        field.name, field.kind
                    )));
                }
                _ => {}
            }
        }
        if let Some(schema) = self
            .constraints
            .as_ref()
            .and_then(|c| c.get("scope_argument_schema"))
        {
            validate_semantic_json_schema(schema, arguments)?;
        }
        Ok(())
    }
}

/// Validate a deliberately narrow JSON Schema subset (not general JSON Schema).
/// Supports typed closed objects, homogeneous arrays, enum/const, inclusive numeric
/// bounds, Unicode character string lengths and array lengths. Unknown keywords,
/// malformed schemas, and schemas deeper than 16 levels fail closed, even for absent
/// optional properties or empty arrays. No references, unions or coercion are used.
/// `number` values and bounds must be within ±(2^53−1); use `integer` for larger
/// integral quantities, whose bounds are compared exactly.
pub fn validate_semantic_json_schema(
    schema: &Value,
    value: &Value,
) -> Result<(), SemanticOperationError> {
    fn check(schema: &Value, value: Option<&Value>, depth: usize) -> Result<(), ()> {
        if depth > 16 {
            return Err(());
        }
        let s = schema.as_object().ok_or(())?;
        let kind = s.get("type").and_then(Value::as_str).ok_or(())?;
        let allowed: &[&str] = match kind {
            "object" => &["properties", "required", "additionalProperties"],
            "array" => &["items", "minItems", "maxItems"],
            "string" => &["minLength", "maxLength"],
            "number" | "integer" => &["minimum", "maximum"],
            "boolean" | "null" => &[],
            _ => return Err(()),
        };
        if s.keys().any(|k| {
            !["type", "enum", "const"].contains(&k.as_str()) && !allowed.contains(&k.as_str())
        }) {
            return Err(());
        }
        if let Some(choices) = s.get("enum") {
            let choices = choices.as_array().filter(|a| !a.is_empty()).ok_or(())?;
            if value.is_some_and(|v| !choices.contains(v)) {
                return Err(());
            }
        }
        if s.get("const")
            .is_some_and(|c| value.is_some_and(|v| v != c))
        {
            return Err(());
        }
        let accepted = match kind {
            "object" => value.is_none_or(Value::is_object),
            "array" => value.is_none_or(Value::is_array),
            "string" => value.is_none_or(Value::is_string),
            "number" => value.is_none_or(Value::is_number),
            "integer" => value.is_none_or(|v| v.is_i64() || v.is_u64()),
            "boolean" => value.is_none_or(Value::is_boolean),
            "null" => value.is_none_or(Value::is_null),
            _ => false,
        };
        if !accepted {
            return Err(());
        }
        let (min, max, actual) = match kind {
            "string" => (
                "minLength",
                "maxLength",
                value
                    .and_then(Value::as_str)
                    .map(|v| v.chars().count() as u64),
            ),
            "array" => (
                "minItems",
                "maxItems",
                value.and_then(Value::as_array).map(|v| v.len() as u64),
            ),
            _ => ("", "", None),
        };
        if !min.is_empty() {
            let lo = s.get(min).map(|v| v.as_u64().ok_or(())).transpose()?;
            let hi = s.get(max).map(|v| v.as_u64().ok_or(())).transpose()?;
            if lo.zip(hi).is_some_and(|(l, h)| l > h)
                || actual.is_some_and(|n| lo.is_some_and(|l| n < l) || hi.is_some_and(|h| n > h))
            {
                return Err(());
            }
        }
        if kind == "integer" {
            fn integer(v: &Value) -> Result<i128, ()> {
                v.as_i64()
                    .map(i128::from)
                    .or_else(|| v.as_u64().map(i128::from))
                    .ok_or(())
            }
            let lo = s.get("minimum").map(integer).transpose()?;
            let hi = s.get("maximum").map(integer).transpose()?;
            let actual = value.map(integer).transpose()?;
            if lo.zip(hi).is_some_and(|(l, h)| l > h)
                || actual.is_some_and(|n| lo.is_some_and(|l| n < l) || hi.is_some_and(|h| n > h))
            {
                return Err(());
            }
        }
        if kind == "number" {
            // Reject values outside the exact integer range before converting.
            // Large exact integral quantities should use the integer schema type.
            fn safe_number(v: &Value) -> Result<f64, ()> {
                const MAX: f64 = 9_007_199_254_740_991.0;
                let n = v.as_f64().ok_or(())?;
                if !n.is_finite() || n.abs() > MAX {
                    return Err(());
                }
                Ok(n)
            }
            let lo = s.get("minimum").map(safe_number).transpose()?;
            let hi = s.get("maximum").map(safe_number).transpose()?;
            let actual = value.map(safe_number).transpose()?;
            if lo.zip(hi).is_some_and(|(l, h)| l > h)
                || actual.is_some_and(|n| lo.is_some_and(|l| n < l) || hi.is_some_and(|h| n > h))
            {
                return Err(());
            }
        }
        if kind == "object" {
            if s.get("additionalProperties") != Some(&Value::Bool(false)) {
                return Err(());
            }
            let properties = s.get("properties").and_then(Value::as_object).ok_or(())?;
            let required = s.get("required").and_then(Value::as_array).ok_or(())?;
            let mut names = std::collections::BTreeSet::new();
            for name in required {
                let name = name.as_str().ok_or(())?;
                if !properties.contains_key(name)
                    || !names.insert(name)
                    || value.is_some_and(|v| v.get(name).is_none())
                {
                    return Err(());
                }
            }
            if value
                .and_then(Value::as_object)
                .is_some_and(|v| v.keys().any(|k| !properties.contains_key(k)))
            {
                return Err(());
            }
            for (name, child) in properties {
                check(child, value.and_then(|v| v.get(name)), depth + 1)?;
            }
        }
        if kind == "array" {
            let items = s.get("items").ok_or(())?;
            check(items, None, depth + 1)?;
            if let Some(values) = value.and_then(Value::as_array) {
                for item in values {
                    check(items, Some(item), depth + 1)?;
                }
            }
        }
        Ok(())
    }
    check(schema, Some(value), 0).map_err(|()| {
        SemanticOperationError::InvalidArguments(
            "arguments or scope_argument_schema violate the supported schema subset".into(),
        )
    })
}

/// Read-only snapshot. Results refer to this document's text and version; hosts
/// must check the revision before applying proposed edits.
#[derive(Debug, Clone)]
pub struct SemanticOperationRequest<'a> {
    pub operation_id: &'a str,
    pub document: LanguageDocument<'a>,
    /// Uses the existing half-open, UTF-8 byte-column coordinate convention.
    /// An empty range may represent a caret target.
    pub selection: Option<TextRange>,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticEdit {
    /// Relative to the original request snapshot, not preceding edits.
    pub range: TextRange,
    pub replacement: String,
}

/// Identifiable origin of expectations. Agent-authored checks are feedback, not
/// an independent specification, and cannot satisfy a required contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectationOrigin {
    User(String),
    Project(String),
    LanguageInvariant(String),
    Agent(String),
}

/// Host-configured, language-interpreted expectations; Scope never interprets
/// the opaque specification or derives tests from operation or candidate names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationContract {
    pub id: String,
    pub origin: ExpectationOrigin,
    pub checks: std::collections::BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    Passed,
    Failed,
    /// Available checks could not establish the specified result.
    Inconclusive,
    /// The evaluator could not complete the check; `actual` may carry diagnostics.
    Error,
    /// No observation was made (retained for existing integrations).
    NotEvaluated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckObservation {
    pub outcome: CheckOutcome,
    pub actual: Value,
}

/// Typed language observations for exactly the requested checks.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ContractValidation {
    pub checks: std::collections::BTreeMap<String, CheckObservation>,
}

/// Aggregate evaluation meaning, independent of execution success and effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticOutcome {
    Satisfied,
    Violated,
    Inconclusive,
}

/// Provider-owned feedback. Criterion IDs refer only to the named contract;
/// satisfaction never establishes unspecified criteria or authorizes an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticEvaluation {
    pub outcome: SemanticOutcome,
    pub summary: String,
    #[serde(default)]
    pub diagnostics: Option<Value>,
    #[serde(default)]
    pub failed_criteria: Vec<String>,
    #[serde(default)]
    pub contract_id: Option<String>,
}

/// Static validation of an inert candidate; never implies execution or application.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CandidateValidation {
    pub status: ValidationStatus,
    /// Ranges refer to candidate source. Related symbols or dependencies may use
    /// the `Diagnostic::source` document convention to identify another document.
    pub diagnostics: Vec<Diagnostic>,
    pub limitations: Vec<String>,
    pub missing_dependencies: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ValidationStatus {
    Accepted,
    AcceptedWithLimitations,
    Rejected,
    #[default]
    NotEvaluated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticBlocker {
    pub operation_id: String,
    pub category: BlockerCategory,
    pub explanation: String,
    pub recoverable: bool,
    pub missing_dependencies: Vec<String>,
    /// Ranges refer to the original document. Related symbols or dependencies
    /// may use the `Diagnostic::source` document convention.
    pub diagnostics: Vec<Diagnostic>,
    pub suggested_next_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockerCategory {
    MissingDependency,
    UnsupportedConstruct,
    UnresolvedTarget,
    UnsatisfiedPrecondition,
    InsufficientContext,
    Other(String),
}

/// Borrowed inputs for static, inert candidate validation only.
/// Edits and selection use original-document coordinates; candidate diagnostics
/// use candidate-source coordinates. This grants no execution, mutation, process,
/// filesystem, or dependency-loading authority.
#[derive(Debug, Clone)]
pub struct SemanticValidationRequest<'a> {
    pub operation_id: &'a str,
    /// Original document snapshot, not the candidate.
    pub document: LanguageDocument<'a>,
    pub candidate_source: &'a str,
    pub edits: &'a [SemanticEdit],
    pub selection: Option<TextRange>,
    pub arguments: &'a Value,
    /// Opaque host identity, not a path or an access capability.
    pub document_id: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SemanticOperationResult {
    ReadOnly {
        value: Value,
        rationale: String,
    },
    /// Read-only output accompanied by typed check observations.
    ReadOnlyEvaluation {
        value: Value,
        rationale: String,
        evaluation: ContractValidation,
    },
    /// Optional language-neutral evaluation; the opaque provider value remains authoritative.
    Evaluated {
        value: Value,
        rationale: String,
        evaluation: SemanticEvaluation,
    },
    /// Hosts own validation, review, and application of these proposals.
    Changes {
        edits: Vec<SemanticEdit>,
        rationale: String,
    },
    /// Static validation accompanies a proposal; hosts still own review and application.
    ValidatedChanges {
        edits: Vec<SemanticEdit>,
        rationale: String,
        validation: CandidateValidation,
    },
    Blocked {
        blocker: SemanticBlocker,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticOperationError {
    InvalidArguments(String),
    UnsupportedOperation(String),
    InvalidSource(Vec<Diagnostic>),
    AmbiguousTarget(String),
    InternalFailure(String),
}

impl fmt::Display for SemanticOperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments(message) => write!(f, "invalid arguments: {message}"),
            Self::UnsupportedOperation(id) => write!(f, "unsupported operation: {id}"),
            Self::InvalidSource(diagnostics) => {
                write!(f, "invalid source ({} diagnostics)", diagnostics.len())
            }
            Self::AmbiguousTarget(message) => write!(f, "ambiguous target: {message}"),
            Self::InternalFailure(message) => write!(f, "internal failure: {message}"),
        }
    }
}
impl std::error::Error for SemanticOperationError {}

/// Validated dispatch, also available to hosts independently of trait overrides.
pub fn execute_semantic_operation<S: LanguageService + ?Sized>(
    service: &S,
    request: SemanticOperationRequest<'_>,
) -> Result<SemanticOperationResult, SemanticOperationError> {
    let description = service
        .semantic_operations()
        .into_iter()
        .find(|description| description.id == request.operation_id)
        .ok_or_else(|| SemanticOperationError::UnsupportedOperation(request.operation_id.into()))?;
    description.validate_arguments(&request.arguments)?;
    if description.target == SemanticTargetRequirement::Selection && request.selection.is_none() {
        return Err(SemanticOperationError::AmbiguousTarget(
            "operation requires a selection".into(),
        ));
    }
    let result = service.perform_semantic_operation(request)?;
    if description.effect != SemanticOperationEffect::ProposeChanges
        && matches!(
            result,
            SemanticOperationResult::Changes { .. }
                | SemanticOperationResult::ValidatedChanges { .. }
        )
    {
        return Err(SemanticOperationError::InternalFailure(
            if description.effect == SemanticOperationEffect::Read {
                "read operation returned proposed changes"
            } else {
                "domain proposal operation returned source changes"
            }
            .into(),
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LanguageAnalysis, LanguagePath, Position};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn description() -> SemanticOperationDescription {
        SemanticOperationDescription {
            id: "rename".into(),
            description: "Rename target".into(),
            arguments: vec![SemanticArgument {
                name: "name".into(),
                kind: SemanticArgumentKind::String,
                required: true,
                description: "New name".into(),
            }],
            effect: SemanticOperationEffect::ProposeChanges,
            target: SemanticTargetRequirement::Selection,
            constraints: Some(json!({"identifier": true})),
        }
    }
    #[test]
    fn purpose_is_compatible_and_independent_of_effect() {
        for effect in [
            SemanticOperationEffect::Read,
            SemanticOperationEffect::ProposeChanges,
            SemanticOperationEffect::ProposeDomain,
        ] {
            let mut operation = description();
            operation.effect = effect;
            assert_eq!(operation.purpose(), SemanticOperationPurpose::Exploration);
            let operation = operation
                .with_purpose(SemanticOperationPurpose::CandidateEvaluation)
                .unwrap();
            assert_eq!(operation.effect, effect);
            assert_eq!(operation.constraints.as_ref().unwrap()["identifier"], true);
            let encoded = serde_json::to_value(&operation).unwrap();
            let decoded: SemanticOperationDescription = serde_json::from_value(encoded).unwrap();
            assert_eq!(
                decoded.purpose(),
                SemanticOperationPurpose::CandidateEvaluation
            );
            assert_eq!(
                decoded
                    .with_purpose(SemanticOperationPurpose::Exploration)
                    .unwrap()
                    .purpose(),
                SemanticOperationPurpose::Exploration
            );
        }
    }

    #[test]
    fn purpose_defaults_and_preserves_opaque_constraints() {
        for constraints in [
            None,
            Some(Value::Null),
            Some(json!([1, 2])),
            Some(json!({"scope_operation_purpose": "FuturePurpose"})),
            Some(json!({"scope_operation_purpose": 42})),
        ] {
            let mut operation = description();
            operation.constraints = constraints;
            assert_eq!(operation.purpose(), SemanticOperationPurpose::Exploration);
        }
        let mut operation = description();
        operation.constraints = None;
        assert_eq!(
            operation
                .with_purpose(SemanticOperationPurpose::CandidateEvaluation)
                .unwrap()
                .purpose(),
            SemanticOperationPurpose::CandidateEvaluation
        );
        let mut operation = description();
        operation.constraints = Some(json!([1, 2]));
        assert!(
            operation
                .clone()
                .with_purpose(SemanticOperationPurpose::CandidateEvaluation)
                .is_err()
        );
        assert_eq!(operation.constraints, Some(json!([1, 2])));
    }

    fn request(arguments: Value) -> SemanticOperationRequest<'static> {
        SemanticOperationRequest {
            operation_id: "rename",
            document: LanguageDocument::new(LanguagePath::new("main.test"), "old", 7),
            selection: Some(TextRange::new(Position::new(0, 0), Position::new(0, 3))),
            arguments,
        }
    }
    struct Service {
        calls: AtomicUsize,
        advertised: bool,
        read: bool,
    }
    impl LanguageService for Service {
        fn language_id(&self) -> &'static str {
            "test"
        }
        fn language_name(&self) -> &'static str {
            "Test"
        }
        fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
            true
        }
        fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
            LanguageAnalysis::default()
        }
        fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
            if !self.advertised {
                return vec![];
            }
            let mut description = description();
            if self.read {
                description.effect = SemanticOperationEffect::Read;
            }
            vec![description]
        }
        fn perform_semantic_operation(
            &self,
            request: SemanticOperationRequest<'_>,
        ) -> Result<SemanticOperationResult, SemanticOperationError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.document.text(), "old");
            assert_eq!(request.document.version(), 7);
            Ok(SemanticOperationResult::Changes {
                edits: vec![SemanticEdit {
                    range: request.selection.unwrap(),
                    replacement: request.arguments["name"].as_str().unwrap().into(),
                }],
                rationale: "Rename resolved target".into(),
            })
        }
    }
    #[test]
    fn descriptions_round_trip_and_validate_arguments() {
        let description = description();
        assert_eq!(
            serde_json::from_value::<SemanticOperationDescription>(
                serde_json::to_value(&description).unwrap()
            )
            .unwrap(),
            description
        );
        for invalid in [
            json!(null),
            json!([]),
            json!({}),
            json!({"name": 1}),
            json!({"name": null}),
            json!({"name": "new", "extra": true}),
        ] {
            assert!(matches!(
                description.validate_arguments(&invalid),
                Err(SemanticOperationError::InvalidArguments(_))
            ));
        }
        assert!(
            description
                .validate_arguments(&json!({"name": "new"}))
                .is_ok()
        );
    }
    #[test]
    fn every_argument_kind_and_optional_field() {
        for (kind, value) in [
            (SemanticArgumentKind::String, json!("x")),
            (SemanticArgumentKind::Number, json!(1.5)),
            (SemanticArgumentKind::Integer, json!(1)),
            (SemanticArgumentKind::Boolean, json!(true)),
            (SemanticArgumentKind::Array, json!([])),
            (SemanticArgumentKind::Object, json!({})),
            (SemanticArgumentKind::Null, Value::Null),
        ] {
            let mut description = description();
            description.arguments[0].kind = kind;
            description.arguments[0].required = false;
            assert!(description.validate_arguments(&json!({})).is_ok());
            assert!(
                description
                    .validate_arguments(&json!({"name": value}))
                    .is_ok()
            );
        }
        assert!(!SemanticArgumentKind::Integer.accepts(&json!(1.5)));
    }
    #[test]
    fn validated_trait_object_dispatch() {
        let service = Service {
            calls: AtomicUsize::new(0),
            advertised: true,
            read: false,
        };
        let dynamic: &dyn LanguageService = &service;
        assert!(
            dynamic
                .execute_semantic_operation(request(json!({})))
                .is_err()
        );
        let mut missing = request(json!({"name": "new"}));
        missing.selection = None;
        assert!(matches!(
            dynamic.execute_semantic_operation(missing),
            Err(SemanticOperationError::AmbiguousTarget(_))
        ));
        let mut unknown = request(json!({"name": "new"}));
        unknown.operation_id = "unknown";
        assert!(matches!(
            dynamic.execute_semantic_operation(unknown),
            Err(SemanticOperationError::UnsupportedOperation(_))
        ));
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            dynamic
                .execute_semantic_operation(request(json!({"name": "new"})))
                .unwrap(),
            SemanticOperationResult::Changes { .. }
        ));
        assert_eq!(service.calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn read_effect_cannot_propose_changes() {
        let service = Service {
            calls: AtomicUsize::new(0),
            advertised: true,
            read: true,
        };
        assert!(matches!(
            execute_semantic_operation(&service, request(json!({"name": "new"}))),
            Err(SemanticOperationError::InternalFailure(_))
        ));
    }
    #[test]
    fn read_effect_cannot_propose_validated_changes() {
        struct ValidatedService;
        impl LanguageService for ValidatedService {
            fn language_id(&self) -> &'static str {
                "test"
            }
            fn language_name(&self) -> &'static str {
                "Test"
            }
            fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
                true
            }
            fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
                LanguageAnalysis::default()
            }
            fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
                let mut description = description();
                description.effect = SemanticOperationEffect::Read;
                vec![description]
            }
            fn perform_semantic_operation(
                &self,
                _: SemanticOperationRequest<'_>,
            ) -> Result<SemanticOperationResult, SemanticOperationError> {
                Ok(SemanticOperationResult::ValidatedChanges {
                    edits: vec![],
                    rationale: "Static proposal".into(),
                    validation: CandidateValidation {
                        status: ValidationStatus::Accepted,
                        ..CandidateValidation::default()
                    },
                })
            }
        }
        assert!(matches!(
            execute_semantic_operation(&ValidatedService, request(json!({"name": "new"}))),
            Err(SemanticOperationError::InternalFailure(_))
        ));
    }

    #[test]
    fn domain_proposal_effect_cannot_return_source_edits() {
        struct DomainService(bool);
        impl LanguageService for DomainService {
            fn language_id(&self) -> &'static str {
                "test"
            }
            fn language_name(&self) -> &'static str {
                "Test"
            }
            fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
                true
            }
            fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
                LanguageAnalysis::default()
            }
            fn semantic_operations(&self) -> Vec<SemanticOperationDescription> {
                let mut op = description();
                op.effect = SemanticOperationEffect::ProposeDomain;
                vec![op]
            }
            fn perform_semantic_operation(
                &self,
                _: SemanticOperationRequest<'_>,
            ) -> Result<SemanticOperationResult, SemanticOperationError> {
                if self.0 {
                    Ok(SemanticOperationResult::ValidatedChanges {
                        edits: vec![],
                        rationale: "not domain output".into(),
                        validation: CandidateValidation {
                            status: ValidationStatus::Accepted,
                            ..CandidateValidation::default()
                        },
                    })
                } else {
                    Ok(SemanticOperationResult::Changes {
                        edits: vec![],
                        rationale: "not domain output".into(),
                    })
                }
            }
        }
        for validated in [false, true] {
            assert!(matches!(
                execute_semantic_operation(
                    &DomainService(validated),
                    request(json!({"name":"new"}))
                ),
                Err(SemanticOperationError::InternalFailure(_))
            ));
        }
    }

    #[test]
    fn legacy_implementation_defaults_are_object_safe() {
        struct Legacy;
        impl LanguageService for Legacy {
            fn language_id(&self) -> &'static str {
                "legacy"
            }
            fn language_name(&self) -> &'static str {
                "Legacy"
            }
            fn matches_path(&self, _: &LanguagePath<'_>) -> bool {
                true
            }
            fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
                LanguageAnalysis::default()
            }
        }
        let service: &dyn LanguageService = &Legacy;
        assert!(service.semantic_operations().is_empty());
        let original = request(json!({}));
        let validation = service
            .validate_semantic_candidate(SemanticValidationRequest {
                operation_id: original.operation_id,
                document: original.document,
                candidate_source: "new",
                edits: &[],
                selection: original.selection,
                arguments: &original.arguments,
                document_id: "opaque-host-document",
            })
            .unwrap();
        assert_eq!(validation.status, ValidationStatus::NotEvaluated);
        assert!(validation.diagnostics.is_empty());
        assert!(validation.limitations.is_empty());
        assert!(validation.missing_dependencies.is_empty());
        assert_eq!(validation.clone(), CandidateValidation::default());
        assert!(matches!(
            service.execute_semantic_operation(request(json!({}))),
            Err(SemanticOperationError::UnsupportedOperation(_))
        ));
    }
}

#[cfg(test)]
mod structured_schema_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn closed_tagged_records_and_lists() {
        let schema = json!({"type":"object","additionalProperties":false,"required":["records"],"properties":{
            "records":{"type":"array","minItems":1,"maxItems":2,"items":{
                "type":"object","additionalProperties":false,"required":["tag","name","count"],"properties":{
                    "tag":{"type":"string","const":"Quest"},
                    "name":{"type":"string","enum":["é","go"],"minLength":1,"maxLength":2},
                    "count":{"type":"integer","minimum":0,"maximum":3}
                }}}
        }});
        let valid = json!({"records":[{"tag":"Quest","name":"é","count":3}]});
        assert!(validate_semantic_json_schema(&schema, &valid).is_ok());
        for bad in [
            json!({"records":[]}),
            json!({"records":[null]}),
            json!({"records":[{"tag":"Other","name":"é","count":1}]}),
            json!({"records":[{"tag":"Quest","name":"no","count":1}]}),
            json!({"records":[{"tag":"Quest","name":"é","count":4}]}),
            json!({"records":[{"tag":"Quest","name":"é","count":1,"extra":true}]}),
            json!({"records":[{"tag":"Quest","name":"é"}]}),
            json!({"records":[],"extra":0}),
        ] {
            assert!(
                validate_semantic_json_schema(&schema, &bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn number_bounds_reject_unsafe_rounding() {
        for schema in [
            json!({"type":"number","maximum":9007199254740992u64}),
            json!({"type":"number"}),
            json!({"type":"number","minimum":-9007199254740992i64}),
        ] {
            assert!(validate_semantic_json_schema(&schema, &json!(9007199254740993u64)).is_err());
            assert!(validate_semantic_json_schema(&schema, &json!(-9007199254740993i64)).is_err());
        }
        assert!(
            validate_semantic_json_schema(
                &json!({"type":"number","maximum":9007199254740992u64}),
                &json!(0)
            )
            .is_err()
        );
        assert!(
            validate_semantic_json_schema(
                &json!({"type":"number","minimum":-1.5,"maximum":2.5}),
                &json!(2.5)
            )
            .is_ok()
        );
        assert!(
            validate_semantic_json_schema(&json!({"type":"number","maximum":2.5}), &json!(2.6))
                .is_err()
        );
        assert!(
            validate_semantic_json_schema(
                &json!({"type":"number","maximum":9007199254740991u64}),
                &json!(9007199254740991u64)
            )
            .is_ok()
        );
    }
    #[test]
    fn malformed_unused_schemas_fail_closed() {
        assert!(
            validate_semantic_json_schema(
                &json!({"type":"integer","maximum":9007199254740992u64}),
                &json!(9007199254740993u64)
            )
            .is_err()
        );
        for items in [
            json!({"type":"string","pattern":"x"}),
            json!({"type":"array"}),
            json!({"type":"string","minLength":3,"maxLength":1}),
            json!({"type":"object","properties":{},"required":[],"additionalProperties":true}),
            json!({"type":"integer","minimum":"0"}),
            json!({"type":"string","enum":[]}),
        ] {
            assert!(
                validate_semantic_json_schema(&json!({"type":"array","items":items}), &json!([]))
                    .is_err()
            );
        }
        let mut deep = json!({"type":"null"});
        for _ in 0..18 {
            deep = json!({"type":"array","items":deep});
        }
        assert!(validate_semantic_json_schema(&deep, &json!([])).is_err());
    }
}
