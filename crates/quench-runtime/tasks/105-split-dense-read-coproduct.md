# 105 — Split dense computed-read fast path from generic fallback

Status: complete

[[99-inline-dense-array-connector]] rejected force-inlining the entire computed-property
helper because it duplicated string conversion, prototype lookup, and all receiver-type
fallbacks into the already-large semantic kernel. Native samples after
[[103-owned-slot-overwrite]] still show `get_computed_prop` as the largest named helper
under Navier-Stokes.

Factor computed reads into a coproduct eliminator: a small inlined numeric-index +
dense-array injection and a non-inlined generic fallback. Both the public semantic
operation and stencil semantic kernel use the same two canonical functions; no property
semantics are duplicated and no source identity participates.

Acceptance: all release tests and full smoke pass, then an exact alternating full-suite
A/B against the [[103-owned-slot-overwrite]] binary clears aggregate and component
floors. Revert and record rejection otherwise.

## Result: rejected and reverted

All 43 tests and `reports/split-dense-read-smoke.jsonl` passed. The exact alternating
six-repetition comparison in `reports/split-dense-read-ab-6/comparison.txt` regressed
aggregate score from 787.417 to 785.874 (-0.20%); Crypto regressed 2.33% and
Navier-Stokes 1.83%. Inlining the borrow/index/clone path into the monolithic semantic
switch costs more in code shape than the removed call. The split was removed. Dense
access must instead be fused with surrounding loads/arithmetic in a coarse stencil or
loop morphism, keeping the central dispatch kernel compact.
