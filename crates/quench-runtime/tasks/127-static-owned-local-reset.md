# 127 — Static ownership set for local-frame reset

Status: complete

Task 13 made non-capturing lexical frames plain pooled vectors, but every return still
walks every binding through generic `Value::overwrite`, including the heap-tag branch
for slots whose bytecode producers can only create immediates. Sampling after Task 125
attributes 84 Richards and 101 DeltaBlue samples to `release_local_values`, so teardown
is now a measured call-path cost.

Derive a conservative least fixed point over the immutable bytecode: register producers
and local stores propagate the property “may own a heap value,” while parameters,
`this`, property/call results, strings, objects, closures, regexps, catch values, and JS
addition seed it. Store only the resulting sorted local-slot set in `DynJitCode`. At
release, drop those possible owners, then restore the entire one-word frame to the
canonical `undefined` bit pattern with ownership-free writes. No analysis or tracking
is added to the hot opcode path.

Acceptance: fixed-point unit tests cover immediate-only, transitive local/register
propagation, and heap-producing slots; ownership tests prove all `Rc` values are
released and every pooled slot is undefined on reacquisition; all release tests and
complete V8v7 smoke pass. Accept for performance only if a stable alternating full or
focused benchmark improves without violating the component floor; otherwise revert the
runtime change and retain the measurement here.

## Result: measured and reverted

The conservative fixed point, sparse sorted slot set, specialized reset, and ownership
tests were implemented. All 64 release tests passed. A release smoke and a debug smoke
(whose assertion checked that no unclassified heap owner remained) completed all eight
suites. The candidate binary was `/tmp/deegen-task127-static-owned-local-reset`, SHA-256
`d0f78c184859a9efa12246f092e7516506b3da858ace6bc98a3f7b6cc8d07955`.

An initial focused run in `reports/task127-static-owned-local-reset-focused-ab-6` was
invalidated by unrelated host contention: scores ranged from 223 to 648. Once idle, the
same six-repetition, 250 ms comparison in
`reports/task127-static-owned-local-reset-focused-stable-ab-6/comparison.txt` measured
only +0.43% across Richards and DeltaBlue. The four-repetition full comparison in
`reports/task127-static-owned-local-reset-full-ab-4/comparison.txt` measured +0.15%
aggregate, with mixed uncontended suite directions and one externally contended final
repetition. That signal is too small for the analysis, metadata, and second clearing
pass. The runtime and tests were reverted completely to Task 126; the result narrows the
next search toward coarse execution regions rather than frame teardown.

Task 338 re-evaluated the idea after reusable activations made reset visibly hot. Its
broader register-aware plan passed correctness and initially improved a targeted screen,
but the stable complete comparison regressed 1.43%. The contiguous reset remains canonical;
do not retry sparse teardown without hardware evidence that changes the scattered-access
cost model.
