use scope_workbench::desktop::launch;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut root = None;
    let mut writable = false;
    let mut initial_path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--writable" => writable = true,
            "--open" if initial_path.is_none() => {
                initial_path = Some(
                    args.next()
                        .ok_or("--open requires a workspace-relative file path")?,
                )
            }
            "--help" | "-h" => {
                println!("Usage: scope-desktop [--writable] [--open FILE] [PROJECT_DIRECTORY]");
                return Ok(());
            }
            _ if !arg.starts_with('-') && root.is_none() => root = Some(arg),
            _ => return Err(format!("unexpected argument: {arg}").into()),
        }
    }
    launch(root, writable, initial_path, |project, events, publish| {
        let mut host = scope_script::workbench::WorkbenchRunner::default();
        scope_ide::tui::desktop::run(
            project.root,
            project.writable,
            project.initial_path.as_deref(),
            &mut host,
            events,
            publish,
        )
    })
}
