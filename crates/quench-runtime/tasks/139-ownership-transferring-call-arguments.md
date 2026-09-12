# 139 — Ownership-transferring call arguments

Status: complete

The accepted Task 137 native Earley profile is dominated by recursive
`dyn_block_step_impl -> Vm::call_arguments -> DynJitCode::run_with_locals` chains.
Task 125 removed the needless `Rc` clone of the callee, but every receiver and argument
still clones a one-word tagged `Value` from a caller register and later drops the
short-lived caller copy. Heap values therefore retain/release on every recursive edge.

Derive ownership transfer as call-site metadata from bytecode liveness. A receiver or
argument register may be moved into the callee only when it has no other semantic read,
does not alias the borrowed callee, and is not duplicated within the argument vector.
The caller slot is replaced with `Undefined`, preserving the owned-register invariant.
The normal borrowed/cloning argument view remains the identity fallback for aliases,
live values, native calls, and observable `arguments` materialization.

This is a higher-level call morphism, not a benchmark pattern and not a hotness policy:
the same semantic `Call` coproduct chooses an ownership-preserving or
ownership-transferring argument view from immutable liveness facts. No interpreter
fallback is introduced.

Acceptance: unit tests for aliases, duplicate arguments, live-after-call values, native
and user calls; release suite and full smoke; counters or a native profile proving the
transfer arm executes; alternating full-suite A/B against Task 137. Reject and revert
unless aggregate score improves without crossing the standing per-suite floor.

## Result

Implemented, measured twice, rejected, and reverted. The candidate derived CFG-aware
per-call transfer plans at link time; it rejected callee aliases, duplicate arguments,
live-after-call values, and functions containing exception handlers. Parameter binding
could then replace an eligible caller register with `Undefined` and take its owned
`Value`. All 77 candidate release tests and the complete V8v7 smoke passed.

The gated runtime counter proved the arm was active: a 20 ms Earley-Boyer run linked
1,438 eligible call inputs and executed 2,917,284 ownership transfers. The first
four-repetition comparison was +0.44%, but it contained severe system outliers. The
longer six-repetition confirmation in
`reports/task139-call-transfer-full-ab-6/comparison.txt` was 1182.02 baseline versus
1168.25 candidate, or -1.16%. The immediate-number-heavy workload made cloning already
a one-word copy, while plan loads and branches taxed every call. The exact Task 137
source and binary SHA-256 were restored. Evidence is retained in
`reports/task139-call-transfer-smoke.jsonl`,
`reports/task139-earley-call-transfer-stats.txt`,
`reports/task139-call-transfer-full-ab-4/`, and
`reports/task139-call-transfer-full-ab-6/`.
