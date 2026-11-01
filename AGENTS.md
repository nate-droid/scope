# Agent development loop

Read [the verification contract](docs/verification.md) before choosing checks.

- Start Fast: select `-p <affected-package>` and `--lib` or `--test <target>` before
  filtering test names. A filter alone does not avoid compiling unrelated targets.
  Verify a named test actually ran (zero matches is not success).
- Complete direct and adjacent verification; low-risk isolated changes need not
  run complete packages or downstream PTYs. Select system/integration/feature and
  reverse-dependent checks by changed boundaries using the linked policy.
  Markdown-only changes usually need Python doc checks, not a Rust workspace
  build. Code examples may need doctests/consumer checks.
- Full is required for mathematical/kernel correctness, security/authorization,
  cross-component/public contracts, dependencies, profiles/toolchain and release
  confidence. Preserve rejection, stale-source, execution-policy and PTY coverage.
  If resources prevent required escalation, explicitly report that gap; do not
  claim complete confidence or silently replace integration tests with mocks.
- Reuse the root `target` (configured for standalone workspaces too). Do not run
  `cargo clean` as routine preparation. Do not delete existing artifacts or tests
  to make a check fit. Add `--offline` only when caches are populated.
- Do not repeat broad checks after a timeout without a specific diagnostic reason;
  a timeout does not invalidate earlier targeted passes. Do not run every feature
  permutation after each edit. Full retains all CI
  configurations; minimal feature absence needs its own check. No live/paid model
  calls or credentials are required for deterministic regression verification.
- Report exact commands, results, features, and omissions with reasons. Timings
  must distinguish cold/warm builds and measurements from estimates.
