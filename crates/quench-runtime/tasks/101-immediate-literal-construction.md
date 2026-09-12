# 101 — Split immediate and allocating literal construction

Status: complete

[[98-inline-literal-construction]] rejected force-inlining the entire five-way helper
because it copied the allocating string arm into the semantic block kernel. Native
samples after [[100-immediate-slot-overwrite]] still attribute 103 Crypto and 81
Navier-Stokes top-of-stack samples to `literal`.

Keep the bytecode representation unchanged, but make its coproduct explicit at the
execution boundary: undefined, null, boolean, and number literals construct their
single tagged word inside the semantic kernel; string literals call one out-of-line
allocation kernel. The generic string branch remains canonical and no source or
benchmark pattern participates in selection.

Acceptance: 42 release tests and full smoke pass, executable size is recorded, and an
exact alternating full-suite A/B against [[100-immediate-slot-overwrite]] clears the
aggregate and component floors. Reject and revert otherwise.

## Result: accepted

Immediate literals now construct directly in the singular semantic executor;
`allocate_string_literal` is the sole non-inlined allocating branch. All 42 release
tests pass and `reports/immediate-literal/smoke.jsonl` covers all suites. The executable
grows by 32 bytes, from 2,990,672 to 2,990,704 bytes.

The exact alternating four-repetition comparison in
`reports/immediate-literal-ab/comparison.txt` improves aggregate score from 772.287 to
776.862 (+0.59%). Crypto improves 4.02%, Navier-Stokes 2.75%, Earley-Boyer 1.09%,
DeltaBlue 0.12%, and RayTrace 0.49%; Richards (-0.92%), RegExp (-0.66%), and Splay
(-2.00%) remain within the component floor. The split is retained.
