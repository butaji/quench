# 97 — Inline the register write barrier

Status: complete

Test forced LLVM inlining of the tiny `put` register assignment into the semantic block
kernel. Symbol inspection shows the generic helper still survives as a call from most
value-producing bytecodes. Inlining exposes the old tagged value's retain/release drop
test and the destination address to surrounding operation code, potentially eliminating
a call at nearly every opcode while keeping one ownership implementation.

Acceptance: ownership and semantic tests, full smoke, executable-size record, and a
balanced full-suite A/B. Revert if duplicating the drop barrier damages code locality.

Result: `put` is forced inline. Forty-one release tests and the full smoke in
`reports/put-inline/smoke.jsonl` pass. The executable shrinks slightly from 2,990,736 to
2,990,672 bytes because the standalone helper disappears without net code duplication.
Four-repetition balanced A/B in `reports/put-inline-ab/comparison.txt` raises the
aggregate 702.166→706.908 (+0.68%); Navier-Stokes improves 4.32%, and all suites remain
inside the standing floor.
