# Scope

**A development environment where humans and agents can work with a project's meaning—not just its files.**

Scope is a language-agnostic toolkit for building development environments, language tools, and constrained agent workflows. It gives projects a way to expose **discoverable, typed operations** so that a person, CLI, or agent can inspect what a project can do and use those operations without hardcoding its language or domain into the host.

Scope runs executable WebAssembly components through [Cell](https://github.com/nate-droid/cell), which provides explicit capability and resource boundaries.

> **The guiding idea:** Projects own their semantics. Scope owns discovery, typed composition, and orchestration. Cell owns execution and capability enforcement.

## Why Scope?

Most development tools treat a project primarily as source files and commands. That works, but a tool or agent often has to rediscover the project's meaning from text: What can be inspected? Which transformation is valid? What inputs does it need? What is safe to run?

Scope explores another approach: **let projects describe the operations they support.** A compiler can expose parsing and code generation. A proof system can expose verification. A simulator can expose state transitions. Scope can discover those operations and connect compatible ones without needing to understand each domain itself.

For example, a compiler project might provide:

```text
Source text → Parse → Syntax tree → Generate → WASM module
```

A proof project might provide:

```text
Proof candidates → Verify → Checked results → Fitness → Report
```

The projects define what these steps mean. Scope checks the contracts and connections; Cell executes the bound components under the host's approved policies. **Matching types does not, by itself, prove a result is correct.**

## What works today

Scope is experimental, but its foundations are usable:

- **Typed compositions.** Describe versioned types, ports, and transformation graphs; validate connections; discover compatible pipelines; and execute bound, acyclic WASM pipelines through Cell.
- **Semantic workspace inspection.** Build revision-aware structural snapshots, source references, and read-only queries without requiring a model.
- **Language and project extensions.** Register domain-specific language services and semantic tools from downstream Rust crates instead of embedding every language in Scope.
- **Tools for agents.** Discover structured tools, authorize concrete calls, and run bounded model-driven decision loops. CLI and shell agent tools are read-only by default.
- **Reviewable text proposals.** Work with bounded, approved-path snapshots; produce replacement proposals; review the before/after text; and explicitly accept or discard changes, with conflict checks.
- **Human interfaces.** Use a CLI, interactive shell, or optional experimental TUI. Registered interactive language evaluators can support stateful sessions.

Some important limits: downstream language services currently run as trusted native Rust code, not automatically as sandboxed WASM; accepting multiple proposed text changes is sequential rather than atomic; and the TUI is mainly a browsing interface, not a full editor.

## Built with Scope

These experiments live **outside** Scope. They exercise the common infrastructure without making their domain logic part of the framework.

| Project | What it explores |
| --- | --- |
| **Glyph** | Language-specific semantics through typed transformations. |
| **Proof Garden** | A proof-search pipeline with independent WASM verification and fitness components. |
| **Xpr** | Parsing and code generation as WASM components, producing executable WASM for Cell. |

The goal is to make new experiments possible by composing familiar building blocks, rather than modifying Scope for every new language or project.

## How it fits together

```text
         Human / CLI / Agent
                 │
               Scope
       discovery · types · tools
        composition · review
                 │
                Cell
       WASM runtime · capabilities
          resource policies
                 │
       Downstream components
   compilers · checkers · simulators
```

There are three distinct choices in a runnable pipeline:

1. **Composition:** What operations are connected?
2. **Binding:** Which component implements each operation?
3. **Execution policy:** What may those components access?

A composition description does **not** automatically supply executable components, inputs, or permissions. Scope can plan and validate; Cell is responsible for enforcing runtime capabilities.

## Try it

For prerequisites and repository setup, start with [Building Scope with Cell](BUILDING_WITH_CELL.md). Cell is available at [nate-droid/cell](https://github.com/nate-droid/cell), and this workspace uses its `v0.0.1` tag; a sibling Cell checkout is not required.

From the Scope workspace, explore the CLI and shell:

```sh
# Inspect the available host interfaces
cargo run -p scope-ide -- --workspace . capabilities

# Take a bounded structural snapshot
cargo run -p scope-ide -- --workspace . meta --max-entries 10000 --max-depth 64

# Open the interactive shell
cargo run -p scope-ide -- open .
```

Run a small typed-composition example:

```sh
cargo run -p scope-types --example composition --offline
cargo run -p scope-execution --example text_pipeline -- 'Hello component world'
```

The execution example connects a text transformation and word count using Cell-backed WASM components. Its text-processing semantics live in those components, not in Scope.

### Experiment with an agent

Agent integration is opt-in and uses a host-configured OpenAI-compatible model endpoint:

```sh
cargo run -p scope-ide --features agent -- \
  --workspace . agent --open README.md 'summarize this file'
```

This uses Scope's bounded tool-decision loop. **The default CLI/shell agent policy does not allow writes**, even if the workspace itself is writable. Model credentials and provider configuration stay with the host; see [Building Scope with Cell](BUILDING_WITH_CELL.md) and the [Harness Guide](HARNESS_GUIDE.md).

### Explore the experimental TUI

```sh
cargo run -p scope-ide --features tui -- tui .
```

The TUI supports browsing files and language information, navigation, and optional custom panes. Full editing and multi-document buffers are not implemented yet.

## Build on Scope

Scope is intended to be extended by other projects. A downstream language can implement `scope_language::LanguageService` and register it with Scope's CLI or app builder. Dynamic plugin loading is not yet available.

You can scaffold an independent starter language crate:

```sh
cargo run -p xtask -- new-language ../scope-calc-language \
  --extension calc --id example.calc --name Calc --with-runner
```

For the integration model and examples, read the [Downstream Guide](DOWNSTREAM_GUIDE.md). If you're interested in agent tool registration, policy, and the bounded decision loop, start with the [Harness Guide](HARNESS_GUIDE.md).

## Where it's going

The next step is a **constrained development session**: an agent discovers project operations, inspects an approved workspace snapshot, proposes a change, runs the project's registered validation tools, and presents the results for human review.

Xpr is the intended first end-to-end example. The goal is not to teach Scope how Xpr works, but to prove that a generic development workflow can use Xpr's own compiler and tests. Later, the same ideas could support more ambitious work—including using Scope to develop parts of Scope itself.

The isolated text-proposal foundation already exists. The complete compile/test/review workflow described here is **planned**, not a current end-to-end feature.

## Design philosophy

- **Projects own meaning.** Scope should not contain special cases for every compiler, prover, or simulator.
- **Authority is explicit.** Discovering an operation is not permission to execute it.
- **Types make composition inspectable.** Connections and errors should be understandable before execution where possible.
- **Humans stay in control.** Agents and human interfaces share the same underlying operations, with explicit authorization and review.
- **Build from real use cases.** Add generic abstractions when downstream projects demonstrate their value.

Scope is not aiming to replace a mature text editor, offer an unrestricted shell, or become a universal autonomous agent. It is an experiment in the smallest useful layer for **semantic tools and safely orchestrated development workflows**.

## Documentation

- [Building Scope with Cell](BUILDING_WITH_CELL.md) — setup, local development, and validation.
- [Downstream Guide](DOWNSTREAM_GUIDE.md) — embedding Scope, language integration, and isolated text proposals.
- [Harness Guide](HARNESS_GUIDE.md) — tool discovery, agent sessions, and policy.
- [Interactive Sessions](INTERACTIVE_SESSIONS.md) — registering and using interactive evaluators.
- [UX Design Charter](docs/design/UX_CHARTER.md) — design principles for human and agent interfaces.
- [`scope-meta`](crates/scope-meta/README.md), [`scope-types`](crates/scope-types/README.md), and [`scope-execution`](crates/scope-execution/README.md) — subsystem details.
- [Contributing](CONTRIBUTING.md) and [Security](SECURITY.md) — contributions and vulnerability reporting.

## License

Scope is licensed under the [MIT License](LICENSE). Dependencies, including Cell, have their own licenses.
