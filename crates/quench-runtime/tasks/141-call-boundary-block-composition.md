# 141 — Call-boundary basic-block composition

Status: complete

Task 140 halves sampled user-call dispatcher cost, but the accepted Earley profile still
places `dyn_block_step_impl` first. A `Call` inside a mixed basic block currently sends
the remaining block through the generic Rust loop because it terminates the directly
inlineable prefix. Evaluate splitting a quoted block at call boundaries into:

`direct prefix ; shared call-effect kernel ; direct suffix`.

Each piece remains `Connector -> Connector`; the call kernel owns the sole semantic
`Call` implementation and exceptional coproduct, and success tail-reenters the next
labeled bytecode. Compose only maximal direct runs around a call—do not recreate a
per-op connector chain. Selection is solely bytecode/effect structure, never source
identity, literals, property names, benchmark identity, or hotness.

First checkpoint: measure residual call blocks by lengths of their inlineable prefix and
suffix on the accepted Task 140 binary. Implement only if the eliminated generic work
substantially exceeds one effect reentry. Acceptance requires exact semantic tests,
direct/effect counters, full smoke, disassembly of the shared kernel and patched
continuations, and a confirmed alternating full-suite gain. Reject and revert otherwise.

## Result

Rejected and reverted. Complete eight-suite residual measurements from the accepted
Task 140 binary are recorded in `reports/task141-*-residual.txt` and summarized in
`reports/task141-call-block-frequency.tsv` and
`reports/task141-call-boundary-summary.txt`. They contain 428 distinct call-block
shapes, 2,515,289 dynamic call-block entries, and 3,532,614 dynamic calls. Of those
calls, 2,429,022 have at least one directly lowerable following opcode and 903,334 have
at least two; the suffixes contain 4,578,319 weighted direct opcodes in total.

The minimum legal composition—generic prefix through `Call`, followed by the existing
owned `Return` stencil—passed all 76 release tests and the complete smoke suite. Its
alternating full-suite comparison in
`reports/task141-call-return-full-ab-4/comparison.txt` regressed aggregate score by
2.22%, with Richards at -5.15%. The extra return boundary costs more than it removes.
The experiment was fully reverted; the release binary is again byte-identical to the
accepted Task 140 image. Any future call-boundary composition must recover a fixed
multi-op suffix in one stencil instance, rather than split off a single instruction.
