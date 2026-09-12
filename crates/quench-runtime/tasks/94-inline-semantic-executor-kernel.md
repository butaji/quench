# 94 — Inline the semantic executor into block kernels

Status: complete

Test LLVM-forced inlining of the single canonical `execute` bytecode semantics function
into the immutable staged block-kernel variants. The current generic block loop makes a
separate Rust call for every bytecode; inlining may expose register, next-PC, and
disabled-effect simplifications across the dispatch boundary. This does not duplicate
semantics and does not add workload-specific templates.

Acceptance: tests and full smoke pass; binary/code-size cost is recorded; a balanced
full-suite A/B satisfies the standing gates. Revert if instruction-cache pressure or
LLVM code shape outweighs the removed call.

Result: `execute` and its effect-refresh wrapper are `#[inline(always)]`, while the
semantic definition remains singular. The release executable grows from 2,940,720 to
2,974,224 bytes (+33,504, 1.14%); immutable kernel text is shared and does not enlarge
each linked stencil instance. Forty release tests and the full smoke in
`reports/execute-inline/smoke.jsonl` pass. Four-repetition balanced A/B in
`reports/execute-inline-ab/comparison.txt` raises the aggregate 590.340→690.091
(+16.90%); every suite improves, led by Navier-Stokes +42.15% and Crypto +27.54%.
