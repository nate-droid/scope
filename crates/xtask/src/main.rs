use std::env;
use std::fs;
use std::io;
use std::path::{Component, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), XtaskError> {
    match args.first().map(String::as_str) {
        Some("new-language") => new_language(&args[1..]),
        Some("check-downstream") => check_downstream(&args[1..]),
        Some("-h" | "--help") | None => {
            print_help();
            Ok(())
        }
        Some(command) => Err(XtaskError::Usage(format!(
            "unknown command `{command}`\n\n{}",
            help_text()
        ))),
    }
}

fn new_language(args: &[String]) -> Result<(), XtaskError> {
    let options = NewLanguageOptions::parse(args)?;
    let project_root = project_root();
    let target_dir = absolute_path(&options.crate_path)?;
    let manifest_path = target_dir.join("Cargo.toml");
    let src_dir = target_dir.join("src");
    let lib_path = src_dir.join("lib.rs");
    let runner_path = src_dir
        .join("bin")
        .join(format!("{}-scope.rs", options.runner_name));

    if manifest_path.exists() {
        return Err(XtaskError::Usage(format!(
            "Cargo.toml already exists: {}",
            manifest_path.display()
        )));
    }

    if lib_path.exists() {
        return Err(XtaskError::Usage(format!(
            "src/lib.rs already exists: {}",
            lib_path.display()
        )));
    }

    if options.with_runner && runner_path.exists() {
        return Err(XtaskError::Usage(format!(
            "runner already exists: {}",
            runner_path.display()
        )));
    }

    fs::create_dir_all(&src_dir)?;
    fs::write(
        &manifest_path,
        render_cargo_manifest(
            &options,
            &project_root.join("crates/scope-language"),
            &project_root.join("crates/scope-ide"),
        ),
    )?;
    fs::write(&lib_path, render_language_module(&options))?;

    println!("created {}", manifest_path.display());
    println!("created {}", lib_path.display());

    if options.with_runner {
        let runner_dir = runner_path
            .parent()
            .expect("runner path should have a parent directory");
        fs::create_dir_all(runner_dir)?;
        fs::write(&runner_path, render_scope_runner(&options))?;
        println!("created {}", runner_path.display());
    }

    Ok(())
}

fn check_downstream(args: &[String]) -> Result<(), XtaskError> {
    let offline = match args {
        [] => false,
        [flag] if flag == "--offline" => true,
        _ => {
            return Err(XtaskError::Usage(
                "Usage: cargo run -p xtask -- check-downstream [--offline]".to_owned(),
            ));
        }
    };
    let root = project_root();
    let target = match env::var_os("CARGO_TARGET_DIR") {
        Some(path) => absolute_path(std::path::Path::new(&path))?,
        None => root.join("target"),
    };
    let example = root.join("examples/downstream/Cargo.toml");
    run_cargo(&example, &target, offline, &["test", "--all-targets"])?;
    run_cargo(&example, &target, offline, &["run", "--bin", "headless"])?;
    run_cargo(
        &example,
        &target,
        offline,
        &["run", "--bin", "calc-scope", "--", "--help"],
    )?;

    let harness = root.join("examples/harness/Cargo.toml");
    run_cargo(&harness, &target, offline, &["test", "--all-targets"])?;
    run_cargo(&harness, &target, offline, &["run"])?;

    let checks = target.join("downstream-check");
    fs::create_dir_all(&checks)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| XtaskError::Usage(error.to_string()))?
        .as_nanos();
    let generated = checks.join(format!(
        "scope-smoke-language-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&generated)?;
    new_language(&[
        generated.to_string_lossy().into_owned(),
        "--package-name".to_owned(),
        "scope-smoke-language".to_owned(),
        "--extension".to_owned(),
        "smoke".to_owned(),
        "--with-runner".to_owned(),
    ])?;
    // Seed the standalone resolution with the versions already used by Scope.
    fs::copy(root.join("Cargo.lock"), generated.join("Cargo.lock"))?;
    let manifest = generated.join("Cargo.toml");
    run_cargo(&manifest, &target, offline, &["test", "--all-targets"])?;
    run_cargo(
        &manifest,
        &target,
        offline,
        &["run", "--bin", "smoke-scope", "--", "--help"],
    )?;
    fs::remove_dir_all(&generated)?;
    println!("downstream checks passed");
    Ok(())
}

fn run_cargo(
    manifest: &std::path::Path,
    target: &std::path::Path,
    offline: bool,
    args: &[&str],
) -> Result<(), XtaskError> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = std::process::Command::new(cargo);
    // Cargo options precede any arguments forwarded to the consumer binary.
    command
        .arg(args[0])
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--target-dir")
        .arg(target);
    if offline {
        command.arg("--offline");
    }
    command.args(&args[1..]);
    let status = command.status()?;
    if !status.success() {
        return Err(XtaskError::Usage(format!(
            "cargo {} failed for {} ({status})",
            args.join(" "),
            manifest.display(),
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NewLanguageOptions {
    crate_path: PathBuf,
    package_name: String,
    raw_name: String,
    struct_name: String,
    language_id: String,
    display_name: String,
    extension: String,
    with_runner: bool,
    runner_name: String,
}

impl NewLanguageOptions {
    fn parse(args: &[String]) -> Result<Self, XtaskError> {
        let mut crate_path = None;
        let mut extension = None;
        let mut language_id = None;
        let mut display_name = None;
        let mut package_name = None;
        let mut with_runner = false;
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--extension" => {
                    index += 1;
                    extension = Some(take_value(args, index, "--extension")?);
                }
                "--id" => {
                    index += 1;
                    language_id = Some(take_value(args, index, "--id")?);
                }
                "--name" => {
                    index += 1;
                    display_name = Some(take_value(args, index, "--name")?);
                }
                "--package-name" => {
                    index += 1;
                    package_name = Some(take_value(args, index, "--package-name")?);
                }
                "--with-runner" => {
                    with_runner = true;
                }
                "-h" | "--help" => {
                    return Err(XtaskError::Usage(new_language_help_text()));
                }
                option if option.starts_with('-') => {
                    return Err(XtaskError::Usage(format!(
                        "unknown option `{option}`\n\n{}",
                        new_language_help_text()
                    )));
                }
                value => {
                    if crate_path.replace(PathBuf::from(value)).is_some() {
                        return Err(XtaskError::Usage(format!(
                            "unexpected positional argument `{value}`\n\n{}",
                            new_language_help_text()
                        )));
                    }
                }
            }

            index += 1;
        }

        let crate_path = crate_path.ok_or_else(|| {
            XtaskError::Usage(format!(
                "missing crate path\n\n{}",
                new_language_help_text()
            ))
        })?;

        let inferred_package_name = crate_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty() && *name != ".")
            .ok_or_else(|| {
                XtaskError::Usage(
                    "crate path must end with a valid UTF-8 directory name".to_owned(),
                )
            })?
            .to_owned();
        let package_name = package_name.unwrap_or(inferred_package_name);
        validate_package_name(&package_name)?;

        let raw_name = package_name
            .strip_prefix("scope-")
            .unwrap_or(&package_name)
            .strip_suffix("-language")
            .unwrap_or_else(|| package_name.strip_prefix("scope-").unwrap_or(&package_name))
            .to_owned();
        validate_language_name(&raw_name)?;

        let extension = extension.unwrap_or_else(|| raw_name.replace('-', "_"));
        validate_extension(&extension)?;

        let struct_name = format!("{}Language", to_pascal_case(&raw_name));
        let display_name = display_name.unwrap_or_else(|| to_title_case(&raw_name));
        let language_id = language_id.unwrap_or_else(|| format!("example.{extension}"));
        let runner_name = raw_name.clone();

        Ok(Self {
            crate_path,
            package_name,
            raw_name,
            struct_name,
            language_id,
            display_name,
            extension,
            with_runner,
            runner_name,
        })
    }
}

fn take_value(args: &[String], index: usize, option: &str) -> Result<String, XtaskError> {
    args.get(index)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or_else(|| XtaskError::Usage(format!("missing value for `{option}`")))
}

fn validate_package_name(name: &str) -> Result<(), XtaskError> {
    let valid = !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
        && name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic());

    if valid {
        Ok(())
    } else {
        Err(XtaskError::Usage(
            "package name must start with a letter and contain only letters, numbers, '-' or '_'"
                .to_owned(),
        ))
    }
}

fn validate_language_name(name: &str) -> Result<(), XtaskError> {
    let valid = !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
        && name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic());

    if valid {
        Ok(())
    } else {
        Err(XtaskError::Usage(
            "language name must start with a letter and contain only letters, numbers, '-' or '_'"
                .to_owned(),
        ))
    }
}

fn validate_extension(extension: &str) -> Result<(), XtaskError> {
    let valid = !extension.is_empty()
        && !extension.starts_with('.')
        && extension.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        });

    if valid {
        Ok(())
    } else {
        Err(XtaskError::Usage(
            "extension must not start with '.' and must contain only letters, numbers, '-' or '_'"
                .to_owned(),
        ))
    }
}

fn render_cargo_manifest(
    options: &NewLanguageOptions,
    scope_language_path: &std::path::Path,
    scope_ide_path: &std::path::Path,
) -> String {
    let mut dependencies = format!(
        "scope-language = {{ path = \"{scope_language_path}\" }}\n",
        scope_language_path = escape_toml_string(&scope_language_path.display().to_string()),
    );

    if options.with_runner {
        dependencies.push_str(&format!(
            "scope-ide = {{ path = \"{scope_ide_path}\" }}\n",
            scope_ide_path = escape_toml_string(&scope_ide_path.display().to_string()),
        ));
    }

    format!(
        "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n# Local development paths into this checkout, not portable downstream dependencies.\n# Scope dependencies remain local paths into this checkout.\n# Cell uses workspace dependencies from https://github.com/nate-droid/cell\n# at tag v0.0.1; no sibling Cell checkout is required.\n[dependencies]\n{dependencies}\n[workspace]\n",
        package_name = options.package_name,
    )
}

fn render_scope_runner(options: &NewLanguageOptions) -> String {
    let crate_name = crate_import_name(&options.package_name);

    format!(
        "fn main() -> Result<(), Box<dyn std::error::Error>> {{\n    scope_ide::cli::run_with_languages(\n        std::env::args().skip(1),\n        [&{crate_name}::LANGUAGE as &'static dyn scope_language::LanguageService],\n    )\n}}\n",
        crate_name = crate_name,
    )
}

fn render_language_module(options: &NewLanguageOptions) -> String {
    format!(
        "use scope_language::{{LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService}};\n\npub struct {struct_name};\n\nimpl LanguageService for {struct_name} {{\n    fn language_id(&self) -> &'static str {{\n        \"{language_id}\"\n    }}\n\n    fn language_name(&self) -> &'static str {{\n        \"{display_name}\"\n    }}\n\n    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {{\n        path.extension() == Some(\"{extension}\")\n    }}\n\n    fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {{\n        analyze(document.text())\n    }}\n}}\n\npub static LANGUAGE: {struct_name} = {struct_name};\n\nfn analyze(_text: &str) -> LanguageAnalysis {{\n    // TODO:\n    // - parse input text\n    // - collect diagnostics\n    // - collect outline symbols\n    LanguageAnalysis::default()\n}}\n\n#[cfg(test)]\nmod tests {{\n    use scope_language::LanguageDocument;\n\n    use super::*;\n\n    #[test]\n    fn detects_language_by_extension() {{\n        assert!({struct_name}.matches_path(&LanguagePath::new(\"valid.{extension}\")));\n        assert!(!{struct_name}.matches_path(&LanguagePath::new(\"valid.txt\")));\n    }}\n\n    #[test]\n    fn returns_outline_for_valid_file() {{\n        let document = LanguageDocument::new(LanguagePath::new(\"valid.{extension}\"), \"\", 0);\n\n        // TODO: add a representative valid fixture and assert the expected symbols.\n        assert_eq!({struct_name}.outline(document), Vec::new());\n    }}\n\n    #[test]\n    fn reports_diagnostics_for_invalid_file() {{\n        let document = LanguageDocument::new(LanguagePath::new(\"invalid.{extension}\"), \"\", 0);\n\n        // TODO: add a representative invalid fixture and assert the expected diagnostics.\n        assert_eq!({struct_name}.diagnostics(document), Vec::new());\n    }}\n}}\n",
        struct_name = options.struct_name,
        language_id = options.language_id,
        display_name = options.display_name,
        extension = options.extension,
    )
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("xtask should live in <workspace>/crates/xtask")
        .to_owned()
}

fn absolute_path(path: &std::path::Path) -> Result<PathBuf, XtaskError> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        env::current_dir()?.join(path)
    };

    Ok(normalize_path(path))
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }

    normalized
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn crate_import_name(package_name: &str) -> String {
    package_name.replace('-', "_")
}

fn to_pascal_case(value: &str) -> String {
    split_words(value)
        .into_iter()
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn to_title_case(value: &str) -> String {
    split_words(value)
        .into_iter()
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_words(value: &str) -> Vec<&str> {
    value
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .collect()
}

fn print_help() {
    println!("{}", help_text());
}

fn help_text() -> String {
    format!(
        "Usage:\n    cargo run -p xtask -- new-language <path> --extension <extension>\n\nCommands:\n    new-language       Initialize a standalone Scope language crate\n    check-downstream   Test standalone language/harness consumers and generated runner [--offline]\n\n{}",
        new_language_help_text()
    )
}

fn new_language_help_text() -> String {
    "new-language options:\n    --extension <extension>       File extension to match, without the leading dot\n    --id <id>                     Language id, defaults to example.<extension>\n    --name <name>                 Display name, defaults to title-cased crate name\n    --package-name <name>         Package name, defaults to the target directory name\n    --with-runner                 Generate src/bin/<language>-scope.rs to launch Scope with this language"
        .to_owned()
}

#[derive(Debug)]
enum XtaskError {
    Io(io::Error),
    Usage(String),
}

impl std::fmt::Display for XtaskError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Usage(message) => formatter.write_str(message),
        }
    }
}

impl From<io::Error> for XtaskError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downstream_check_rejects_unknown_options() {
        assert!(matches!(
            check_downstream(&["--unknown".to_owned()]),
            Err(XtaskError::Usage(_)),
        ));
    }

    #[test]
    fn parses_new_language_options() {
        let options = NewLanguageOptions::parse(&[
            "../scope-calc-language".to_owned(),
            "--extension".to_owned(),
            "calc".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.crate_path, PathBuf::from("../scope-calc-language"));
        assert_eq!(options.package_name, "scope-calc-language");
        assert_eq!(options.raw_name, "calc");
        assert_eq!(options.struct_name, "CalcLanguage");
        assert_eq!(options.language_id, "example.calc");
        assert_eq!(options.display_name, "Calc");
        assert_eq!(options.extension, "calc");
        assert!(!options.with_runner);
        assert_eq!(options.runner_name, "calc");
    }

    #[test]
    fn supports_explicit_package_name() {
        let options = NewLanguageOptions::parse(&[
            "../generated".to_owned(),
            "--package-name".to_owned(),
            "calc-lang".to_owned(),
            "--extension".to_owned(),
            "calc".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.package_name, "calc-lang");
        assert_eq!(options.raw_name, "calc-lang");
        assert_eq!(options.struct_name, "CalcLangLanguage");
        assert_eq!(options.display_name, "Calc Lang");
    }

    #[test]
    fn normalizes_parent_components_in_paths() {
        assert_eq!(
            normalize_path(PathBuf::from("/repo/scope/../scope-garden-language")),
            PathBuf::from("/repo/scope-garden-language")
        );
    }

    #[test]
    fn renders_standalone_manifest() {
        let options = NewLanguageOptions::parse(&[
            "../scope-calc-language".to_owned(),
            "--extension".to_owned(),
            "calc".to_owned(),
        ])
        .unwrap();
        let manifest = render_cargo_manifest(
            &options,
            std::path::Path::new("/repo/crates/scope-language"),
            std::path::Path::new("/repo/crates/scope-ide"),
        );

        assert!(manifest.contains("name = \"scope-calc-language\""));
        assert!(manifest.contains("edition = \"2024\""));
        assert!(manifest.contains("scope-language = { path = \"/repo/crates/scope-language\" }"));
        assert!(!manifest.contains("scope-ide ="));
        assert!(manifest.contains("[workspace]"));
        assert!(manifest.contains("not portable downstream dependencies"));
        assert!(manifest.contains("Scope dependencies remain local paths"));
        assert!(manifest.contains("https://github.com/nate-droid/cell"));
        assert!(manifest.contains("tag v0.0.1"));
        assert!(manifest.contains("no sibling Cell checkout is required"));
        assert!(!manifest.contains("../cell"));
    }

    #[test]
    fn renders_runner_when_requested() {
        let options = NewLanguageOptions::parse(&[
            "../scope-calc-language".to_owned(),
            "--extension".to_owned(),
            "calc".to_owned(),
            "--with-runner".to_owned(),
        ])
        .unwrap();
        let manifest = render_cargo_manifest(
            &options,
            std::path::Path::new("/repo/crates/scope-language"),
            std::path::Path::new("/repo/crates/scope-ide"),
        );
        let runner = render_scope_runner(&options);

        assert!(options.with_runner);
        assert!(manifest.contains("scope-ide = { path = \"/repo/crates/scope-ide\" }"));
        assert!(runner.contains("scope_ide::cli::run_with_languages"));
        assert!(runner.contains("&scope_calc_language::LANGUAGE"));
    }

    #[test]
    fn renders_standalone_language_service_api() {
        let options = NewLanguageOptions::parse(&[
            "../scope-calc-language".to_owned(),
            "--extension".to_owned(),
            "calc".to_owned(),
        ])
        .unwrap();
        let module = render_language_module(&options);

        assert!(module.contains("use scope_language::{LanguageAnalysis, LanguageDocument, LanguagePath, LanguageService};"));
        assert!(
            module
                .contains("fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis")
        );
        assert!(module.contains("fn analyze(_text: &str) -> LanguageAnalysis"));
        assert!(!module.contains("fn diagnostics("));
        assert!(!module.contains("fn outline("));
        assert!(module.contains("pub struct CalcLanguage;"));
        assert!(module.contains("pub static LANGUAGE: CalcLanguage = CalcLanguage;"));
        assert!(module.contains("fn language_id(&self) -> &'static str"));
        assert!(module.contains("fn matches_path(&self, path: &LanguagePath<'_>)"));
        assert!(module.contains("path.extension() == Some(\"calc\")"));
        assert!(
            module.contains("LanguageDocument::new(LanguagePath::new(\"valid.calc\"), \"\", 0)")
        );
        assert!(!module.contains("crate::document::Document"));
        assert!(!module.contains("crate::language"));
        assert!(!module.contains("std::fs"));
        assert!(!module.contains("wit_bindgen"));
    }
}
