# S0.75 registered invocation contract

Scope owns validation, sealing, host authorization, dispatch freshness, and retained
observations. Trusted providers own language-specific constraint validation, bounded
execution/output, and enforcement of runtime policy. This is not a sandbox.

- Existing zero-argument capture/invoke and TUI behavior remain supported.
- Parameterized calls accept a bounded JSON object of primitive values only, using
  the existing SemanticOperationDescription argument validator. Unknown fields,
  missing required values, duplicate/empty schema names, containers, and bounds
  violations are rejected before discovery, authorization, or execution.
- A parameterized call supplies host-owned nonempty schema version, entry, output,
  and effect identities. The complete operation/schema/constraints, these identities,
  exact arguments (including absent versus null), sources, implementation, and
  effective policy are sealed together and included in review and freshness checks.
  Identity claims must truthfully describe the prepared provider; Scope cannot
  infer an ABI from opaque native code. Only Read/Document operations are admitted.
- Each invocation selection is consumed, including rejection. Provider requests
  expose only sealed arguments. Successful observations retain original arguments,
  identities, sources, and output; observations confer no execution authority.
- Recurring service authority is explicit opt-in. Approval seals the complete
  binding EXCEPT argument values: owner, sources, implementation, operation/schema,
  call contract, and policy must still match exactly. The host supplies an explicit
  nonempty grant ID and nonzero version, trusted approval reference, and monotonic
  expiry. Grant identity is a host-owned audit label, never authority by itself.
- Every service invocation consumes a new validated selection and requires a trusted
  per-request reference plus HostSemanticPolicy authorization via ServiceClosure,
  exposing actual sealed arguments, exact review, and grant ID/version. A grant is
  necessary but never sufficient to execute. Expiry and sticky revocation are
  checked before per-request policy and again immediately at dispatch after policy.
  Result observations retain request/grant labels alongside exact call evidence.
- An adapter may derive a fresh runtime deadline from each call's sealed duration
  JSON. Scope does not own Cell deadlines or refresh sealed absolute timestamps;
  runtime deadline enforcement remains the trusted provider's responsibility.
