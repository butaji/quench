# 289 — Tail-merge identical cold continuations across stencils

Status: planned

The mirror image of [[161]]'s cited tail-duplication (bounded duplication of tiny hot
tails to remove branches): many distinct guard families in this codebase share the
exact same *cold* continuation shape — raise the same class of error, fall back to the
same generic dispatcher, or exit to the same `KernelExit`/dead-condition path (per
[[84]]/[[160]]/[[281]]'s dead-result/dead-local condition-stencil families). Today each
guard family's cold arm is presumably emitted at its own site rather than sharing one
physical tail, since nothing in the existing composition machinery ([[109]]'s normalized
vocabulary, [[117]]'s composable sub-block regions) explicitly merges structurally
identical cold continuations across otherwise-unrelated call sites — each is only
deduplicated *within* its own kernel-identity/hash-consing scope ([[16]], [[42]],
[[135]]), not across distinct guard families that happen to converge on the same
generic exit.

This matters directly for the instruction-cache pressure this project's own growing
stencil catalog creates (79 constant-operand variants, 163 caller-customized images,
each AOT-cooked family multiplying the number of physically distinct cold-arm
occurrences) — merging identical cold tails is a pure code-size/icache win with no
correctness risk beyond correctly proving the tails are *actually* identical (same
target, same argument-passing convention, same effect), which is a straightforward
structural equality check, not a new optimization to invent from scratch.

Concrete steps:
1. Identify the small number of genuinely common cold-continuation shapes already in
   the codebase (the shared generic block-step exit, the shared `KernelExit`, common
   error-raise targets) and confirm which guard families currently emit their own
   physically distinct copy of each rather than branching to one shared tail.
2. Where two or more guard families' cold arms are structurally identical (same target,
   same calling convention, same live-value set crossing the boundary), merge them into
   one physical tail at cook time, with every contributing guard family branching to the
   same address rather than each owning a copy.
3. Verify this is purely a code-size/layout change: no guard family's *fast* arm timing
   or correctness changes, only the cold arm's address and code-sharing.

Acceptance: at least one pair of currently-distinct, structurally-identical cold
continuations (from two different guard families) merges into one shared tail, verified
by disassembly showing both guard families branch to the same address; total compiled
code size decreases measurably for the affected stencils; alternating A/B on the full
V8v7 suite shows no regression (this is a size/locality change, not expected to move the
score materially on its own, but must not regress it either); a cold continuation that
is *not* actually identical (different live-value set or calling convention) is
correctly excluded from merging, verified by a negative test.

Primary source: this is the standard "identical-tail merging"/cold-path deduplication
technique already implicit in [[161]]'s cited LLVM machine-block-placement discussion of
tail duplication's inverse; no new external citation needed beyond that existing
reference.
