# Scope Script SS4-B — Bounded semantic trial orchestration

## Decision and SS4-A prerequisite

Implemented as an opt-in executable downstream example, not a Scope-wide experiment subsystem:

- `crates/scope-script/examples/trial_runner.rs`
- `crates/scope-script/examples/trial-comparison.scope`

Before implementation, read the sibling Path checkout's `docs/design/PATH_SS4_EXPERIMENT_CONTRACT.md` (SS4-A). Its conclusions are important: `evaluate_goal_candidate` is a pure baseline/candidate evaluator; language registration does not install Script privileges; Path has no configurable per-candidate kernel budget, no independent candidate source identity in the projection, and no generic typed output compatibility contract. There is no upstream blocker for this milestone. No Path dependency or guessed Path API was added here.

## 1. Existing APIs reused

| API | Use |
| --- | --- |
| `SourceTarget::snapshot`, `source_edit::SourceSnapshot` | Borrow exact source identity, revision, language and bytes. Not the separate `scope_language::SourceSnapshot` historical evaluation type. |
| `ScratchBuffers` | Existing source owners, one collection for the two trial targets; distinct IDs within that collection. |
| `SemanticToolProvider` | Existing registered language operation discovery, validation, target scope, authorization, source text/version checks and dispatch. |
| `AgentSession`, `SemanticHost` | Existing tool catalog, explicitly installed single Read action, exact source/version tokens, explicit host authorization and existing value/call limits. `AgentSession` here is a tool dispatcher, not a model invocation. |
| `SourceEditController` | Required existing provider integration; remains empty. No proposals or applications. |
| `Host`, `Interpreter` | Explicit `trials.compare` installation and ordinary SS3 syntax/budgets. |

The operation registry/`RegistryHost` was examined: it uses a persisted `DocumentSession`, unnecessary for this in-memory fixture. Language operations already have their own semantic dispatch path. The example does not create a second operation registry.

`ScriptRunner`/`RunRecord` were examined. The existing runner snapshots an App document and installs one selected semantic Read operation; it is not a general host-composition hook. This example uses the lower-level `Host` interface instead of changing the runner or its records. It retains one returned report and the interpreter's bounded bindings/prints, not an unbounded `RunRecord` history. A fresh interpreter is constructed in `main`. Existing standalone `NoHost`, shell callbacks and runner privileges are unchanged.

## 2. Concrete fixed contract

The executable host installs only provider `trials`, action `compare`, input `{}`. Script cannot supply trials, callbacks, source paths, policies, reset instructions or operation names. All specifications are host-owned.

There are exactly two predefined specifications, each referring to baseline `captured`:

| Trial | Required operation | Arguments/context | Policy | Maximum attempts |
| --- | --- | --- | --- | --- |
| `first` | `language.example.declarations.describe_selection` | `{}`, first complete declaration line | Continue | 1 |
| `second` | same | `{}`, second complete declaration line | Stop | 1 |

Baseline is the existing deterministic declaration fixture, captured once verbatim: `let answer = 42\nlet count = 7\n`. Each trial describes a different selection of that same complete baseline, rather than mutating it. This is a semantic orchestration fixture, not a proof evaluator.

Before creating providers, validate authorization, one-shot state, exactly two distinct ordered predefined IDs, both selections, baseline references, both operations, argument objects, per-trial maxima, source identity/revision/language/bytes and the 65,536-byte baseline cap. The entire specification list is checked before either attempt. Invalid requests and stale baselines are invocation errors with zero trial attempts. Per-trial authorization remains a separate host decision and can refuse an individual attempt.

The report has `trials` (always two records after valid dispatch), `invocations` (attempts, including explicit per-trial denial), and `retention_limit`. Normal trial records contain:

- `trial_id`, owner-qualified `baseline` and `trial_source` identities/revisions/languages;
- `provider`, `operation`, original catalog `contract`, arguments and selection;
- explicit policy, maximum attempts and isolation description;
- controller `status`, untouched semantic `outcome`, and invocation `error` or null.

Controller status is **not** a downstream classification:

- `returned`: host returned a structured value, including any downstream blocked/rejected/unknown result;
- `invocation_error`: controller refusal or actual invocation error; error text is retained using the existing error display;
- `skipped`: preceding controller failure with Stop policy;
- `resource_limit`: complete result could not be retained; outcome is explicitly null, never a truncated semantic report.

Stop/Continue applies to controller errors and retention failures, not to invented interpretations of semantic classifications. Every downstream returned value remains inspectable as-is. The fixed fallback omission record retains trial/source/provider/operation provenance and explains omission. It deliberately does not claim to preserve an oversized result.

## 3. Host versus Script responsibilities

Host owns capture, source collections, fixed specifications, allowlist, policies, limits, dispatch and report assembly. Script invokes the comparison once, binds its structured result, uses ordinary SS3 conditions on documented controller status fields and prints the full report. The final expression returns that same report. There are no loops, new syntax, privileged builtins or arbitrary script execution callbacks in the controller.

Run from the Scope root:

```sh
cargo run -p scope-script --example trial_runner --locked --offline -- --allow-read
```

Omitting `--allow-read` denies comparison. The flag authorizes only this explicitly installed fixed Read comparison, not workspace editing, agents, models, networking or processes.

## 4. Isolation guarantees

Original source is an immutable borrowed snapshot throughout execution. There is no mutable original-source access, save, filesystem read, rollback or reset path. Both trial owners are populated from exactly that one capture; byte equality is asserted before dispatch. Each has a distinct scratch identity in the **same** trial collection, with baseline collection provenance kept separate. Baseline and first trial may have equal raw scratch IDs; owner-qualified identities do not alias.

Owner labels are local to this one-shot example host, not globally persistent IDs. A downstream multi-document/multi-host installation must supply its own stable owner namespaces rather than reuse these literal example labels.

Different guarantees remain separate:

- Fresh Script interpreter: one per full example execution, not one per semantic call. It cannot reset source or downstream state.
- Fresh trial source owner/snapshot: separate scratch owners with identical bytes and their own revisions.
- Fresh tool provider/controller/`AgentSession`/`SemanticHost`: one per trial. These are dispatch/authorization objects, not fresh interactive language runtimes.
- Fresh downstream runtime/model conversation: none is needed or created by this stateless declaration fixture. No claim is made for stateful providers or future models.

Scratch owners are used because the existing Scope semantic bridge takes a `SourceTarget`. No editable language runtime is manufactured. Path's pure Rust candidate route can share the immutable baseline directly and omit scratch buffers and interactive sessions entirely: each evaluator owns its candidate string. A stateful evaluator is **not supported by this example** until its host establishes and tests an appropriate fresh-runtime/reset guarantee. Never mutate the original and roll back.

## 5. Authorization, effects and bounds

Both comparison authorization and existing provider/semantic-host authorization remain in force. The provider policy binds actual source ID, revision and exact baseline bytes; permitted scope is that trial document only. `SemanticHost` restricts dispatch to the selected registered Read action, preserves its existing 64-attempt ceiling and value validation. The outer controller is narrower: one comparison and at most two total attempts, each specification permitting one. Invocation counting and recursive retained-value accounting use checked arithmetic.

The controller does not re-label an editing/agent operation as Read. The only installed operation is the existing fixture Read operation. There are no agent/model calls or writes to the original workspace. Edit application, if added later, must be separate and use Scope's proposal review/edit controllers with fresh validation and authorization.

Script's existing source, syntax-depth, expression-step, work and retained-value budgets are unchanged. This fixture has no configurable downstream computational budget; its existing operation/value constraints remain authoritative. This does **not** promise wall-clock deadlines, preemption or cancellation of synchronous downstream work.

One report only: no internal outcome history. Complete trial entries share a 65,536-unit retention budget (checked recursive value-size accounting, not a claim about allocator bytes). Up to 4,096 additional units are reserved for the outer envelope and two fixed omission records; the complete final report is checked against that bound. Intermediate downstream construction is governed by downstream limits and existing `SemanticHost` value checks, not this post-return retention budget. Do not claim the retention limit bounds a provider's peak allocation.

## 6. Contract/type preservation

The original selected catalog descriptor is retained; no output schema/version is fabricated where the fixture publishes none. Complete nested result envelopes are stored without projection or normalization. The fixture retains its partial flag, unsupported references, unknown inferred type, rationale, source span, literal and operation identity. Controller status `returned` does not assert semantic acceptance.

Tests compare **whole** trial outcomes against independently wired direct `SemanticHost` invocations, and compare the Script-returned/printed full report against the controller's report. Matching fields grant neither nominal compatibility nor authorization. No adapters, nominal conversions or new Script type algebra are introduced.

## 7. Deterministic tests and observed results

`trial_runner` example tests cover:

- byte-identical trial baselines (asserted at each trial creation);
- distinct trial owners and two different outcomes (`answer`/`42`, `count`/`7`);
- complete nested outcome equality against independent direct semantic dispatch;
- rejection of 0, 1 and 3 trials before effects;
- whole-request validation of an invalid second maximum before the first attempt;
- explicit comparison authorization denial, stale revision and zero attempts;
- first-trial authorization failure followed by Continue versus Stop (2 versus 1 attempts, explicit skipped record);
- forced zero payload-retention budget, two resource-limit records, bounded fallback report;
- one-shot reuse denial without further attempts;
- end-to-end Script conditions, full report print and final report preservation;
- no proposals (asserted after every attempt), no mutable original-source capability, and unchanged original text/revision assertions in the executable.

An invalid selection and provider policy refusal initially demonstrated that downstream failures may be returned as envelopes rather than Rust invocation errors. Tests now deliberately use an explicit controller authorization refusal for controller Stop/Continue coverage, preserving that important distinction.

Validation commands:

```sh
cargo test -p scope-script --all-targets --locked --offline
cargo run -p scope-script --example trial_runner --locked --offline -- --allow-read
cargo fmt --all -- --check
git --no-pager diff --check
```

The test suite and authorized executable passed. The executable printed both full independent envelopes with literals `42` and `7`, revisions `0`, and trial owners `Scratch(0)` and `Scratch(1)` in the trial collection. Existing cached Cell duplicate-package warnings are unrelated.

## 8. Downstream Path integration requirements

No live Path connection is claimed. SS4-A documents the supported next step:

1. Capture exact lesson bytes, owner identity and revision once; validate freshness at the host boundary.
2. Fix two `(goal_id, replacement)` specifications and operation `language.language.path.evaluate_goal_candidate` (the doubled `language` is intentional). Keep candidate-only Read authorization and explicit invocation maxima.
3. Prefer Path's pure immutable baseline evaluator where sufficient. If using Scope's provider bridge, explicitly bind `PathLanguage`, trial targets, source scope, argument allowlist and authorization; registration alone grants nothing.
4. Preserve the complete candidate envelope and its published `schema_version: "1"` alongside owner-qualified provenance. Candidate source text is not an installed document revision; Path's projected candidate version is the baseline revision.
5. Preserve `Completed`, `RejectedCandidate`, `StillIncomplete`, `Inconclusive`, `InvalidRequest` and supporting observations verbatim. Controller error/resource status is a different layer.
6. Keep Path parser/kernel limits and projection limits in force. SS4-A reports 100,000 ticks per public kernel call, depth 128, and projection depth 64/output 65,536 bytes; these are not caller-configurable global trial limits. Record the actual downstream implementation version when connecting it.
7. Retain limitations: original candidate-stage history is absent, candidate identity is host-owned, and no automatic typed output compatibility is published. Applying candidates remains a separately authorized reviewed edit.

## 9. Remaining blockers and non-goals

No Scope core blocker for the requested bounded comparison. Production Path wiring, persistent owner namespaces, stronger downstream output contracts and stateful runtime lifecycle guarantees are downstream integration work, not features claimed by this fixture. No generalized scheduler, reset command, asynchronous cancellation, history store or multi-action Script runner was added.
