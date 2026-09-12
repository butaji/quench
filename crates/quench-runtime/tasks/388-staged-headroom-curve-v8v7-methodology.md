# 388 — Staged headroom curve: score `deegen-curriculum` V8v7-style against Node/Bun

Status: planned

Companion to [[387-adopt-deegen-curriculum-as-calibration-foundation]], using the same
38-case, 8-stage graduated corpus for a distinct purpose: [[387]] adopts it for
mechanism-engagement/correctness calibration; this task scores it for *performance
headroom*, stage by stage, against Node and Bun, using the same methodology
[[15-v8v7-10000-gate]] already trusts — so the result is directly comparable to, not a
parallel invention alongside, the existing V8v7 score.

**Why a whole-suite score alone is too coarse for this purpose.** [[15]]'s aggregate
(currently ~23-24% of target) blends eight suites, each exercising many mechanisms at
once — a single number cannot say *at which complexity tier the gap actually opens up*.
`deegen-curriculum`'s staged structure (interpreter-dispatch → call-IC →
property-generic-IC → fast-path-kernel-admission → tier-up/OSR → full-system-closure)
gives a controlled complexity axis nothing in the current scored corpus provides: if
this VM tracks Node/Bun closely through stages 1-2 and falls off sharply at stage 3
(property-generic-IC), that is a precise, high-resolution signal pointing at exactly
which mechanism family dominates the remaining gap — sharper than [[253]]'s whole-suite
native-profile percentages, and cheap to re-run after any candidate lands.

**Same scoring formula as [[15]]'s own gate, for direct comparability.**
`scripts/run-v8v7.sh:57` computes the suite aggregate as a geometric mean of per-suite
ratio scores: `exp(mean(log(per_case_score)))`, each per-case score derived from a
declared reference time over measured time. Reuse this exact formula for the curriculum,
computed once per stage (geometric mean across that stage's case scores) and once
overall, rather than inventing a differently-shaped metric that would need its own
justification for comparability to [[15]].

Concrete steps:
1. Run each `deegen-curriculum` case under this project's own release binary, Node, and
   Bun, using the same alternating-sample/independent-process discipline [[00]] and
   [[253]] already require (not a single noisy run).
2. Compute a per-case ratio score against each comparator (deegen/Node, deegen/Bun)
   using [[15]]'s own reference-time-over-measured-time formula; aggregate to a
   per-stage geometric mean, then an overall geometric mean across all 8 stages.
3. Produce the headroom curve as the actual deliverable: one table/chart, stage on one
   axis, ratio-to-Node and ratio-to-Bun on the other, so the complexity tier where this
   VM's relative standing drops is visible directly rather than inferred from prose.
4. Cross-reference the resulting curve against [[301]]'s tier ordering: if the curve
   confirms Tier 0/1's boundary hypothesis (a sharp drop at property-IC or call-IC
   stages, matching the generic-executor/call-frame boundaries [[301]] already
   prioritizes), that is independent, stage-resolved corroboration; if it instead shows
   the drop at a different stage (e.g. full-system-closure specifically, suggesting a
   composition-level rather than single-mechanism issue), that is new evidence [[301]]
   should incorporate into its next review-trigger revision.

Acceptance: every curriculum case has a computed ratio score against both Node and Bun,
using [[15]]'s exact scoring formula; a per-stage and overall geometric-mean headroom
curve is produced and stored as a reusable artifact (re-runnable after any candidate
lands, the same way [[253]]'s profiles are re-run per [[301]]'s review triggers); the
curve's shape is explicitly compared against [[301]]'s current tier hypothesis and
either confirms it or is used to revise it, with the specific stage-level evidence cited.

Source: `scripts/run-v8v7.sh:57` (this project's own existing scoring formula, reused
for consistency); `../quench/quench-bench/deegen-curriculum/manifest.json` (the staged
corpus, per [[387]]).
