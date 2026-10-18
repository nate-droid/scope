use scope_downstream_example::LANGUAGE;
use scope_ide::{app::App, workspace::Workspace};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| format!("{}/fixtures", env!("CARGO_MANIFEST_DIR")));
    let workspace = Workspace::read_only(root)?;
    let mut app = App::builder(workspace).language(&LANGUAGE).build()?;
    app.open_path("main.calc")?;

    for symbol in app.open_outline()? {
        println!("{}: {}", symbol.kind, symbol.name);
    }
    for diagnostic in app.open_diagnostics()? {
        println!("{}: {}", diagnostic.severity, diagnostic.message);
    }
    Ok(())
}
