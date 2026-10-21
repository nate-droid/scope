fn main() {
    let mut scripts = scope_script::workbench::WorkbenchRunner::default();
    if let Err(error) =
        scope_ide::cli::run_from_args_with_command(std::env::args().skip(1), &mut scripts)
    {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
