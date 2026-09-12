# 172 — Ownership SSA for Value flow

Status: in_progress

Annotate every nontrivial `Value` definition and use in Task 171's region plan as
`Owned`, `Guaranteed`, or `Trivial`. An owned value is consumed exactly once along every
path; cloning is an explicit `CopyValue`, borrowing has a checked lifetime scope, and
destruction is an explicit `DestroyValue`. Forwarding uses transfer ownership without a
retain/release pair.

Derive last-use moves, borrowed helper operands, phi ownership, and edge-specific destroy
schedules before stencil tiling. Select pre-cooked `copy`, `borrow`, `consume`, and
`destroy` stencil variants from that proof. This subsumes per-op ownership guesses while
remaining valid when Task 148 changes the heap representation: ownership recipes stay
the same and only the leaf implementation changes.

Categorically, ordinary value flow is not cartesian for owned heap values: duplication
and disposal are explicit morphisms rather than free structural rules. Trivial tagged
immediates retain unrestricted copying. This distinction must be represented in data,
not inferred from ad-hoc branches in hot helpers.

Acceptance: an ownership verifier rejects leaks, double consumption, and borrow scopes
that outlive their base; tests cover branches, loops, calls, exceptions, returns, and
overwriting slots; retain/release/drop counters fall in Richards and Earley-Boyer;
native profiles and complete alternating V8v7 A/B improve.

Source: <https://github.com/swiftlang/swift/blob/main/docs/SIL/Ownership.md>.

## Task 361 accepted slice

Task 361 implements and accepts the first `StoreTake` proof for terminal property stores.
Instead of incrementing/decrementing `Rc`, it performs an ownership-preserving raw swap:
the source owner moves into the property, and the displaced property owner moves into the
dead source local for normal frame teardown. Static constructor-shape installation supplies
the independent fixed-slot obligation. The exact Earley-Boyer residual falls from 784,948
generic entries to 2, its suite score improves 16.95% in the full A/B, and the aggregate
improves 0.72%.

The broader task remains in progress. Generalize the same representation to per-edge
last-use facts, `CopyValue`, `DestroyValue`, branches, calls, and exceptions; do not clone
Task 361's exact syntax into additional handlers.

## First executable slice after Task 359

Task 359 selected a seven-operation two-property terminal stencil, but all 1,280,750
targeted Earley-Boyer entries still took its slow edge because the source or displaced
property values retained `Rc` ownership. Its full aggregate was -0.31% and it was removed.
This makes the first Ownership SSA slice concrete.

For a property store whose source has a proved lifetime-ending use, emit `StoreTake`: move
the raw owner into the property slot, replace the source frame location with `undefined`,
and create an explicit `DestroyValue` for the displaced slot value. For a non-ending use,
emit `CopyValue ; StoreTake`. Terminal regions with no observing effect are the first
scope, but selection is solely by liveness and ownership facts. The verifier must prove
one consume on every path, including cache-miss replay and exception exits.

Until Task 148 migrates the remaining heap kinds, a rustc-cooked destroy leaf may use
Rust's supported raw `Rc` decrement API. Its zero-count/deallocation branch is cold and
must not force the preceding shape guard, source transfer, or slot store back through
`dyn_block_step_impl`. Counters distinguish ownership transfers, explicit copies,
destroy-fast, destroy-finalize, and ownership side exits. Only after this primitive has a
native hit population may Task 359's general grammar be reconsidered.

Make semantic operations non-consuming where possible and derive explicit
`DestroyValue` operations afterward. This mirrors CPython's current uop strategy of
leaving inputs for separate cleanup operations so ordinary DCE can erase cleanup that
becomes unnecessary. Attach instruction-pointer/finalizer/GC synchronization to the
rare destroy arm that can actually run observable cleanup, not to every logical value
death. Sources: <https://github.com/python/cpython/issues/145866> and
<https://github.com/python/cpython/issues/152106>.

CPython 3.15's copy-and-patch JIT now tracks unique references to eliminate reference
count updates and enable in-place integer/float operations. That is corroborating
evidence for making uniqueness a refinement of `Owned`, not an unrelated counter pass:
`Owned { unique: Proven | Unknown }`. A uniqueness proof may select an in-place heap
operation; any aliasing store, unknown call, phi with distinct origins, or publication
widening clears it. The current direct-double `Value` does not need boxed-number
mutation, but string/buffer and future heap aggregates can consume the same fact.
Source: <https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>.
