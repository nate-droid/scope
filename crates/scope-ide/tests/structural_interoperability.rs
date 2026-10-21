#![cfg(feature = "tools")]

use scope_ide::document::session::DocumentSnapshot;
use scope_ide::tool::operations::{
    OperationDescriptor, OperationExecution, OperationFailure, OperationLimits, OperationProvider,
    OperationRegistry, OperationSchema,
};
use scope_ide::tool::{ToolArguments, ToolEffect, ToolValue};
use scope_types::{
    Connection, Diagnostic, Endpoint, Graph, Port, Registry, Source, Transformation, TypeId,
};
use std::collections::BTreeSet;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    Structural,
    Nominal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Description {
    source: &'static str,
    mode: Mode,
    identity: Option<TypeId>,
    representation: Option<OperationSchema>,
    // Opaque integration-owned facts, never interpreted as predicates by Scope.
    obligations: Vec<&'static str>,
}

// Written vocabulary: closed required text/labels, uninterpreted text and ordered
// labels; no normalization, unit, or hidden refinement promises. These modules
// intentionally repeat declarations instead of importing each other's constants.
mod producer {
    use super::{Description, Mode};
    use scope_ide::tool::operations::OperationSchema;
    use scope_types::{Port, Transformation, TypeId};

    pub(super) fn text() -> Description {
        Description {
            source: "producer:contract/text@1",
            mode: Mode::Structural,
            identity: None,
            representation: Some(OperationSchema::Object {
                fields: [
                    ("text".into(), OperationSchema::String),
                    (
                        "labels".into(),
                        OperationSchema::Array(Box::new(OperationSchema::String)),
                    ),
                ]
                .into(),
                required: ["text".into(), "labels".into()].into(),
            }),
            obligations: vec![],
        }
    }
    pub(super) fn temperature() -> Description {
        Description {
            source: "producer:temperature/milli-celsius@1",
            mode: Mode::Nominal,
            identity: Some(TypeId::new("producer.milli-celsius", "1")),
            representation: Some(OperationSchema::Integer),
            obligations: vec!["milli-Celsius; domain -273150..=1000000; exact i64"],
        }
    }
    pub(super) fn published_port() -> Transformation {
        Transformation {
            id: TypeId::new("producer.emit", "1"),
            inputs: vec![],
            outputs: vec![Port::new("out", TypeId::new("published.text-labels", "1"))],
            requires: Default::default(),
        }
    }
}

mod consumer {
    use super::{Description, Mode};
    use scope_ide::tool::operations::OperationSchema;
    use scope_types::{Port, Transformation, TypeId};

    pub(super) fn text() -> Description {
        Description {
            source: "consumer:contract/text@1",
            mode: Mode::Structural,
            identity: None,
            representation: Some(OperationSchema::Object {
                fields: [
                    (
                        "labels".into(),
                        OperationSchema::Array(Box::new(OperationSchema::String)),
                    ),
                    ("text".into(), OperationSchema::String),
                ]
                .into(),
                required: ["labels".into(), "text".into()].into(),
            }),
            obligations: vec![],
        }
    }
    pub(super) fn temperature() -> Description {
        Description {
            source: "consumer:temperature/milli-kelvin@1",
            mode: Mode::Nominal,
            identity: Some(TypeId::new("consumer.milli-kelvin", "1")),
            // Explicit fixture choice, not inferred from the word Kelvin.
            representation: Some(OperationSchema::Integer),
            obligations: vec!["milli-Kelvin; domain 0..=1273150; exact i64"],
        }
    }
    pub(super) fn published_port() -> Transformation {
        Transformation {
            id: TypeId::new("consumer.accept", "1"),
            inputs: vec![Port::new("in", TypeId::new("published.text-labels", "1"))],
            outputs: vec![],
            requires: Default::default(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    StructuralEqual,
    NominalIdentity,
    RequiresAdapter,
    PlannedAdapter(TypeId),
    Unknown(String),
    Incompatible(String),
}

#[derive(Debug)]
struct Comparison {
    // Retain full declarations, including identities, obligations and provenance.
    from: Description,
    to: Description,
    outcome: Outcome,
}

fn compare(from: &Description, to: &Description) -> Comparison {
    let outcome = if from.representation.is_none() || to.representation.is_none() {
        Outcome::Unknown("missing representation".into())
    } else if from.mode == Mode::Nominal || to.mode == Mode::Nominal {
        if from.mode == Mode::Nominal
            && to.mode == Mode::Nominal
            && from.identity.is_some()
            && from.identity == to.identity
        {
            Outcome::NominalIdentity
        } else {
            Outcome::RequiresAdapter
        }
    } else if !from.obligations.is_empty() || !to.obligations.is_empty() {
        Outcome::Unknown(format!(
            "undischarged semantic obligations: {:?} -> {:?}",
            from.obligations, to.obligations
        ))
    } else if from.representation == to.representation {
        Outcome::StructuralEqual
    } else {
        // Diagnostics only: equality above is the sole structural success rule.
        // No second recursive validator or assignability algorithm is introduced.
        let field = match (&from.representation, &to.representation) {
            (
                Some(OperationSchema::Object { fields: a, .. }),
                Some(OperationSchema::Object { fields: b, .. }),
            ) => a
                .keys()
                .chain(b.keys())
                .find(|key| a.get(*key) != b.get(*key))
                .map(String::as_str),
            _ => None,
        };
        Outcome::Incompatible(format!(
            "exact closed contract mismatch at {}",
            field.unwrap_or("root/required fields")
        ))
    };
    Comparison {
        from: from.clone(),
        to: to.clone(),
        outcome,
    }
}

fn direct_graph(from: TypeId, to: TypeId) -> Graph {
    Graph {
        inputs: [("input".into(), from)].into(),
        nodes: [(
            "sink".into(),
            Transformation {
                id: TypeId::new("host.sink", "1"),
                inputs: vec![Port::new("in", to)],
                outputs: vec![],
                requires: BTreeSet::new(),
            },
        )]
        .into(),
        connections: vec![Connection {
            from: Source::Input("input".into()),
            to: Endpoint::new("sink", "in"),
        }],
    }
}

fn adapter() -> Transformation {
    Transformation {
        id: TypeId::new("adapter.temperature", "1"),
        inputs: vec![Port::new("in", producer::temperature().identity.unwrap())],
        outputs: vec![Port::new("out", consumer::temperature().identity.unwrap())],
        requires: BTreeSet::new(),
    }
}

#[derive(Clone)]
struct Acceptance {
    declaration: Transformation,
    from: Description,
    to: Description,
    validation_reference: &'static str,
}

fn accepted_plan(
    from: &Description,
    to: &Description,
    registry: &Registry,
    evidence: Option<&Acceptance>,
) -> Comparison {
    let mut result = compare(from, to);
    if result.outcome != Outcome::RequiresAdapter {
        return result;
    }
    let Some((input, output)) = from.identity.as_ref().zip(to.identity.as_ref()) else {
        return result;
    };
    let Some(plan) = registry.discover(input, output, None) else {
        return result;
    };
    // Deliberately only this experiment's single declared adapter, not a solver.
    result.outcome = match (plan.graph.nodes.values().next(), evidence) {
        (Some(declaration), Some(e))
            if plan.graph.nodes.len() == 1
                && declaration == &e.declaration
                && from == &e.from
                && to == &e.to
                && !e.validation_reference.is_empty() =>
        {
            Outcome::PlannedAdapter(declaration.id.clone())
        }
        _ => Outcome::Unknown("missing scoped host acceptance evidence".into()),
    };
    result
}

struct InspectionOnly {
    descriptors: Vec<OperationDescriptor>,
    calls: Arc<AtomicUsize>,
}
impl OperationProvider for InspectionOnly {
    fn provider_id(&self) -> &str {
        "si2"
    }
    fn descriptors(&self) -> &[OperationDescriptor] {
        &self.descriptors
    }
    fn invoke(
        &self,
        _: &str,
        _: &DocumentSnapshot,
        _: &ToolArguments,
    ) -> Result<ToolValue, OperationFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("SI2 must not execute a provider")
    }
}

#[test]
fn structural_success_without_provider_execution() {
    let a = producer::text();
    let b = consumer::text();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = OperationRegistry::new(OperationLimits::default());
    registry
        .register(Box::new(InspectionOnly {
            descriptors: vec![OperationDescriptor {
                action_id: "describe".into(),
                description: "inspection only".into(),
                input: b.representation.clone().unwrap(),
                output: a.representation.clone().unwrap(),
                effect: ToolEffect::Read,
                execution: OperationExecution::Immediate,
                authorization: BTreeSet::new(),
            }],
            calls: calls.clone(),
        }))
        .unwrap();
    let descriptions: Vec<_> = registry.descriptors().collect();
    assert_eq!(descriptions.len(), 1);
    assert_eq!(Some(&descriptions[0].1.output), a.representation.as_ref());
    assert_eq!(Some(&descriptions[0].1.input), b.representation.as_ref());
    let result = compare(&a, &b);
    assert_eq!(result.outcome, Outcome::StructuralEqual);
    assert_eq!(result.from, a);
    assert_eq!(result.to, b);
    assert_ne!(result.from.source, result.to.source);
    assert!(result.from.obligations.is_empty() && result.to.obligations.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn shared_nominal_identity_uses_existing_graph_validation() {
    let emit = producer::published_port();
    let accept = consumer::published_port();
    let graph = Graph {
        nodes: [("producer".into(), emit), ("consumer".into(), accept)].into(),
        connections: vec![Connection {
            from: Source::Output(Endpoint::new("producer", "out")),
            to: Endpoint::new("consumer", "in"),
        }],
        ..Graph::default()
    };
    assert!(graph.validate(&BTreeSet::new()).is_empty());
}

#[test]
fn equal_integer_representations_do_not_cross_nominal_barrier() {
    let a = producer::temperature();
    let b = consumer::temperature();
    assert_eq!(a.representation, b.representation);
    assert_eq!(compare(&a, &b).outcome, Outcome::RequiresAdapter);
    let diagnostics =
        direct_graph(a.identity.unwrap(), b.identity.unwrap()).validate(&BTreeSet::new());
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d, Diagnostic::TypeMismatch { .. }))
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d, Diagnostic::MissingInput { .. }))
    );
}

#[test]
fn adapter_declaration_is_not_host_acceptance_or_conversion_proof() {
    let a = producer::temperature();
    let b = consumer::temperature();
    let declaration = adapter();
    let mut registry = Registry::default();
    registry.register(declaration.clone()).unwrap();
    let plan = registry
        .discover(
            a.identity.as_ref().unwrap(),
            b.identity.as_ref().unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(plan.graph.nodes.len(), 1);
    // Nominal planning accepts a declaration without semantic evidence.
    assert!(plan.graph.validate(&BTreeSet::new()).is_empty());
    assert_eq!(
        accepted_plan(&a, &b, &registry, None).outcome,
        Outcome::Unknown("missing scoped host acceptance evidence".into())
    );
    let evidence = Acceptance {
        declaration: declaration.clone(),
        from: a.clone(),
        to: b.clone(),
        validation_reference: "fixture-only assumption: bounded domains above; exact offset 273150; no execution/correctness proof",
    };
    assert_eq!(
        accepted_plan(&a, &b, &registry, Some(&evidence)).outcome,
        Outcome::PlannedAdapter(declaration.id)
    );
    let mut stale = evidence.clone();
    stale.to.source = "consumer:changed-publication";
    assert!(matches!(
        accepted_plan(&a, &b, &registry, Some(&stale)).outcome,
        Outcome::Unknown(_)
    ));
    let mut changed = evidence;
    changed
        .declaration
        .requires
        .insert("new.requirement".into());
    assert!(matches!(
        accepted_plan(&a, &b, &registry, Some(&changed)).outcome,
        Outcome::Unknown(_)
    ));
}

#[test]
fn missing_information_is_unknown_not_a_wildcard() {
    let a = producer::text();
    let mut b = consumer::text();
    b.representation = None;
    assert_eq!(
        compare(&a, &b).outcome,
        Outcome::Unknown("missing representation".into())
    );
    let mut kelvin = consumer::temperature();
    kelvin.representation = None;
    assert_eq!(
        compare(&producer::temperature(), &kelvin).outcome,
        Outcome::Unknown("missing representation".into())
    );
    b = consumer::text();
    b.obligations
        .push("consumer requires normalization evidence");
    assert!(
        matches!(compare(&a, &b).outcome, Outcome::Unknown(reason) if reason.contains("normalization evidence"))
    );
}

#[test]
fn closed_object_mismatch_identifies_text_field() {
    let mut b = consumer::text();
    if let Some(OperationSchema::Object { fields, .. }) = &mut b.representation {
        fields.insert("text".into(), OperationSchema::Integer);
    }
    assert_eq!(
        compare(&producer::text(), &b).outcome,
        Outcome::Incompatible("exact closed contract mismatch at text".into())
    );
}

#[test]
fn opacity_and_versions_are_not_erased() {
    let opaque = producer::temperature();
    let bare = Description {
        source: "host:bare-integer",
        mode: Mode::Structural,
        identity: None,
        representation: Some(OperationSchema::Integer),
        obligations: vec![],
    };
    assert_eq!(compare(&opaque, &bare).outcome, Outcome::RequiresAdapter);
    let mut next = opaque.clone();
    next.identity.as_mut().unwrap().version = "2".into();
    assert_eq!(compare(&opaque, &next).outcome, Outcome::RequiresAdapter);
    assert!(
        direct_graph(opaque.identity.clone().unwrap(), next.identity.unwrap())
            .validate(&BTreeSet::new())
            .iter()
            .any(|d| matches!(d, Diagnostic::TypeMismatch { .. }))
    );
    assert_eq!(compare(&opaque, &opaque).outcome, Outcome::NominalIdentity);
}

#[test]
fn inspection_and_host_acceptance_cannot_manufacture_authority() {
    let a = producer::temperature();
    let b = consumer::temperature();
    let mut declaration = adapter();
    declaration.requires.insert("temperature.execute".into());
    let mut registry = Registry::default();
    registry.register(declaration.clone()).unwrap();
    let grants = BTreeSet::new();
    let plan = registry
        .discover(
            a.identity.as_ref().unwrap(),
            b.identity.as_ref().unwrap(),
            None,
        )
        .unwrap();
    let evidence = Acceptance {
        declaration: declaration.clone(),
        from: a.clone(),
        to: b.clone(),
        validation_reference: "fixture scoped acceptance only",
    };
    assert_eq!(
        accepted_plan(&a, &b, &registry, Some(&evidence)).outcome,
        Outcome::PlannedAdapter(declaration.id)
    );
    assert_eq!(
        plan.graph.validate(&grants),
        vec![Diagnostic::MissingCapability {
            node: "step-0".into(),
            capability: "temperature.execute".into(),
        }]
    );
    assert!(
        registry
            .discover(
                a.identity.as_ref().unwrap(),
                b.identity.as_ref().unwrap(),
                Some(&grants)
            )
            .is_none()
    );
    assert!(grants.is_empty());
}
