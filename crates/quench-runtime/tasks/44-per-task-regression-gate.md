# 44 — Standing per-task A/B regression gate

Status: planned

`scripts/perf-cycle.sh accept` already performs alternating baseline/candidate A/B measurement, currently used as the final acceptance mechanism at [[15-v8v7-10000-gate]]. Wire the same `accept` invocation to run automatically whenever any item in `tasks/index.json` moves to `complete`, rather than only at the final full-suite gate.

This matters more than usual here because much of this plan depends on subtle categorical proofs — guard-connector typing ([[19-guard-typed-connectors]], [[25-generalized-speculative-guards]]), e-graph rewrite soundness ([[23-egraph-rewriting]]), associativity-based hoisting ([[21-loop-invariant-hoist-associativity]]) — staying semantically transparent as they compose. Catching a regression at the task boundary where it was introduced is far cheaper than catching it at task 15 after several more tasks have built on top of a broken one.

Acceptance: marking a task `complete` in `tasks/index.json` triggers (or requires, if run manually) an `accept` run recorded alongside that task's completion; a task cannot be marked `complete` with a recorded regression against the prior accepted baseline; the gate's history is inspectable per task, not only as one final pass/fail at task 15.
