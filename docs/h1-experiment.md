# H1: real Path × retained Scope agent

## Result

**PASS with a real local model**, plus a passing deterministic decision-model control against the **same real Path LanguageService/kernel**. No fake language service was used. This establishes the bounded retained-agent integration, not general proof-authoring ability.

| Evidence | Local CellAgent | Deterministic control |
| --- | --- | --- |
| Model | `qwen3-30b-a3b` via LM Studio | scripted tool/complete decisions |
| Typed stop / terminal state | `Completed` / `Completed` | `Completed` / `Completed` |
| Decisions / tools / malformed | 2 / 1 / 0 | 2 / 1 / 0 |
| Successful semantic operation | `language.language.path.inspect_example` | same |
| Native kernel result | `kernel.checked = true`, `proof_check.status = passed` | same |
| Published lifecycle callbacks / failures | 8 / 0 | 8 / 0 |
| Host authorization count | 1 | 1 |
| `scope.agent.get` equals published snapshot | true | true |
| Terminal JSON deserializes as `AgentTurn` | true | true |
| Retained source revision / fresh / unchanged | 0 / true / true | 0 / true / true |
| Accounted conversation bytes | 9679 | 9416 |

The retained source is Path's exact `FIXTURE_SOURCE` sentinel. `PathLanguage` constructs the compiled associativity proof/type, invokes in-process `path_kernel::infer` and `check` in an empty context, and returns its judgments. It does **not** parse or prove an edited document. The harness checks native tool output in memory; neither model prose nor a successful tool name alone is accepted as kernel evidence.

## Setup and identity

- Scope HEAD: `6ec3192657c91a54abcdc22acc4a06a608d0e984`.
- Path HEAD: `2abade9ff0eda06c4846e777a868ad83991422f1`.
- Both working trees already had changes; these runs used their current working sources, **not pristine HEADs**. The original ignored `target/h1-experiment/baseline.json` records baseline status and tracked-file SHA-256 values; it is local-only, not a clone deliverable.
- Reusable standalone Cargo workspace: `examples/h1-path-agent/Cargo.toml`, with a committed lockfile and relative path dependencies on this checkout's `scope-ide` (`agent` feature) and sibling `path-scope`. Root manifests/dependencies were not edited. The lockfile pins external dependencies, not working-tree path dependencies.
- Required layout: sibling directories `parent/scope/` and `parent/path/`, with the harness at `scope/examples/h1-path-agent/`. See its `README.md`. Path's own Scope dependency must resolve to this same `scope/` checkout. Original local-only metadata verified exactly one `scope-ide` and one `scope-language`, both under `scope/crates`; do not substitute another checkout or registry/git source.
- Before model use, approved unsandboxed loopback request `curl --noproxy '*' --connect-timeout 3 --max-time 10 -fsS http://127.0.0.1:1234/v1/models` returned `qwen3-30b-a3b`, `qwen3.8-27b`, and `text-embedding-nomic-embed-text-v1.5`. Only the first was used. No paid endpoint/API key was used.

## Bounds, authority, and retention

Reusable harness: `examples/h1-path-agent/src/main.rs` (copied from the original isolated experiment). It calls `run_retained_scratch_agent_observed` directly with real `path_scope::LANGUAGE`, a caller-owned retained scratch buffer, and `CellAgent::new(AgentConfig::local(...))` for the real run. The allowlist grants only `inspect_example`; authorization verifies the exact scratch identity, revision, operation, and fixture bytes. No save/proposal/write tools are registered.

Limits: 3 decisions, 1 tool call, 1 malformed tolerance, 320 tokens per decision, 65536 bytes per message, 196608 conversation bytes. The existing Cell OpenAI adapter has a **120-second HTTP timeout per request**; the tool additionally bounded each build and real run to **120 seconds total**. Neither timed out.

Every callback publishes into `AgentTurnStore`. After the run an independent `AgentSession` with `AgentTurnToolProvider` invokes `scope.agent.get`; its result must exactly match the store. Both snapshots deserialize to terminal typed `AgentTurn`s. Native output is used only for in-memory acceptance checks; snapshots omit native outputs, prompts, responses, and raw protocol captures. Original safe derived checks and sanitized lifecycle snapshots are preserved in `examples/h1-path-agent/evidence/{real,fake}-{summary,snapshot}.json`. New runs write to ignored `examples/h1-path-agent/target/results/`, leaving that historical evidence untouched. No raw trace or credentials are persisted; machine-local baseline/metadata files are not copied into the example.

## Commands and actual outcomes

Original commands ran from the Scope root (before the harness was preserved in `examples/`):

1. `cargo build --offline --manifest-path target/h1-experiment/Cargo.toml` with `CARGO_TARGET_DIR` set below: initial harness compile failed because `SourceTarget` was not imported. Added the trait import **only in the harness**.
2. `cargo build --offline --locked --manifest-path target/h1-experiment/Cargo.toml` with the same target directory: passed (120-second tool limit). Existing Scope dead-code warning and cached Cell vendored-example duplicate-package warnings were nonfatal.
3. `target/h1-experiment/build/debug/scope-h1-experiment fake`: passed with the tabled results.
4. `target/h1-experiment/build/debug/scope-h1-experiment real`: approved unsandboxed loopback access; passed with the tabled results (120-second tool limit).
5. Unfiltered offline `cargo metadata` failed because uncached Windows-only `crossterm_winapi` would require downloading. The dependent JSON verification consequently also failed to parse that empty output. Repeated with `--filter-platform aarch64-apple-darwin`: passed offline, with no download.
6. SHA-256 comparison of every baseline tracked file in both trees: **zero changed files**. Evidence: `verification.json`. Existing downstream edits were preserved. Writes were confined to `target/h1-experiment` and this new report.

## Preserved-harness validation

`cargo build --offline --locked --manifest-path examples/h1-path-agent/Cargo.toml --target-dir target/h1-experiment/build` passed within the 120-second tool limit, reusing the ignored original build cache. Running `target/h1-experiment/build/debug/scope-h1-experiment fake` then passed: real kernel checked, `Completed`, 2 decisions, 1 semantic call, 8 published callbacks, and successful `scope.agent.get` equality/typed deserialization. No real-model rerun was made during preservation; the real-model evidence above is from the original exercise. Only the harness, its packaging, and this report changed; no core code was modified.

## Exact bounded reproduction

Run from the `scope/` root in the sibling layout described above. Offline builds require cached dependencies; a fresh machine must first populate its cache with an authorized online build. LM Studio must serve the listed model; rerun the model-list probe before selecting it. In Zed, the loopback probe and real binary need approved **unsandboxed** access because host-specific sandbox grants cannot authorize IP-literal loopback.

```sh
curl --noproxy '*' --connect-timeout 3 --max-time 10 -fsS http://127.0.0.1:1234/v1/models
python3 -c 'import subprocess; subprocess.run(["cargo","build","--offline","--locked","--manifest-path","examples/h1-path-agent/Cargo.toml","--target-dir","examples/h1-path-agent/target"],check=True,timeout=120)'
python3 -c 'import subprocess; subprocess.run(["examples/h1-path-agent/target/debug/scope-h1-experiment","real"],check=True,timeout=120)'
python3 -c 'import subprocess; subprocess.run(["examples/h1-path-agent/target/debug/scope-h1-experiment","fake"],check=True,timeout=120)'
python3 -c 'import pathlib,subprocess; host=next(line.split(": ",1)[1] for line in subprocess.check_output(["rustc","-vV"],text=True).splitlines() if line.startswith("host: ")); output=subprocess.check_output(["cargo","metadata","--offline","--locked","--filter-platform",host,"--format-version","1","--manifest-path","examples/h1-path-agent/Cargo.toml"],timeout=120); pathlib.Path("examples/h1-path-agent/target/metadata.json").write_bytes(output)'
```

Each run overwrites only its sanitized experiment snapshot/summary. Local-model output may vary; the binary exits nonzero unless completion, exactly one authorized successful semantic call, positive native kernel checks, fresh source, and successful publication are all observed. No broader test suite was run.
