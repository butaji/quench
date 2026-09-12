# 329 — Block-local value numbering and effect forwarding

Status: in_progress

Add one forward reducer over the canonical quoted region before stencil tiling. Maintain
immutable maps for:

- pure expression identity `(op, inputs, representation) -> ValueId`;
- exact abstract locations `LocationKey -> ValueId`;
- the last exact store per location.

Within one basic block, reuse an existing value for identical pure numeric/tag/shape
operations, forward an exact store into a following load, and erase an overwritten exact
store when no intervening observable effect can see it. Calls, allocation, prototype
mutation, coercions, exceptions, and unknown heap access clear only the affected domains.

This is the finite, block-local implementation slice of [[173-effect-token-memory-ssa]].
It uses the same `RegionOp`, `ValueId`, `LocationKey`, and effect vocabulary; it must not
create a second optimizer AST. The reducer is a pure quote-to-quote transformation and
therefore composes before [[157]]'s cover selection and [[158]]'s register plan.

Acceptance: tests cover aliasing, calls, prototype mutation, throwing coercions, repeated
loads, store-forwarding, and dead stores; diagnostics count each rewrite kind; the emitted
block has fewer semantic operations and machine loads/stores; Richards, DeltaBlue,
RayTrace, and complete-suite alternating A/B decide retention. All worklist/table limits
are named constants.

Primary source: JavaScriptCore documents block-local CSE and register allocation as the
low-latency DFG strategy before its heavier global tiers:
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>. LLVM MemorySSA remains the
model for the later global generalization: <https://llvm.org/docs/MemorySSA.html>.

## First implementation and measurement

`numeric_region/rewrite.rs` is now the single pure quote-to-quote reducer over `RegionOp`.
It derives basic-block boundaries, tracks aliases and exact local/captured/heap locations,
protects side-exit frame state and operands still read from `InlineSite`, and exposes named
feature constants for each rewrite family. Diagnostics report each transformation kind.

Only repeated local-load forwarding into fully burned numeric operands is enabled. Enabling
the other families together exposed a real semantic error in Crypto: shift and bitwise
stencils still read their original metadata operands. Extending forwarding through traced
loops was semantically correct after protecting those operands, but slowed Navier-Stokes by
about 44% because loop side exits and re-entry still require eager canonical frame state.
That work remains gated on [[330-zero-code-frame-state-hints]].

The safe block-only candidate passes 123 release tests and 123 stress-GC tests. The first
three-pair, 300 ms full comparison in
`reports/task329-block-only-forward-full-ab-3/comparison.txt` was neutral: 2104.83 to
2105.34 (+0.02%). Only 16 sites are selected across V8v7 (Crypto 3, Earley-Boyer 1,
Navier-Stokes 12); the other six suites select none. Therefore this is retained as analysis
substrate, not accepted as a score improvement. [[332]] makes erased operations physically
removable but cannot compensate for such sparse coverage.

[[372-quoted-register-copy-and-dead-definition-rewrite]] adds the complementary register
copy/DCE slice directly over canonical `DynCode`. It deliberately does not duplicate this
task's heap-location or numeric-expression maps: exact memory forwarding remains here,
while register copy elimination belongs to the lower, shared quoted representation.
