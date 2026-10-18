fn main() -> Result<(), Box<dyn std::error::Error>> {
    scope_ide::cli::run_with_languages(
        std::env::args().skip(1),
        [&scope_downstream_example::LANGUAGE as &'static dyn scope_language::LanguageService],
    )
}
