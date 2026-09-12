# 112 — Sparse owned-register set

Status: complete

Replace task 111's rejected full register scan with an immutable sparse ownership
projection derived from bytecode. Each opcode reports whether its destination can ever
hold a heap-tagged value. Function construction unions those destination registers once;
frame release visits only that set, drops actual heap values to undefined, and leaves
all immediate-only registers unspecified in pooled storage.

This is preferable to the initially proposed mutable bitset: it adds no branch or bitset
update to `put`, so numeric loop writes remain untouched. The immutable bytecode is the
single fact and the sparse release set is a memoized projection. Direct AOT stencils
need no metadata mutation because their guards admit and produce immediate values only.

Acceptance: every result-producing opcode is exhaustively classified; copied heap values,
allocating results, unknown call/property results, and immediate-only results are tested;
pool release retains no `Rc`; direct stencil writes need no metadata mutation;
all tests and full smoke pass. Accept only after an exact alternating full-suite A/B
against [[110-constant-condition-control-stencils]] improves aggregate without crossing
the per-suite floor. Remove the machinery if metadata maintenance costs exceed sparse
release savings.

Result: rejected. The immutable projection and sparse release passed all 47 tests and
`reports/sparse-register-release-smoke.jsonl`. The exact alternating six-run comparison
in `reports/sparse-register-release-ab-6/comparison.txt` was neutral but negative:
830.931→830.735 (-0.02%). Crypto fell 1.20%; all other components remained within the
floor. The code was reverted under the strict positive-aggregate rule.

Together with [[111-owned-register-pool-invariant]], this establishes that neither a
full conditional scan nor a statically narrowed conditional scan is worthwhile. Future
register-release work must eliminate scanning entirely (for example, a zero-overhead
ownership event already implied by an allocating operation), not reshuffle the scan.
