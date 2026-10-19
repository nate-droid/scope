fn main() -> std::io::Result<()> {
    scope_script::repl::run(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        &mut scope_script::host::NoHost,
    )
}
