# Scope

Scope is a lightweight development workbench with terminal and graphical editors. Use it to edit and run a scratch script, or open a project in a launcher that understands its language. Registered language services can add contextual actions such as Check, Format, Run, and Inspect. Where a language returns structured results, Scope can display details and compare checked returns.

## Start here

### Try the terminal workbench

From this checkout, with Rust installed, run this in an interactive terminal:

```sh
cargo run --locked
```

1. Press **F1**, choose **New Script**, and type `print(2 + 3)`.
2. Press **Ctrl-S** to publish the draft in memory, then **F5** to run it. The output pane shows `5`.
3. Press **F3** to return to the editor or **Ctrl-Q** to quit.

No model server, API key, or sibling checkout is needed. The first build fetches dependencies, including Cell. See the [terminal guide](docs/quickstart.md) for project files, explicit disk saves, and advanced actions.

### Open a desktop workspace

```sh
cargo run --locked -p scope-workbench --features desktop --bin scope-desktop -- . --open README.md
```

This opens the current directory in the graphical preview. Add `--writable` to permit disk saves. Stock Scope is not a bundle of language runtimes; language-enabled launchers supply their own services. For example, with the Desktop-enabled Sprout binary installed, run `sprout ide .` from a Sprout project to edit, format, check, and run supported entries without registering modules by hand.

## Learn more

- [Desktop guide](docs/desktop.md): project navigation, editing, execution, and result inspection and comparison.
- [Scope Script](docs/scope-script.md): scratch programs and host automation.
- [Language integration](docs/integration.md) and the [downstream example](examples/downstream/README.md): compose a launcher with your own semantic and execution services.
- [Architecture](docs/architecture.md) and [security](SECURITY.md): Scope owns workspace state and authorization; Cell enforces containment on Cell-backed execution paths. Native extensions are trusted host code. Discovering an action or accepting a proposal does not authorize execution or save source.
- [Contributing](CONTRIBUTING.md): development setup and verification.

Scope is a preview. Available actions depend on the launcher and the language; agent integration and the graphical editor are not complete IDE workflows.

[MIT license](LICENSE). Dependencies, including Cell, have their own licenses.
