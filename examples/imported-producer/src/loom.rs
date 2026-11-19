//! Immutable, opt-in Loom computation using the ordinary imported producer bridge.
use super::*;

const ROOT: &str = "loom_cz";
const SOURCE: &str = include_str!("loom_cz.sprout");

pub(super) fn run(path: &std::path::Path, tui: bool) -> Result<()> {
    let host = host(path)?;
    let mut operation = authorized_call::operation();
    operation.id = "loom-cz-amplitudes-v1".into();
    operation.arguments.clear();
    let contract = InvocationContract {
        schema_version: bundle_call::PROTOCOL.into(),
        entry: "cz_amplitudes".into(),
        output: "CzAmplitudes-v1".into(),
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
        // Acquire only the maintained mathematical closure, byte-for-byte. No
        // directory discovery, generated kernel, or fallback producer is used.
        let loom = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../loom");
        let source = if second {
            SOURCE.replace(
                "let decoded = effects.effect_decode(first.state);",
                "let second = effects.effect_apply(first.state, 2, 0, 1);\n    let decoded = effects.effect_decode(second.state);",
            )
        } else {
            SOURCE.to_owned()
        };
        for (i, (module, source)) in [
            (ROOT, source),
            (
                "effects",
                std::fs::read_to_string(loom.join("effects.sprout"))?,
            ),
            (
                "common",
                std::fs::read_to_string(loom.join("common.sprout"))?,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            registry
                .register(
                    module.into(),
                    Document::new(
                        DocumentId::new(i as u64 + 1),
                        format!("{module}.sprout"),
                        source,
                    ),
                )
                .map_err(|e| anyhow::anyhow!("Loom register: {e:?}"))?;
        }
        adapter::prepare_bundle_call(
            registry,
            host.clone(),
            Program {
                root: ROOT.into(),
                operation: operation.clone(),
                contract: contract.clone(),
            },
        )
    };
    let capture = |app: &scope_ide::app::App<_>| {
        app.capture_registered_invocation(ROOT, &operation.id, json!({}), contract.clone())
            .context("missing Loom registry")?
            .map_err(|e| anyhow::anyhow!("Loom capture: {e:?}"))
    };
    let mut observations = Vec::new();
    let mut latest = None;
    for second in [false, true] {
        let mut app = App::new(Workspace::new(".")?)?
            .try_with_registered_documents(prepare(second)?)
            .map_err(|_| anyhow::anyhow!("Loom attachment rejected"))?;
        app.select_registered_document(ROOT)
            .map_err(|e| anyhow::anyhow!("Loom select: {e:?}"))?;
        let mut policy = Policy {
            allow: false,
            calls: 0,
        };
        ensure!(
            app.invoke_registered_execution(capture(&app)?, &mut policy)
                .unwrap()
                .is_err()
        );
        ensure!(policy.calls == 1 && app.registered_documents().unwrap().dispatches() == 0);
        policy.allow = true;
        let result = app
            .invoke_registered_execution(capture(&app)?, &mut policy)
            .unwrap()
            .map_err(|e| anyhow::anyhow!("Loom invoke: {e:?}"))?;
        let checked = adapter::checked_value(&result).map_err(anyhow::Error::msg)?;
        let expected = [1, 1, 1, if second { 1 } else { -1 }, 2];
        for (name, expected) in [
            "numerator_00",
            "numerator_01",
            "numerator_10",
            "numerator_11",
            "sqrt2_exponent",
        ]
        .into_iter()
        .zip(expected)
        {
            ensure!(
                checked
                    .field(name)
                    .and_then(|v| v.int32())
                    .map_err(anyhow::Error::msg)?
                    == expected,
                "incorrect Loom {name}"
            );
        }
        let observation = InspectionResult::from_registered_completion(&result, canonical);
        let Some(scope_language::SemanticOperationResult::ReadOnly { value, .. }) =
            &observation.native_result
        else {
            anyhow::bail!("missing Loom native feedback");
        };
        ensure!(
            *value == serde_json::to_value(checked)?,
            "Loom projection lost checked data"
        );
        ensure!(observation.is_fresh_app(&app));
        ensure!(policy.calls == 2 && app.registered_documents().unwrap().dispatches() == 1);
        println!(
            "Loom {} CZ checked return: {checked:#?}",
            if second { 2 } else { 1 }
        );
        println!(
            "Loom source binding: {:#?}",
            capture(&app)?.binding().snapshot()
        );
        observations.push(observation);
        latest = Some(app);
    }
    let mut app = latest.unwrap();
    ensure!(!observations[0].is_fresh_app(&app));
    ensure!(observations[1].is_fresh_app(&app));
    println!(
        "Loom: two independent checked computations; exact canonical projections; denied authorization had zero dispatches; historical/current provenance verified."
    );
    if tui {
        let authorizations = std::rc::Rc::new(std::cell::Cell::new(0));
        let dispatches = app.registered_documents().unwrap().dispatches();
        WorkspaceConfig {
            observations,
            registered_execution: Some(
                RegisteredExecution::new(operation.id.clone(), |completion| RegisteredOutput {
                    summary: "Imported Loom CZ amplitudes".into(),
                    details: format!("{:?}", adapter::checked_value(completion)),
                })
                .with_native_feedback(canonical)
                .with_invocation_contract(contract)
                .with_policy(LauncherPolicy(authorizations.clone())),
            ),
            ..WorkspaceConfig::default()
        }
        .run(&mut app)
        .map_err(|e| anyhow::anyhow!("workspace: {e}"))?;
        println!(
            "Launcher activity: authorizations={} dispatches={}",
            authorizations.get(),
            app.registered_documents().unwrap().dispatches() - dispatches
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires explicitly pinned sibling Sprout B1 artifact and maintained Loom sources"]
    fn checked_loom_cz_observations() -> Result<(), anyhow::Error> {
        super::run(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../sprout/target/b1-candidate.component.wasm"),
            false,
        )
    }
}
