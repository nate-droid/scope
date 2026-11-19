# Imported producer

Requires the sibling Sprout checkout and the explicitly SHA-pinned B1 candidate
at `../sprout/target/b1-candidate.component.wasm` (see `TUTORIAL.md`). No artifact
is acquired or rebuilt implicitly, and no producer sources are modified.

From the Scope root:

```sh
cargo run --locked --manifest-path examples/imported-producer/Cargo.toml -- ../sprout/target/b1-candidate.component.wasm
cargo run --locked --manifest-path examples/imported-producer/Cargo.toml -- ../sprout/target/b1-candidate.component.wasm --tui
cargo test --locked --manifest-path examples/imported-producer/Cargo.toml --bin scope-imported-producer tests::canonical_computed_fields_and_independent_owner_evidence -- --ignored --exact
```

Two independently prepared owners consume the maintained producer modules. The
second replaces the checkout call's inputs in its own source copy, before
preparation: `(3, 1200, 500)` becomes `(5, 900, 250)`. Both computations invoke
through App and retain their typed completion. Expected totals (4100 and 4750)
are regression assertions, never injected output. Denial, foreign-owner,
contract-mismatch, and stale-source rejection checks remain in the journey.

The consumer projects the checked canonical value using `serde_json::to_value`,
after applying Sprout's encoding bounds. It preserves the nominal record,
fields, and primitive tags rather than manufacturing a display-only summary.

`--tui` opens the second App's registered root and seeds the shared observation
slots with both real completions. The first is historical/foreign to this owner;
the second is current. Run uses ordinary review and authorization, and publishes
new canonical feedback through the same consumer callback. Startup observations
do not grant execution authority.

## Opt-in Loom CZ observation

With the maintained sibling `../loom` checkout present, use the same executable
and explicitly pinned B1 candidate:

```sh
cargo run --locked --manifest-path examples/imported-producer/Cargo.toml -- ../sprout/target/b1-candidate.component.wasm --loom --tui
```

Omit `--tui` for checked-return and source-binding CLI diagnostics. Without
`--loom`, the original order-summary demonstration is unchanged. Missing Loom
sources or an absent/wrong candidate is an error, never a fallback. The required
candidate SHA-256 is
`f1e0c70cf6b9c7744180df1d68c1a5bb393fdda6d5770e53e4aa27e7d24dc404`.
No guest is built or promoted.

The permanent application entry `src/loom_cz.sprout::cz_amplitudes` imports the
byte-identical maintained `effects.sprout` and its `common.sprout` dependency.
It calls `effect_plus(2)`, `effect_apply(state, 2, 0, 1)` (CZ), and
`effect_decode`. The existing Loom `test_effect_baseline` returns `Bool`, not
amplitudes; this entry projects the real decoded list into a B1-supported nominal
`CzAmplitudes` record with five `Int32` fields:

| Field | One CZ | Two CZs |
| --- | --- | --- |
| `numerator_00` | 1 | 1 |
| `numerator_01` | 1 | 1 |
| `numerator_10` | 1 | 1 |
| `numerator_11` | -1 | 1 |
| `sqrt2_exponent` | 2 | 2 |

The second root source inserts a second actual CZ before decoding, **before its
own independent preparation**. Both imports are acquired from the sibling Loom
checkout at startup, not copied into the example. The source closure is sealed by
`prepare_bundle_call`; later disk edits cannot change that prepared computation.
Both calls use the existing registered operation (`loom-cz-amplitudes-v1`), exact
`call-bundle-v1` / `cz_amplitudes` / `CzAmplitudes-v1` / `read-v1` invocation
contract, pinned host, Scope authorization, fresh Cell, checked return, and
canonical native projection from that same completion. Existing limits remain
10,000 steps, recursion 16, and registry dispatch ceiling 8. Regression expectations
are checked only after execution; they are never used to construct feedback.
Denied authorization is checked for both owners and must cause zero dispatches.

In the TUI, open the palette with F1 and select **Compare Results**. The existing
observation slots contain the real first completion (historical/STALE, foreign to
the second owner) and second completion (CURRENT). Compare highlights the changed
`numerator_11` leaf. Keys `1` and `2` open each side in **Result Explorer**; expand
`value` → `fields` to inspect the preserved names and primitive values. `d` opens
retained Details; Escape returns to Compare, then the ordinary workspace.
Ordinary **Run** reviews and authorizes the second owner's prepared computation.
No historical observation grants execution authority.

Basis order is `00, 01, 10, 11` (first physical ID is MSB); amplitudes are the
numerators divided by `sqrt(2)^2 = 2`, with zero imaginary parts. Both squared
norms are exactly one. This is an **amplitude projection**, not a complete register
or event-ledger export: one and two CZs have different histories even when a vector
returns to its initial value. Scope Compare is structural, not an equivalence
prover. The independent `tests/test_effects.py::dense` oracle supplies external
mathematical expectations, never execution evidence.

Focused verification (from Scope; native effects check runs from Loom):

```sh
cargo test --locked --manifest-path examples/imported-producer/Cargo.toml --bin scope-imported-producer loom::tests::checked_loom_cz_observations -- --ignored --exact
cargo build --locked --manifest-path examples/imported-producer/Cargo.toml --bin scope-imported-producer
python3 -B examples/imported-producer/pty_launcher.py --artifact ../sprout/target/b1-candidate.component.wasm --loom
```

The real PTY journey checks Run discoverability (canceling review), the actual
changed numerator, all five fields in both Explorers, provenance/freshness,
Details, navigation, zero browsing authorizations/dispatches, clean exit, and
terminal restoration. Allow its documented 300-second scenario deadline.

This is **not editor-driven Loom execution**. Sources are prepared before the UI
opens. Edits do not re-prepare or replace the registered closure. The remaining
boundary is explicit exact-revision imported-closure preparation and rebinding:
Edit → Check closure → Prepare exact revision → Authorize → Execute → Observe.

## Real PTY acceptance

Build the current launcher, then run the example-local driver:

```sh
cargo build --locked --manifest-path examples/imported-producer/Cargo.toml --bin scope-imported-producer
python3 -B examples/imported-producer/pty_launcher.py --artifact ../sprout/target/b1-candidate.component.wasm
```

The stdlib-only driver reuses Scope's bounded PTY transport and gives the two
contained computations plus UI journey a 300-second deadline. It checks fresh
forced-redraw output (not a screen emulator): Compare's canonical total leaf
changes from 4100 to 4750; both side Explorers show all five canonical field
names and scalar values; baseline is STALE, latest CURRENT; and Details retains
closure provenance and invocation feedback. Passive Enter and navigation must
leave the pair available. After quitting, the launcher reports measured policy
calls and registry dispatches during the UI session; browsing requires both to
be zero. The driver also checks successful exit and restored terminal attributes,
alternate-screen state, and cursor visibility. It uses temporary working files,
never credentials, model calls, or changes to the producer checkout.
