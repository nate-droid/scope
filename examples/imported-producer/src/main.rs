//! Explicit local consumer: cargo run --manifest-path examples/imported-producer/Cargo.toml -- <candidate>
use anyhow::{Context, Result, ensure};
use scope_ide::{
    app::App,
    cli::WorkspaceConfig,
    document::{Document, DocumentId},
    registered_document_closure::{
        InvocationContract, Limits, RegisteredDocuments, TrustedLoadedArtifact,
    },
    semantic_inspection::InspectionResult,
    tool::semantic::{HostAuthorizationRequest, HostSemanticPolicy},
    tui::{RegisteredExecution, RegisteredOutput},
    workspace::Workspace,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sprout::{
    approved_bundle_call::{self as adapter, Program},
    authorized_call, bundle_call,
    contained_bundle_call::Host,
    executable::closure_discovery::SproutDiscovery,
};

mod loom;

const SHA: &str = "f1e0c70cf6b9c7744180df1d68c1a5bb393fdda6d5770e53e4aa27e7d24dc404";
struct Policy {
    allow: bool,
    calls: usize,
}
impl HostSemanticPolicy for Policy {
    fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
        self.calls += 1;
        let HostAuthorizationRequest::Closure { binding, review } = request else {
            return false;
        };
        self.allow
            && binding.snapshot().artifact_identity.contains(SHA)
            && binding
                .review()
                .is_ok_and(|r| r.as_str() == review.as_str())
    }
}
// The launcher reports these counters after exit so PTY browsing can prove it
// did not authorize or dispatch, without exposing a semantic test command.
struct LauncherPolicy(std::rc::Rc<std::cell::Cell<usize>>);
impl HostSemanticPolicy for LauncherPolicy {
    fn authorize(&mut self, request: HostAuthorizationRequest<'_>) -> bool {
        self.0.set(self.0.get() + 1);
        Policy {
            allow: true,
            calls: 0,
        }
        .authorize(request)
    }
}

fn canonical(
    completion: &adapter::InvocationResult,
) -> Result<scope_language::SemanticOperationResult, String> {
    let checked = adapter::checked_value(completion)?;
    // Apply the producer's serialization bounds without changing its canonical shape.
    sprout::call::encode(checked)?;
    let value = serde_json::to_value(checked).map_err(|e| e.to_string())?;
    Ok(scope_language::SemanticOperationResult::ReadOnly {
        value,
        rationale: "Checked imported value; computed from sealed producer sources".into(),
    })
}

fn verify_projection(
    completion: &adapter::InvocationResult,
    observation: &InspectionResult,
    items: i32,
    subtotal: i32,
    shipping: i32,
    total: i32,
) -> Result<()> {
    let checked = adapter::checked_value(completion).map_err(anyhow::Error::msg)?;
    let Some(scope_language::SemanticOperationResult::ReadOnly { value, .. }) =
        &observation.native_result
    else {
        anyhow::bail!("missing canonical native observation");
    };
    ensure!(
        *value == serde_json::to_value(checked)?,
        "canonical projection lost producer data"
    );
    for (name, expected) in [
        ("items", items),
        ("subtotal_cents", subtotal),
        ("shipping_cents", shipping),
        ("total_cents", total),
    ] {
        ensure!(
            checked
                .field(name)
                .and_then(|field| field.int32())
                .map_err(anyhow::Error::msg)?
                == expected,
            "incorrect computed {name}"
        );
    }
    ensure!(
        *checked.field("free_shipping").map_err(anyhow::Error::msg)?
            == sprout::call::Primitive::Bool(false)
    );
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let path = args
        .next()
        .context("explicit pinned candidate path required")?;
    let mut tui = false;
    let mut loom = false;
    for flag in args {
        if flag == "--tui" && !tui {
            tui = true;
        } else if flag == "--loom" && !loom {
            loom = true;
        } else {
            anyhow::bail!("expected optional --loom and --tui after candidate path");
        }
    }
    if loom {
        loom::run(std::path::Path::new(&path), tui)
    } else {
        run(std::path::Path::new(&path), tui)
    }
}

fn host(path: &std::path::Path) -> Result<std::sync::Arc<Host>> {
    let bytes = std::fs::read(path)?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == SHA,
        "candidate identity mismatch"
    );
    Ok(std::sync::Arc::new(Host::from_bytes(&bytes, path)?))
}

fn run(path: &std::path::Path, tui: bool) -> Result<()> {
    let host = host(path)?;
    let mut operation = authorized_call::operation();
    operation.id = "bundle-order-summary-v1".into();
    operation.arguments.clear();
    let contract = InvocationContract {
        schema_version: bundle_call::PROTOCOL.into(),
        entry: "order_summary".into(),
        output: "OrderSummary-v1".into(),
        effect: "read-v1".into(),
    };
    let prepare = |second: bool| -> Result<_> {
        let mut registry = RegisteredDocuments::new(
            SproutDiscovery,
            TrustedLoadedArtifact::default(),
            vec![operation.clone()],
            adapter::execution_policy(),
            Limits {
                steps: 10_000,
                recursion: 16,
                ..Limits::default()
            },
            8,
        );
        // Consume the maintained generic application sources, not copied return values.
        let sources = [
            (
                "bundle_order",
                include_str!("../../../../sprout/examples/programs/bundle_order.sprout"),
            ),
            (
                "bundle_checkout",
                include_str!("../../../../sprout/examples/programs/bundle_checkout.sprout"),
            ),
            (
                "bundle_pricing",
                include_str!("../../../../sprout/examples/programs/bundle_pricing.sprout"),
            ),
        ];
        for (i, (module, source)) in sources.iter().enumerate() {
            let source = if second && *module == "bundle_checkout" {
                ensure!(
                    source.matches("price_order(3, 1200, 500)").count() == 1,
                    "maintained checkout call changed; review second computation inputs"
                );
                source.replace("price_order(3, 1200, 500)", "price_order(5, 900, 250)")
            } else {
                (*source).to_owned()
            };
            registry
                .register(
                    (*module).into(),
                    Document::new(
                        DocumentId::new(i as u64 + 1),
                        format!("{module}.sprout"),
                        source,
                    ),
                )
                .map_err(|e| anyhow::anyhow!("register: {e:?}"))?;
        }
        let registry = adapter::prepare_bundle_call(
            registry,
            host.clone(),
            Program {
                root: "bundle_order".into(),
                operation: operation.clone(),
                contract: contract.clone(),
            },
        )?;
        Ok(registry)
    };
    let registry = prepare(false)?;
    let mut other = prepare(true)?;
    let app = App::new(Workspace::new(".")?)?;
    let mut app = app
        .try_with_registered_documents(registry)
        .map_err(|_| anyhow::anyhow!("attachment rejected"))?;
    let capture = |app: &scope_ide::app::App<_>| {
        app.capture_registered_invocation(
            "bundle_order",
            &operation.id,
            json!({}),
            contract.clone(),
        )
        .unwrap()
        .unwrap()
    };
    let mut policy = Policy {
        allow: false,
        calls: 0,
    };
    let denied = capture(&app);
    ensure!(
        app.invoke_registered_execution(denied, &mut policy)
            .unwrap()
            .is_err()
    );
    ensure!(app.registered_documents().unwrap().dispatches() == 0);
    policy.allow = true;
    let selected = capture(&app);
    let snapshot = selected.binding().snapshot().clone();
    let result = app
        .invoke_registered_execution(selected, &mut policy)
        .unwrap()
        .map_err(|e| anyhow::anyhow!("invoke: {e:?}"))?;
    ensure!(result.review().is_ok());
    ensure!(policy.calls == 2 && app.registered_documents().unwrap().dispatches() == 1);
    let value = adapter::checked_value(&result).map_err(anyhow::Error::msg)?;
    println!("Checked imported return: {value:#?}");
    println!(
        "Original binding: {snapshot:#?}\nReview: {}",
        result.review().unwrap().as_str()
    );
    let first_observation = InspectionResult::from_registered_completion(&result, canonical);

    // A separately prepared owner is not interchangeable, even with the same Wasm.
    let foreign = capture(&app);
    let calls = policy.calls;
    ensure!(other.invoke(foreign, &mut policy).is_err());
    ensure!(policy.calls == calls && other.dispatches() == 0);
    ensure!(app.registered_documents().unwrap().dispatches() == 1);
    println!(
        "Authentic prepared-registry owner/artifact substitution rejected before authorization."
    );

    // This is a fresh, authorized binding, but not the producer's prepared contract.
    let mut mismatched = contract.clone();
    mismatched.output = "NotOrderSummary-v1".into();
    let mismatched = app
        .capture_registered_invocation("bundle_order", &operation.id, json!({}), mismatched)
        .unwrap()
        .map_err(|e| anyhow::anyhow!("mismatched capture: {e:?}"))?;
    let rejected = app
        .invoke_registered_execution(mismatched, &mut policy)
        .unwrap()
        .map_err(|e| anyhow::anyhow!("mismatched authorization: {e:?}"))?;
    ensure!(rejected.output().is_err());
    ensure!(adapter::checked_value(&rejected).is_err());
    ensure!(policy.calls == calls + 1);
    ensure!(app.registered_documents().unwrap().dispatches() == 2);
    println!(
        "Mismatched contract rejected by real producer private dispatch: {:?}",
        rejected.output().as_ref().err().unwrap()
    );

    for module in ["bundle_order", "bundle_pricing"] {
        let stale = capture(&app);
        app.select_registered_document(module)
            .map_err(|e| anyhow::anyhow!("select {module}: {e:?}"))?;
        app.open_document_mut()
            .context("selected registered document missing")?
            .apply_edit(scope_ide::text::TextEdit::insert(
                scope_ide::text::Position::new(0, 0),
                " ",
            ))?;
        let calls = policy.calls;
        ensure!(
            app.invoke_registered_execution(stale, &mut policy)
                .unwrap()
                .is_err()
        );
        ensure!(policy.calls == calls);
        ensure!(app.registered_documents().unwrap().dispatches() == 2);
        ensure!(adapter::checked_value(&result).is_ok());
        println!("Stale {module} binding rejected before authorization/dispatch.");
    }
    drop(app);
    let mut second_app = App::new(Workspace::new(".")?)?
        .try_with_registered_documents(other)
        .map_err(|_| anyhow::anyhow!("second attachment rejected"))?;
    let second = second_app
        .invoke_registered_execution(capture(&second_app), &mut policy)
        .unwrap()
        .map_err(|e| anyhow::anyhow!("second invoke: {e:?}"))?;
    let second_observation = InspectionResult::from_registered_completion(&second, canonical);
    verify_projection(&result, &first_observation, 3, 3600, 500, 4100)?;
    verify_projection(&second, &second_observation, 5, 4500, 250, 4750)?;
    ensure!(
        serde_json::to_value(adapter::checked_value(&result).map_err(anyhow::Error::msg)?)?
            != serde_json::to_value(adapter::checked_value(&second).map_err(anyhow::Error::msg)?)?
    );
    ensure!(!first_observation.is_fresh_app(&second_app));
    ensure!(second_observation.is_fresh_app(&second_app));
    ensure!(second_app.registered_documents().unwrap().dispatches() == 1);
    println!(
        "Second checked imported return: {:#?}",
        adapter::checked_value(&second).map_err(anyhow::Error::msg)?
    );
    if tui {
        let authorizations = std::rc::Rc::new(std::cell::Cell::new(0));
        let dispatches = second_app.registered_documents().unwrap().dispatches();
        second_app
            .select_registered_document("bundle_order")
            .map_err(|e| anyhow::anyhow!("select second root: {e:?}"))?;
        WorkspaceConfig {
            observations: vec![first_observation, second_observation],
            registered_execution: Some(
                RegisteredExecution::new(operation.id.clone(), |completion| RegisteredOutput {
                    summary: "Imported OrderSummary".into(),
                    details: format!("{:?}", adapter::checked_value(completion)),
                })
                .with_native_feedback(canonical)
                .with_invocation_contract(contract.clone())
                .with_policy(LauncherPolicy(authorizations.clone())),
            ),
            ..WorkspaceConfig::default()
        }
        .run(&mut second_app)
        .map_err(|e| anyhow::anyhow!("workspace: {e}"))?;
        println!(
            "Launcher activity: authorizations={} dispatches={}",
            authorizations.get(),
            second_app.registered_documents().unwrap().dispatches() - dispatches
        );
    }
    ensure!(result.review().is_ok());
    ensure!(adapter::checked_value(&result).is_ok());
    println!(
        "Retained completion survives App teardown; two successful computations, one private dispatch rejection, no stale/foreign dispatches."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires explicitly pinned sibling Sprout B1 artifact"]
    fn canonical_computed_fields_and_independent_owner_evidence() -> anyhow::Result<()> {
        let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../sprout/target/b1-candidate.component.wasm");
        super::run(&artifact, false)
    }
}
