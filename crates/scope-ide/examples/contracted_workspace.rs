//! Downstream adapter: keep the existing registry/command host, add the host's
//! explicit editing contract. No mutable proposal access enters the callback.
use scope_ide::{
    cli::{self, InteractiveEditConfig, ReadOnlyCommand},
    language::LanguageRegistry,
    scratch::ScratchBuffer,
};

/// Path/Hilb supplies its own language service, contract, operation allowlist and
/// concrete-call authorization in `editing`, independently of read-only policy.
pub fn launch(
    args: impl IntoIterator<Item = String>,
    registry: LanguageRegistry,
    host: &mut dyn ReadOnlyCommand,
    editing: InteractiveEditConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    cli::run_with_registry_and_command_and_editing(args, registry, host, editing)
}

/// A downstream read-only callback can inspect the original committed scratch
/// after human acceptance. Domain validation belongs to the language host.
pub fn independently_validate(
    original: &ScratchBuffer,
    validate: impl FnOnce(&str) -> bool,
) -> (u64, scope_language::SourceId, u64, bool) {
    (
        original.id(),
        original.evaluation_source_id().clone(),
        original.version(),
        validate(original.text()),
    )
}

fn main() {
    println!(
        "Host adapter example; supply your existing registry, ReadOnlyCommand and InteractiveEditConfig to launch(). See docs/CONTRACTED_WORKSPACE_EDITING.md."
    );
}
