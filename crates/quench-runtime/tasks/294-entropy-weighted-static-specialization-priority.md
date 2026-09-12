# 294 — Entropy-weighted static priority for guard/stencil-family selection

Status: planned

**Scope guardrail, stated first because it is the whole point:** this task uses
information theory (Shannon entropy) as a *static* prioritization metric computed from
already-collected offline corpus profiles ([[83-runtime-block-frequency-profile]],
[[132-residual-generic-block-profiler]]), never as an online, per-execution adaptive
signal. Several superficially related ideas — runtime hotness counters, Bayesian
per-workload confidence updating, online learning of specialization thresholds,
control-theoretic dynamic budget adjustment — are explicitly **out of scope** and
incompatible with this project's standing no-tier/no-hotness-threshold constraint
([[45-no-tier-first-execution]], [[153]]'s explicit rejection of CPython's counter-gated
quickening). This task is a different thing: a fixed, deterministic formula computed
once from static corpus data, used to *rank already-identified* candidate work — not a
mechanism that changes VM behavior based on what it observes at runtime.

**The idea, scoped correctly.** A dynamically-typed value at a program point carries
uncertainty about its actual runtime shape/type — model this as a distribution over
possible concrete types/shapes at that point (estimated from the static corpus profile,
the same data [[81]]/[[82]]/[[132]] already collect) and its Shannon entropy
`H = -Σ p(x) log p(x)`. A guard exists specifically to resolve this uncertainty; a fully
monomorphic site (one shape with probability 1) has `H = 0` and is exactly [[231]]'s
zero-check-erasable case; a megamorphic site has high `H` and is exactly what
[[159]]/[[200]]'s dictionary/megamorphic fallback exists for. The candidate metric:
prioritize new stencil-family work by *entropy reduced per unit of implementation cost*
— `ΔH / cost`, where `cost` is [[157]]'s existing dynamic-operations-removed-based cost
measure, not a new one to invent. This is a specific, principled refinement of
[[166-ic-recipe-health-scoring]]'s existing site-scoring ambition, not a competing
mechanism: [[166]] already wants to rank sites by cost; this task supplies a specific,
information-theoretically motivated numerator for that ranking instead of an ad hoc one.

Concrete steps:
1. Compute the type/shape-distribution entropy for the guard sites already profiled by
   [[83]]/[[132]], using the existing static corpus data — no new instrumentation, no
   runtime counters.
2. Rank currently-planned-but-unimplemented guard/stencil-family tasks by their
   estimated `ΔH / cost` against this static ranking, and compare the result to this
   project's actual historical task-acceptance order (recorded in [[15]]'s ledger) —
   confirming whether the metric would have predicted the highest-value work first, or
   identifying where it would have reordered priority.
3. If the metric's retrospective ranking meaningfully agrees with (or improves on) the
   actual historical order, adopt it as [[166]]'s stated scoring formula going forward;
   if it does not, document why (e.g. `cost` estimation error, distribution estimation
   error from a small/unrepresentative corpus) rather than discarding the idea silently.

Acceptance: entropy is computed for a representative set of already-profiled guard
sites from static corpus data alone; a retrospective comparison against this project's
actual historical prioritization order is produced and its agreement (or disagreement)
stated honestly; [[166]]'s scoring formula is either updated to use this metric or an
explicit, evidenced reason is given for not doing so; no runtime/online adaptation
mechanism is introduced anywhere in this task's scope, verified by confirming the
computation happens entirely at AOT/offline analysis time.

Primary sources:
- Shannon, *A Mathematical Theory of Communication* (1948) — the entropy definition
  this task's metric is built from.
- This task's numerator/denominator reuse [[157]]'s and [[166]]'s existing cost-measure
  citations rather than introducing a new one.
