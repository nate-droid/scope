fn main() {
    let args = public_args(std::env::args().skip(1));
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => {
            print_help();
            return;
        }
        [flag] if flag == "--version" => {
            println!("scope {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {}
    }
    if let Err(error) = run(args) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let options = LauncherOptions::parse(args)?;
    let mut scripts = scope_script::workbench::WorkbenchRunner::default();
    #[cfg(feature = "agent")]
    {
        if options.local_binding {
            scripts = scripts.with_agent_reads();
        }
        if options.allow_edit {
            return scope_ide::cli::run_with_registry_and_command_and_editing(
                options.args,
                scope_ide::language::LanguageRegistry::builtins(),
                &mut scripts,
                scope_script::workbench_agent::editing_config(),
            );
        }
    }
    scope_ide::cli::run_from_args_with_command(options.args, &mut scripts)
}

/// Consume only launcher-owned authorization options before the command. Values
/// (including request text) are never scanned for authorization-looking strings.
struct LauncherOptions {
    args: Vec<String>,
    #[cfg(feature = "agent")]
    local_binding: bool,
    #[cfg(feature = "agent")]
    allow_edit: bool,
}
impl LauncherOptions {
    fn parse(args: Vec<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut forwarded = Vec::new();
        let (mut allow_read, mut allow_edit, mut local_binding) = (false, false, false);
        let mut args = args.into_iter();
        let mut has_command = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--allow-read" => allow_read = true,
                "--allow-edit" => allow_edit = true,
                "--agent-local-binding" => {
                    local_binding = true;
                    forwarded.push(arg);
                }
                "-w"
                | "--workspace"
                | "--agent-model"
                | "--agent-base-url"
                | "--agent-api-key-env" => {
                    forwarded.push(arg);
                    if let Some(value) = args.next() {
                        forwarded.push(value);
                    } else {
                        return Err("missing value for launcher option".into());
                    }
                }
                _ if arg.starts_with('-') => forwarded.push(arg),
                _ => {
                    has_command = true;
                    forwarded.push(arg);
                    forwarded.extend(args);
                    break;
                }
            }
        }
        if allow_read || allow_edit || local_binding {
            if !cfg!(feature = "agent") {
                return Err(
                    "Script agent support requires cargo run --features agent -- ...".into(),
                );
            }
            if !local_binding || !allow_read {
                return Err("Script agent requires --agent-local-binding and explicit --allow-read; --allow-edit separately authorizes proposals".into());
            }
        }
        if !has_command {
            forwarded.push(
                if cfg!(feature = "tui") {
                    "workspace"
                } else {
                    "shell"
                }
                .into(),
            );
        }
        Ok(Self {
            args: forwarded,
            #[cfg(feature = "agent")]
            local_binding,
            #[cfg(feature = "agent")]
            allow_edit,
        })
    }
}

// Only the public binary selects a different default; explicit invocations and
// downstream users of scope-ide keep their existing dispatch and defaults.
fn public_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut args: Vec<_> = args.into_iter().collect();
    if args.is_empty() {
        args.push(
            if cfg!(feature = "tui") {
                "workspace"
            } else {
                "shell"
            }
            .into(),
        );
    }
    args
}

fn print_help() {
    println!("Usage: scope [OPTIONS] [COMMAND]\n       scope --version\n");
    #[cfg(feature = "tui")]
    println!(
        "No arguments: open the workspace editor in the current directory.\n\
         F1 actions: choose New Script scratch, then type Script in the editor.\n\
         Ctrl-S commits the draft in memory (not to disk).\n\
         F5 runs committed selected Script; F2 opens the Scope shell, then returns\n\
         to the workspace when you leave the shell. Ctrl-Q exits the workspace.\n"
    );
    #[cfg(not(feature = "tui"))]
    println!(
        "No arguments: start the Scope shell in the current directory.\n\
         This build has no TUI; the workspace editor requires feature `tui`.\n"
    );
    println!(
        "Public commands:\n\
         \x20 shell, open [ROOT]  Start the Scope shell (supports piped input)\n\
         \x20 -h, --help          Show this help\n\
         \x20 --version           Show the public binary version\n\n\
         Developer commands (existing CLI):\n\
         \x20 list                List workspace files\n\
         \x20 show PATH           Print a workspace-relative file\n\
         \x20 capabilities        List installed interfaces\n\
         \x20 authority           Show advisory authority metadata\n\
         \x20 meta                Bounded read-only metadata graph\n\
         \x20 agent, ask          Ask the agent (requires feature `agent`)"
    );
    #[cfg(feature = "tui")]
    println!(
        "  workspace [ROOT]    Open the workspace editor\n  tui [ROOT]          Open the developer TUI"
    );
    println!(
        "\nOptions (before the command):\n\
         \x20 -w, --workspace ROOT  Workspace root (default: current directory)\n\
         \x20 --writable            Opt in to explicit workspace writes\n\
         \x20 --agent-local-binding  Opt in to selected Script agent (feature agent)\n\
         \x20 --agent-model NAME     Explicit model required for that binding\n\
         \x20 --agent-base-url URL   Explicit loopback OpenAI-compatible endpoint\n\
         \x20 --allow-read           Independently authorize Script source reads\n\
         \x20 --allow-edit           Also authorize reviewed, syntax-checked proposals\n\
         Agent flags precede the command. They grant no execution or filesystem writes.\n\
         Proposal checks establish syntax only, not task correctness.\n\n\
         meta options: --max-entries N (positive), --max-depth N (zero allowed).\n\
         A workspace argument without a command retains the file-listing default.\n\
         Workspaces must already exist; access defaults to read-only.\n\
         Pure Script has no native authority. Shell discovery: help, files, quit.\n\
         Standalone file execution: cargo run -p scope-script -- --help"
    );
}

#[cfg(test)]
mod tests {
    use super::public_args;

    #[cfg(feature = "agent")]
    #[test]
    fn agent_authorizations_are_explicit_separate_and_not_read_from_values() {
        use super::LauncherOptions;
        let parse = |args: &[&str]| {
            LauncherOptions::parse(args.iter().map(|arg| arg.to_string()).collect())
        };
        for args in [
            vec!["--allow-read", "shell"],
            vec!["--allow-edit", "shell"],
            vec!["--agent-local-binding", "shell"],
            vec!["--agent-local-binding", "--allow-edit", "shell"],
            vec![
                "--agent-model",
                "--allow-read",
                "--agent-local-binding",
                "shell",
            ],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        let read = parse(&["--agent-local-binding", "--allow-read", "shell"]).unwrap();
        assert!(read.local_binding);
        assert!(!read.allow_edit);
        assert!(!read.args.iter().any(|arg| arg == "--allow-read"));
        let edit = parse(&[
            "--agent-local-binding",
            "--allow-read",
            "--allow-edit",
            "shell",
        ])
        .unwrap();
        assert!(edit.allow_edit);
        let request = parse(&["shell", "--allow-read", "--allow-edit"]).unwrap();
        assert!(!request.local_binding);
        assert!(!request.allow_edit);
        assert_eq!(request.args, ["shell", "--allow-read", "--allow-edit"]);
        let default = parse(&["--agent-local-binding", "--allow-read"]).unwrap();
        assert_eq!(
            default.args.last().unwrap(),
            if cfg!(feature = "tui") {
                "workspace"
            } else {
                "shell"
            }
        );
    }

    #[cfg(not(feature = "agent"))]
    #[test]
    fn agent_authorization_requires_compile_time_feature() {
        assert!(super::LauncherOptions::parse(vec!["--allow-read".into()]).is_err());
    }

    #[test]
    fn public_default_selects_existing_command() {
        let expected = if cfg!(feature = "tui") {
            "workspace"
        } else {
            "shell"
        };
        assert_eq!(public_args([]), [expected]);
    }

    #[test]
    fn explicit_arguments_are_unchanged() {
        for args in [
            vec!["shell"],
            vec!["workspace", "some-root"],
            vec!["--writable", "shell", "some-root"],
            vec!["some-root"],
            vec!["--help"],
            vec!["--version"],
        ] {
            let args: Vec<String> = args.into_iter().map(str::to_owned).collect();
            assert_eq!(public_args(args.clone()), args);
        }
    }
}
