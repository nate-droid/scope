use scope_ide::{
    cli::{self, ReadOnlyCommand},
    document::Document,
    language::LanguageRegistry,
    text::TextRange,
};
use std::io::Write;

struct UnexpectedCommand;

impl ReadOnlyCommand for UnexpectedCommand {
    fn invoke(
        &mut self,
        _document: &Document,
        _selection: TextRange,
        _operation: &str,
        _output: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        panic!("startup and non-shell commands must not invoke the host callback");
    }
}

#[test]
fn help_skips_workspace_validation_with_either_entry_point() {
    let args = ["--workspace", "", "--help"].map(String::from);
    cli::run_with_registry_and_command(
        args.clone(),
        LanguageRegistry::builtins(),
        &mut UnexpectedCommand,
    )
    .unwrap();
    cli::run_from_args_with_command(args, &mut UnexpectedCommand).unwrap();
}

#[test]
fn argument_errors_match_existing_entry_point() {
    let args = ["--not-a-scope-option"].map(String::from);
    let error = cli::run_with_registry_and_command(
        args.clone(),
        LanguageRegistry::builtins(),
        &mut UnexpectedCommand,
    )
    .unwrap_err();
    let existing = cli::run_from_args_with_command(args, &mut UnexpectedCommand).unwrap_err();
    assert_eq!(error.to_string(), existing.to_string());
}

#[test]
fn non_shell_command_uses_normal_startup_without_invoking_callback() {
    cli::run_with_registry_and_command(
        ["--workspace", env!("CARGO_MANIFEST_DIR"), "list"].map(String::from),
        LanguageRegistry::builtins(),
        &mut UnexpectedCommand,
    )
    .unwrap();
}
