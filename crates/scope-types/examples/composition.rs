//! Run with `cargo run -p scope-types --example composition --offline`.
//! This example only constructs and validates plans; it executes no transformations.
use scope_types::{Capabilities, Diagnostic, Port, Registry, Transformation, TypeId};

fn ty(name: &str) -> TypeId {
    TypeId::new(name, "1")
}

fn step(name: &str, input: &str, output: &str, requirements: &[&str]) -> Transformation {
    Transformation {
        id: ty(name),
        inputs: vec![Port::new("in", ty(input))],
        outputs: vec![Port::new("out", ty(output))],
        requires: requirements.iter().map(|name| (*name).into()).collect(),
    }
}

fn main() {
    let mut registry = Registry::default();
    for declaration in [
        step("parse", "SourceText", "SyntaxTree", &[]),
        step("typecheck", "SyntaxTree", "TypedTree", &[]),
        step("compile", "TypedTree", "WasmComponent", &[]),
        step("propose", "Conjecture", "ProofCandidate", &[]),
        step(
            "verify",
            "ProofCandidate",
            "VerifiedProof",
            &["proof-checker"],
        ),
    ] {
        registry.register(declaration).expect("unique declarations");
    }
    println!(
        "Registered {} semantic transformations.",
        registry.transformations().count()
    );

    // Planning permissions are supplied independently by the host, not by declarations.
    let no_grants = Capabilities::new();
    let compiler = registry
        .discover(&ty("SourceText"), &ty("WasmComponent"), Some(&no_grants))
        .expect("compiler chain exists");
    assert!(compiler.graph.validate(&no_grants).is_empty());
    println!(
        "Successful compiler composition: {} steps, result {:?}",
        compiler.graph.nodes.len(),
        compiler.output
    );
    for (node, declaration) in &compiler.graph.nodes {
        println!(
            "  {node}: {} -> {}",
            declaration.inputs[0].ty, declaration.outputs[0].ty
        );
    }

    let mut mismatched = compiler.graph.clone();
    mismatched
        .inputs
        .insert("input".into(), TypeId::new("SourceText", "2"));
    let diagnostics = mismatched.validate(&no_grants);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic, Diagnostic::TypeMismatch { .. }))
    );
    println!("Version mismatch (invalid wiring also leaves an input missing):");
    for diagnostic in diagnostics {
        println!("  {diagnostic}");
    }

    // Unfiltered discovery finds declarations, but does not grant their requirements.
    let proof = registry
        .discover(&ty("Conjecture"), &ty("VerifiedProof"), None)
        .expect("proof chain exists");
    let diagnostics = proof.graph.validate(&no_grants);
    assert_eq!(
        diagnostics,
        vec![Diagnostic::MissingCapability {
            node: "step-1".into(),
            capability: "proof-checker".into(),
        }]
    );
    println!(
        "Unfiltered proof discovery: {} steps; validation without grants:",
        proof.graph.nodes.len()
    );
    for diagnostic in diagnostics {
        println!("  {diagnostic}");
    }
    assert!(
        registry
            .discover(&ty("Conjecture"), &ty("VerifiedProof"), Some(&no_grants))
            .is_none()
    );
    println!("Capability-filtered discovery without grants: no route.");

    let host_grants: Capabilities = ["proof-checker".into()].into();
    let permitted = registry
        .discover(&ty("Conjecture"), &ty("VerifiedProof"), Some(&host_grants))
        .expect("host permits proof checking");
    assert!(permitted.graph.validate(&host_grants).is_empty());
    println!(
        "With host planning permission: {} proof steps, result {:?}",
        permitted.graph.nodes.len(),
        permitted.output
    );
    println!("No code was executed; planning permissions are not runtime authorization.");
}
