fn main() {
    if let Err(error) = scope_ide::cli::run_from_env() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
