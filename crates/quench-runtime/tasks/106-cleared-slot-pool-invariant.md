# 106 — Cleared slot invariant for register and environment pools

Status: complete

Function exit currently `clear()`s the entire register vector, invoking generic `Drop`
for every immediate slot, and the next call resizes/refills it with `undefined`.
Environment release fills every binding with `undefined`, then acquisition fills the
same bindings a second time. Native samples after [[103-owned-slot-overwrite]] still
place `Value` drop glue and `DynJitCode::run` among the main Crypto/Navier costs.

Define one pool invariant: every retained slot is the immediate `undefined` value.
Release establishes it with [[100-immediate-slot-overwrite]]'s canonical ownership
eliminator, releasing heap values exactly once. Acquisition derives readiness from that
invariant; growth initializes only new slots, and shrinkage changes length without
dropping already-cleared immediate words.

This is `state = memo(f(events))`: pool state is derived from the release event rather
than defensively reconstructed at both edges. Acceptance requires ownership/pool tests,
all release tests, complete smoke, and an exact alternating full-suite A/B against the
[[103-owned-slot-overwrite]] binary. Revert and record rejection otherwise.

## Result: accepted

Register and reusable environment pools now have one explicit invariant: all retained
slots contain immediate `undefined`. Release establishes it with `Value::overwrite`;
acquisition only initializes growth, and safely shortens an already-cleared vector
without running meaningless drops. The pool ownership test verifies heap release and
undefined state after reuse. All 44 release tests pass, all eight suites pass in
`reports/cleared-slot-pool-smoke.jsonl`, and the executable remains 2,990,704 bytes.

The exact alternating six-repetition comparison in
`reports/cleared-slot-pool-ab-6/comparison.txt` improves aggregate score from 753.334
to 809.793 (+7.49%). Richards improves 10.86%, DeltaBlue 3.24%, Crypto 6.14%, RayTrace
6.97%, Earley-Boyer 15.95%, RegExp 10.50%, and Splay 10.34%. Navier-Stokes regresses
2.96%, inside the standing component floor. The invariant is retained.
