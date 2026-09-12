# 338 — Sparse reusable-activation reset experiment

Status: complete

Re-evaluate Task 127's static ownership reset after Task 336 made activation reuse the
default monomorphic call path. A three-second Richards sample of the accepted binary at
`reports/task338-richards-after-activation-cache.sample.txt` attributes 209 top-of-stack
samples to `reset_value_slots`, second only to the general block executor. Frame allocation
is nearly absent, so this was a materially changed profile from Task 127's original test.

The candidate derived a conservative possible-heap result fact from the canonical
`DynOp` sum. Reusable activations fully cleared locals and name snapshots, cleared only
registers whose producers could yield a heap-tagged value, and left definition-dominated
numeric/immediate virtual registers untouched. The immutable reset plan was stored once
with the function image; no hot-path ownership tracking, execution counter, or benchmark
rule was added. Debug validation required every inactive activation to contain no heap
tag, and a unit test covered string, `typeof`, JS addition, and immediate arithmetic
classification.

All 125 debug tests and 125 forced-GC release tests passed. The five-pair call-heavy
screen at `reports/task338-sparse-reusable-reset-targeted-ab-5/comparison.txt` appeared
positive at 1432.17 -> 1464.98 (+2.29%), but the complete gate did not confirm it. The
five-pair, 500 ms full result at
`reports/task338-sparse-reusable-reset-full-ab-5/comparison.txt` measured **2121.83 ->
2091.55 (-1.43%)**. Richards fell 1.08%, DeltaBlue 1.98%, RayTrace 3.28%, Earley-Boyer
2.23%, RegExp 1.20%, and Navier-Stokes 1.97%; only Crypto (+0.06%) and Splay (+0.32%)
were neutral-positive.

Candidate SHA-256:
`afc722813a094c006dc43152c7e4a53173ab86e5b374144740f05c1252486a2c`.

The candidate was reverted. This falsifies using sampled leaf weight alone as a benefit
estimate: the saved sequential writes were replaced by a pointer/count load and scattered
cleanup accesses, worsening the real critical path. Keep the canonical contiguous reset.
The next optimization must erase the complete call/return morphism or a semantic region,
not specialize another part of activation cleanup.
