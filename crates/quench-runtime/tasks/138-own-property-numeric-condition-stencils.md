# 138 — Own-property numeric condition stencils

Status: complete

Lift Task 137's helper-free own-property primitive into the general condition grammar
`LoadLocal(value); LoadLocal(receiver); GetStatic(property); Binary(compare);
JumpIfFalse`. Task 132 measured 180,116 entries for the `Less` member across Splay and
RayTrace. Generate the numeric comparison family with one Rust macro so equality and
ordering members share the same guards, connector behavior, and slow coproduct.

The block is selected only when dataflow matches and every bytecode temporary is dead
outside its semantic use. Both operands must be numbers on the direct arm. The property
descriptor must match the receiver's canonical shape; otherwise execution tail-rejoins
the original block. Property spelling, source identity, and hotness are not selectors.

Acceptance: structural/liveness selector tests; rustc catalog and disassembly with no
helper call; release suite and complete smoke; residual evidence; alternating full-suite
A/B. Reject and revert if the aggregate does not improve.

## Result

Implemented and measured, then rejected and reverted. The candidate added one
macro-generated rustc/LLVM stencil per numeric comparison for the general five-op
grammar. All 77 release tests and the complete smoke suite passed. Cooked AArch64
disassembly confirmed a direct shape/slot/numeric guard and comparison with no `bl` or
`blr` helper call. On Splay, the candidate selected four more direct blocks and reduced
the residual `Lt` form from 171,913 to 82,843 entries and the `Gt` form from 87,774 to
42,961 entries, proving that the stencils were genuinely wired.

The full alternating four-repetition result in
`reports/task138-property-number-full-ab-4/comparison.txt` was nevertheless negative:
1168.92 baseline versus 1164.40 candidate, or -0.39%. Splay changed -0.46% and RayTrace
-1.65%. The per-block repeated numeric and shape guards cost more than the eliminated
dispatches, so selection coverage was not a valid proxy for speed. The implementation
was removed and the accepted Task 137 code restored. Supporting evidence remains in
`reports/task138-property-number-smoke.jsonl`, `reports/task138-splay-residual.txt`,
`reports/task138-splay-baseline-residual.txt`, and
`reports/task138-raytrace-residual.txt`.
