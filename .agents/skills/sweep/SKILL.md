---
name: sweep
description: Periodic public-repository cleanup for Scope. Remove obsolete milestone artifacts, consolidate overlapping documentation, simplify structure, and keep current user and contributor paths clear without weakening behavior or verification.
---

# Sweep — maintain the repository people actually use

## Purpose

Make Scope's **current** repository easier to navigate, understand, build, use, and contribute to. Treat the repository as a public product, not a chronological record of development prompts. Prefer deleting obsolete scaffolding and consolidating useful knowledge into authoritative, capability-oriented locations over creating archives or moving clutter elsewhere.

This skill is **maintenance, not a redesign**. Preserve working functionality, public contracts, important design rationale, security boundaries, and regression assurance. Aim for an incremental, explainable diff that leaves the repository simpler than it was.

## Invocation and scope

Run on demand from the Scope repository root, after reading `AGENTS.md`, `CONTRIBUTING.md`, `README.md`, and `docs/verification.md` where present. Respect their current precedence and instructions; this skill does not supersede the repository verification policy.

Unless the requester narrows scope, inspect tracked documentation, examples, experiments, scripts, test fixtures, package/layout structure, and obviously dead source files. **Do not** treat all old code or docs as dead merely because their names contain a milestone or date. Do not edit sibling repositories or generated/vendor directories.

## Operating rules

1. **Inspect first.** Capture `git status --short`, the tracked file tree, documented entry points, and cross-references before making edits. Preserve unrelated worktree changes; do not overwrite or revert them.
2. **Judge by present relevance.** A file is a removal candidate if it is superseded, unreachable, purely milestone-specific, or duplicates another maintained explanation. Age, filename, and directory alone are not sufficient evidence.
3. **Prefer canonical structure.** Organize docs and examples around stable concepts (architecture, usage, extension points, verification), not H1/UX3/SI6-style implementation phases. Keep the README a practical entry point and link to a small number of authoritative guides.
4. **Consolidate, then remove.** Before deleting material, extract any still-useful contract, rationale, negative case, reproducibility instruction, or operational knowledge into its natural maintained home. Do not retain a whole milestone report to preserve one paragraph.
5. **Protect executable evidence.** Do not remove tests, fixtures, examples used by tests, compatibility shims, security checks, or public APIs without proving they are redundant or unused and obtaining appropriate verification. A milestone-named test can still enforce an essential invariant; rename or reorganize only when it improves clarity.
6. **Do not create an archive by default.** Git history already preserves past revisions. An `archive/`, `legacy/`, or `old/` tree is not an acceptable destination for ordinary obsolete artifacts. Keep historical material only when it has a demonstrable ongoing audience or reproducibility purpose, and explain why.
7. **Avoid cosmetic churn.** No large-scale renames, formatting-only rewrites, speculative abstractions, dependency changes, broad source refactors, or test-suite redesigns merely to make the tree look tidy. Separate worthwhile larger changes as recommendations.
8. **No silent behavioral changes.** Maintain CLI flags, default launch behavior, editor and agent workflows, authorization boundaries, source/proposal identity, public extension contracts, and build semantics unless explicitly authorized to change them.
9. **Be conservative under uncertainty.** If ownership, usage, or dependency is unclear, keep the artifact and report the question. Never infer unreachability solely from a text search.
10. **Leave an auditable result.** Do not commit, push, publish, or rewrite history unless explicitly requested.

## Sweep procedure

### 1. Establish the public story

Inspect what a new visitor and new contributor actually encounter:

- `README.md`, `CONTRIBUTING.md`, `AGENTS.md`, any getting-started material, and the default launch instructions.
- `docs/` navigation and whether guidance points to currently supported commands and workflows.
- Real examples and tutorials: can a person start with a supported path without studying historical milestones?
- The source and package tree: can a maintainer identify the main systems and their extension boundaries without digging through abandoned prototypes?

Write a brief baseline: current entry points, authoritative documents, obvious confusion, and any pre-existing worktree modifications.

### 2. Inventory and classify candidates

Use `git ls-files`, scoped `rg` searches, manifest/build references, imports, test discovery, and documentation links. Inspect content and usage before classification. Categorize each candidate:

| Classification | Action |
| --- | --- |
| Current and authoritative | Keep; improve navigation only if needed |
| Valuable but misplaced or milestone-named | Consolidate or rename with references updated |
| Superseded or duplicated | Remove once essential content is preserved elsewhere |
| Historical but still required for reproducibility or policy | Keep with explicit purpose and discoverable ownership |
| Dead experiment or orphan artifact | Remove once build/test/runtime usage is ruled out |
| Uncertain or high-risk | Defer with a concrete reason and suggested verification |

Look particularly for milestone summaries, one-off reports, stale design proposals, duplicated contributor commands, abandoned experiment directories, stray Rust source files, unused scripts, old generated outputs, unreferenced tutorials, and contradictory README instructions. Treat tracked files differently from ignored build artifacts; avoid cleaning the user's local caches.

### 3. Propose the smallest coherent cleanup

Before changing files, state a compact plan identifying:

- What would be deleted, consolidated, renamed, or left alone, and why.
- The canonical destination for knowledge worth retaining.
- Potential consumer and verification impact.

Then execute safe, well-supported changes. Do not pause for permission on routine reversible cleanup; **do** stop and ask before deleting ambiguous high-value material, changing public contracts, or performing a large structural refactor.

### 4. Repair coherence

After edits, check that:

- The README describes what Scope does **now**, how to start it, and where to learn more.
- Contributor instructions reference `docs/verification.md` instead of duplicating a competing testing policy.
- Links, paths, command examples, and terminology match the resulting tree.
- Important invariants and rationale have clear owners, without maintaining parallel explanations.
- Example and test directory names reflect capabilities or behaviors rather than obsolete delivery milestones where practical.
- The cleanup did not merely move clutter into a new holding area.

### 5. Verify proportionally

Follow `docs/verification.md` as the authority for Direct / Adjacent / System selection. At minimum, inspect the final diff and status, search for broken references and removed paths, and check any changed examples/commands where feasible. Run targeted checks for any source, fixture, test, manifest, or integration changes. Escalate to System/Full checks when required by affected boundaries and risk; avoid redundant blanket test runs for documentation-only changes. Report exactly what ran, passed, failed, or was not run. Never claim an unexecuted check passed.

### 6. Report the sweep

Give a concise final report with:

1. **Public-facing result:** what is simpler for a reader or contributor now.
2. **Removed / consolidated / retained:** grouped paths with short reasons, and where retained knowledge now lives.
3. **Assurance:** verification commands and outcomes, plus boundaries not exercised.
4. **Deferred candidates:** only actionable uncertain or larger items, with reasons.
5. **Net effect:** number of tracked files added/removed (if useful), major directory changes, and any remaining documentation duplication.

If the repository is already coherent, prefer a **no-change sweep** over inventing cleanup work. Do not introduce a permanent sweep report or milestone document unless specifically requested; the final response and Git diff are ordinarily sufficient.

## Success criteria

A reader on the default branch encounters current workflows rather than development chronology. A maintainer can locate the authoritative documentation for a capability. Important tests and security guarantees remain intact. Git shows a restrained, understandable change set. Running this skill again immediately should yield little or no further work.
