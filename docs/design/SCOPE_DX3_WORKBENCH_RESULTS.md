# DX3 — One-command workbench results

## Before and after

Before: the virtual Cargo workspace had no default member, so bare `cargo run`
could not choose among IDE, Script, and developer executables. Documentation
mixed onboarding, duplicated commands, completed milestones, and live-model
experiments. CLI workspace mounts ordinarily allowed writes and could initialize
missing roots through the workspace constructor.

After: from the repository root, run:

```sh
cargo run
```

This selects the existing `scope-ide` binary and opens its interactive shell.
No wrapper executable was added. The current directory is visibly selected and
mounted read-only; it must already exist. Startup does not create a workspace.
Use `cargo run -- --workspace ./playground shell` for another existing directory.
An explicit workspace without a command retains the older file-list behavior.
Writes require an explicit opt-in, e.g. `cargo run -- --writable shell`.
This is a deliberate compatibility change for callers previously relying on
implicit write authority. `meta` remains read-only even with that option.

## Entry-point inventory and actual defaults

| Entry point / boundary | Audited behavior |
| --- | --- |
| Root Cargo workspace | `default-members = ["crates/scope-ide"]`; no ambiguous default executable |
| `scope-ide` | Canonical CLI and shell; no arguments opens shell; `open`/`shell` aliases remain |
| `scope-script` | Separate saved-file interpreter and stateful no-argument REPL |
| `xtask` | Contributor scaffolding and downstream checks, not a workbench |
| Optional IDE TUI | `--features tui -- tui .`; browsing rather than a full editor |
| Default feature | `tools`; installed tools are not permission to write or invoke semantic operations |
| Default analysis | `scope.simple` (Scope Simple, `.scope`) and `scope.neutral` fallback; no dedicated built-in Rust service |
| Default evaluator / semantic operations | No interactive evaluators and no language semantic operations installed |
| Registration | `LanguageRegistry::builtins` plus explicit `run_with_languages`/App builder registration; evaluator/session registration is separate |
| Filesystem | Existing selected root, read-only by default; explicit `--writable` opts into within-root writes |
| Models / network | No default model backend invocation, network grant, plugin discovery, or downstream runtime installation |
| Agent feature | Opt-in backend integration; CLI agent tool policy still denies writes |
| Script file execution | Fresh full-buffer interpreter with `NoHost`; reading the supplied source file does not grant scripts filesystem access |
| History | Existing `SCOPE_HISTORY_FILE` configuration is opt-in persistence; no implicit history file added |

Startup points to `languages`, `:scope session`, `:scope context`, and help.
`languages` lists analysis services, installed semantic operations, evaluator
providers, and the active session. `:scope languages` remains observational during
an active session. `authority` reports advisory policy, not authorization tokens.
`:scope inspect` discovers eligible App-target reads. Scratch/source editing is
in memory; evaluation requires a registered evaluator. The standalone Script
command is advertised separately rather than pretending it is installed in IDE.

## Documentation change group

Canonical reading path:

- [README](../../README.md): purpose, one command, five-minute Script workflow.
- [Getting started](../getting-started.md): exact CLI forms, discovery, source owners, editing and evaluation limits.
- [Scope Script](../scope-script.md): saved files, structured values/conditionals, output/errors, explicit host example.
- [Architecture](../architecture.md): crate boundaries, authority, source identity, composition and current limits.
- [Development](../development.md): toolchain, Cell **v0.0.2**, feature flags, build/test/check commands.
- [Example catalog](../../examples/README.md): runnable demos versus fixture hosts and historical experiments.

`BUILDING_WITH_CELL.md` is a compatibility pointer with build essentials;
`crates/scope-script/README.md` points to canonical usage while preserving unique
embedding/trust details. Existing downstream, harness, interactive-session,
language-designer, security, and subsystem references remain available.

### Moved and preserved

The following 17 Markdown records moved from `docs/design/` to `docs/archive/`
with their names preserved:

- `FOCUSED_AGENT_DISCOVERY_RESULTS.md`
- `INTERACTIVE_SCRIPT_WORKBENCH_DISCOVERY.md`
- `Q1_SINGLE_DECISION_COMPATIBILITY.md`
- `Q2_LIVE_QWEN_PROTOCOL.md`
- `Q3_SEQUENTIAL_TOOL_SERVING.md`
- `RICH_COMPLETION_RESULTS.md`
- `SCOPE_SCRIPT_AUTHORING_AND_EXPERIMENTS.md`
- `SCOPE_SCRIPT_DX1_RESULTS.md`
- `SCOPE_SCRIPT_DX2_RESULTS.md`
- `SCOPE_SCRIPT_SS2_RESULTS.md`
- `SCOPE_SCRIPT_SS3_RESULTS.md`
- `SCOPE_SCRIPT_SS4_B_RESULTS.md`
- `SCOPE_SCRIPT_SS4_TRIAL_RUNNER.md`
- `STRUCTURAL_INTEROPERABILITY_DISCOVERY.md`
- `STRUCTURAL_INTEROPERABILITY_RESULTS.md`
- `STRUCTURAL_INTEROPERABILITY_SUMMARY.md`
- `TERMINAL_INPUT_INTEGRATION_T1.md`

Also moved `FOCUSED_AGENT_DISCOVERY_TRANSCRIPT.json` and complete `q2-live/` and
`q3-live/` evidence directories. All 39 non-Markdown evidence files were compared
byte-for-byte with HEAD. Research text is retained, with relative-link repairs
and explicit labels where historical citations refer to absent artifacts.

[Archive index](../archive/README.md) records placement and evidence policy;
[active design index](README.md) identifies retained ownership, authority,
terminal, inspection, and trace contracts. No implementation, example, fixture,
or test was deleted. Source/test references were checked before moving records;
none consumed the moved files. Relative Markdown references were rebased, not
redirected to unrelated evidence. No additional milestone report was created.

### Examples

The catalog distinguishes standalone pure scripts, explicit native host examples,
regression fixtures/shared data, and historical live-model capture programs.
`structured-report.scope` needs a semantic host and is not advertised as a
standalone no-host script. Path/Forth trial material stays historical/host-specific.
Live Q2/Q3 and M2 hosts require configured backends and can send source externally;
they are not onboarding or automatic validation commands. No new runner was added.

## Validation

Validated on this existing macOS checkout using the declared Rust toolchain and
cached Cargo dependencies. **This is not clean-checkout, empty-cache, first-network-
download, or hosted CI verification.** The build uses the declared locked Cell
v0.0.2 source (resolved revision prefix `86145d1e`), not a sibling checkout.

| Check | Result |
| --- | --- |
| `cargo build --workspace --locked --offline` | Passed |
| Root no-argument `cargo run --locked --offline` with shell input | Passed; intended IDE, read-only authority |
| CLI and Script `--help` | Passed |
| `cargo test --workspace --locked --offline` | Passed after final runtime changes |
| `cargo test --workspace --all-features --locked --offline` | Passed after final runtime changes |
| Runtime CLI integration suite | 6 passed; startup/no creation, discovery, compatibility, denied save and explicit writable save |
| `python3 crates/scope-ide/tests/pty_shell.py` | 7 passed against actual binary after final build |
| `cargo fmt --all -- --check` | Passed |
| Markdown checker unit tests | 5 passed |
| Working-tree local Markdown link check | Passed; not remote URLs or heading anchors |
| Standalone `daily.scope` | Passed; expected daily/ok/12 record |
| Daily summary output | Passed |
| Semantic example with `--allow-read` | Passed |
| Same semantic example without grant | Expected nonzero denial with source location and denied operation |
| Newly authored temporary Script record/array/conditional | Passed, printed `"DX3"` |
| Newly authored malformed Script | Expected nonzero, readable file/line/column/byte syntax error |

The link checker originally failed on unstaged moved paths and omitted new docs.
It now enumerates non-ignored tracked and untracked working-tree Markdown,
deduplicates paths, and excludes deleted files; its unit test covers these cases.
This lets the documented checker run before staging without mutating Git metadata.

Cargo emits duplicate-package warnings from Cell's checked-in downstream vendor
examples. These did not fail the builds/tests. Strict clippy, rustdoc, standalone
downstream/harness workspace suites, and live model experiments were not run as
part of this milestone's final broad validation.

## Preserved architectural boundaries

No new Script syntax, evaluator, model infrastructure, filesystem host, plugin
loader, or language runtime was added. Host-owned authorization, Cell confinement,
source identity/revision checks, fresh Script file execution, semantic result
classification, nominal/structural distinctions, and explicit adapter requirements
remain intact. Read-only CLI defaults strengthen rather than replace concrete
host checks; trusted native code is still not an OS sandbox. Workspace directory
preflight is not a race-proof filesystem security boundary.

## Remaining friction and next small improvement

- First-download/fresh-checkout verification and hosted platform CI remain to be established.
- IDE and Script are separate experiences: a stock IDE scratch buffer cannot run Script.
  The explicit `scratch_runner` demonstrates that integration without claiming it is default.
- Existing workspace-only CLI syntax lists files instead of entering the shell; guides
  show the explicit `shell` command rather than silently breaking compatibility.
- Shell help remains comprehensive; startup and the new observational discovery command
  provide a shorter entry path. App documents and shell buffers remain distinct owners.
- Empty semantic/evaluator inventories are honest but less compelling than a configured host.

The next small DX improvement should be a deliberately configured Script-workbench
profile using the existing no-host runner, with explicit source ownership and no
new authority, rather than a general plugin loader or implicit downstream runtime.
