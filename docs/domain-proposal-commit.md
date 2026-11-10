# Pure domain proposals and host commit authority

Available with `scope-ide` feature `experimental-sprout-closure`.

## Execution admission

`scope_language::SemanticOperationEffect::ProposeDomain` classifies pure evaluation
of a proposal for external domain state. It is **not** `Read`, source-edit
`ProposeChanges`, or permission to mutate. Generic closure inspection admits
`Read` and `ProposeDomain` on `Document` targets only. `Selection` and source-edit
closures remain rejected. The effect is part of the sealed catalog, binding,
review, freshness checks, and recurring approval scope. An opaque
`InvocationContract.effect` string cannot grant this effect or commit authority.

Use the existing `RegisteredDocuments::{capture_invocation, approve_service,
invoke_service}` APIs. Service approval still uses the explicit
`HostAuthorizationRequest::RecurringClosure`; every request still requires
`ServiceClosure` policy approval of the exact arguments. Policies must explicitly
opt in to `ProposeDomain`. Legacy single-source callbacks cannot approve services.
Existing Read and interactive execution remain unchanged. The general semantic
execution boundary also forbids domain proposals from returning source-edit
`Changes` or `ValidatedChanges`. Tool metadata conservatively classifies the new
effect as `Write`, not `Read`.

## Separate commit protocol

APIs in `scope_ide::registered_document_closure`:

```rust,ignore
RegisteredClosureResult<T>::authorize_commit<P>(
    self,
    proposal: P,
    identity: ServiceGrantIdentity,
    request_ref: String,
    expires_at: Instant,
    authorize: impl FnOnce(CommitAuthorizationRequest<'_, T, P>) -> bool,
) -> Result<RegisteredCommitGrant<P>, CommitDenied>

RegisteredCommitGrant<P>::commit<O, E>(
    &self,
    transaction: impl FnOnce(&P) -> Result<O, E>,
) -> Result<O, CommitError<E>>

RegisteredCommitGrant<P>::revoke(&self)
```

`CommitAuthorizationRequest` exposes the exact retained invocation (including
output, sealed binding, review and optional service evidence), the proposed host
value, grant identity, trusted commit request reference and expiry. This callback
is a **new explicit authority decision**, not execution policy approval. It must
verify successful checked domain output, correlate its exact narrow proposal,
and authorize principal/resource/action scope. Nested provider `Result::Err`,
domain rejection, or malformed output must not be approved. Scope is runtime-
and schema-agnostic; it cannot perform this application validation automatically.
Use immutable, owned proposal data without authority-changing interior mutation.

The method consumes the invocation result, so it cannot mint two grants from
one result. Read results cannot mint grants even with an accepting callback.
Identity/request bounds, expiry and parent service validity fail closed; parent
validity and commit expiry are rechecked after the approval callback.

A grant retains the exact proposal, binding, service correlation, identity,
trusted request reference and expiry. Accessors are `binding()`, `service()`,
`identity()`, `request_ref()` and `expires_at()`. Commit and revocation share a
mutex; service-derived commits additionally hold their parent service's revocation
mutex. Service lifetime is retained independently of its public handle. No lock
is held over guest execution. Expiry is checked at commit admission: a transaction
admitted before expiry may finish afterwards. Revocation returning means an
already-admitted transaction has finished; subsequent commits fail. Parent service
revocation also invalidates all its outstanding commit grants.

Inside `transaction`, the trusted host must acquire its repository transaction/
CAS lock, recheck **current** principal authorization, resource scope, expected
revision and narrow preconditions, compute checked revision increments, then
atomically write or change nothing. All competing writers must obey that same
repository protocol. Scope does not supply storage, roll back writes, authenticate
principals, or synchronize unrelated ACL mutations. Any ACL revocation requiring
ordering with persistence must participate in the host transaction protocol.
Do not run guests, await, perform reentrant grant operations, or acquire these
locks in reverse order inside the callback. Keep it short and bounded.

An admitted attempt consumes the grant even if the host returns an error.
Panics poison authority locks and future checks fail closed. Sharing an `Arc` of
a grant does not allow replay. This is in-process single-attempt authority, not
durable exactly-once delivery or a durable transaction log. Retain an application-
bounded audit correlation separately if required.

`CommitDenied` distinguishes `NotDomainProposal`, `InvalidRequest`, `Denied`,
`Revoked`, `Expired`, `Consumed`, and `Poisoned`. `CommitError<E>` distinguishes
`Authority(CommitDenied)` from `Host(E)`.

## Focused regression coverage

In `registered_document_closure::tests`:

- `domain_proposal_admission_preserves_source_edit_and_selection_rejection`
- `domain_service_requires_explicit_approval_and_each_exact_request`
- `commit_authority_is_not_implied_by_read_or_proposal_execution`
- `commit_is_single_attempt_including_host_failure`
- `commit_rechecks_own_and_parent_revocation_and_expiry`
- `commit_authorization_rechecks_parent_after_callback`
- `commit_and_revocation_have_a_shared_linearization_boundary`
- `concurrent_proposals_require_host_cas_and_current_authorization`
- `panicking_transaction_poisons_authority_instead_of_replaying`

Concurrency tests use channels and direct lock probes rather than timing sleeps.
They cover both commit-grant and parent-service revocation waiting for an admitted
transaction, and two proposals competing for one authoritative revision.
`scope-language` additionally tests `domain_proposal_effect_cannot_return_source_edits`
and includes the new effect in purpose/serialization round trips.

No Tavern integration, HTTP mutation endpoint, persistent repository, or guest
capability was added. Consumers must explicitly register and approve the new
effect and implement the checked-output and host transaction boundaries above.

## Compatibility and validation

The additive public enum variant requires downstream exhaustive matches on
`SemanticOperationEffect` to add an explicit case. Existing serialized effect
names and Read behavior are unchanged. Hosts that allow all effects indiscriminately
should tighten their policy; proposal execution is not commit approval.

Validated in this checkout:

- Scope IDE library with `experimental-sprout-closure,tui`: **654 passed**.
- Scope language library: **24 passed**.
- Registered closure focused suite: **33 passed** (9 new tests).
- Interactive inspection integration: **8 passed, 1 ignored**.
- Operation interoperability integration: **6 passed**.
- Shell inspection integration: **26 passed**.
- `git diff --check`: clean.

A broader Scope IDE/language run timed out after 200 seconds at the PTY launcher
`source_proposals_through_real_launcher`; no failure had been reported. It is not
claimed as a passing full suite. Cargo also emits existing duplicate Cell-package
warnings. No dependency changes were made.
