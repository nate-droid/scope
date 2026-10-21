//! Normal downstream CLI startup with live scratch semantic-target dispatch.
#[allow(dead_code)]
#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod language;
use scope_ide::{cli, language::LanguageRegistry};
use scope_script::runner::ScriptRunner;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let allow_read = args.iter().any(|arg| arg == "--allow-read");
    args.retain(|arg| arg != "--allow-read");
    // The registry requires static services; this example installs one for process lifetime.
    let service = Box::leak(Box::new(language::DeclarationLanguage::default()));
    let registry = LanguageRegistry::builtins().with_language(service);
    let mut runner = ScriptRunner::new(service, "describe_selection", Some(allow_read));
    cli::run_with_registry_and_command(args, registry, &mut runner)
}
