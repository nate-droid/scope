//! Read-only semantic scripts embedded in the owned Scope shell.
#[allow(dead_code)]
#[path = "../../scope-ide/examples/selection_description_support/mod.rs"]
mod language;
use scope_ide::{app::App, cli, language::InteractiveSession, workspace::Workspace};
use scope_script::runner::ScriptRunner;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let terminal = args.iter().any(|arg| arg == "--terminal");
    let args: Vec<_> = args.into_iter().filter(|arg| arg != "--terminal").collect();
    let (root, authorization) = match args.as_slice() {
        [root] => (root, None),
        [root, flag] if flag == "--allow-read" => (root, Some(true)),
        _ => return Err("usage: scratch_runner ROOT [--allow-read] [--terminal]".into()),
    };
    let service = language::DeclarationLanguage::default();
    let mut runner = ScriptRunner::new(&service, "describe_selection", authorization);
    let app = App::new(Workspace::read_only(root)?)?;
    if terminal {
        return cli::run_terminal_with_session_and_command(
            app,
            InteractiveSession::new(),
            &mut runner,
        );
    }
    cli::run_with_session_and_command(
        app,
        InteractiveSession::new(),
        &mut runner,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )
}
