# 326 — Static guarded numeric basic-block regions

Status: in_progress

Extend the existing numeric-loop quote to straight semantic basic blocks. Every accepted
block is compiled on load as a categorical choice with exactly two compatible arrows:

`UnknownFrame --guard--> NumericFrame --region--> Successor`

or the existing general-purpose stencil range on guard failure. This is selected from
bytecode structure and abstract requirements only. It has no invocation counter, hotness
threshold, benchmark identity, property-name selector, or runtime LLVM dependency.

This is deliberately a precursor to [[144-static-bounded-context-versioned-stencil-regions]],
not a claim that full static BBV exists. It creates one specialized numeric version plus
one generic version for an eligible block; it does not yet propagate multiple successor
contexts through the whole CFG.

Current implementation:

- `numeric_region::quote_block` quotes blocks of at least
  `MIN_NUMERIC_BLOCK_REGION_OPS` operations and requires a numeric unary/binary operation;
- blocks overlapping an accepted loop region are excluded, so region ownership is unique;
- `NumericRegionLink` records whether the selected morphism is block- or loop-level;
- diagnostics distinguish `linked_block_regions` from all linked numeric regions;
- scalar number/index guards validate without constructing unused `NumberBinding` values;
- a focused test proves an eight-operation straight block quotes as a non-trace region.

Acceptance: full release and GC-stress tests pass; every suite remains semantically correct;
guard success/failure and selected-block diagnostics are recorded; a complete alternating
A/B determines retention. If entry-guard cost exceeds eliminated semantic work, narrow the
selector using named structural cost constants or reject and remove the runtime change.

## First measurement

The unused scalar-binding removal is included in `/tmp/deegen-task326-static-block-regions-v2`.
`reports/task326-static-block-regions-v2-full-ab-3/comparison.txt` records a three-pair,
300 ms complete-suite comparison against the Task 318 checkpoint:

- Crypto: +3.42%;
- Navier-Stokes: +6.43%;
- complete-suite aggregate: 2080.01 -> 2083.02 (+0.14%);
- Richards: -3.54%, with smaller regressions in DeltaBlue, RayTrace, and Splay.

This is not accepted as a score improvement. A 100 ms diagnostic found 398,211 guard
failures out of 1,023,939 Crypto block/loop entries and 57,815 failures versus three
successes in Earley-Boyer, while Navier-Stokes had 122,651 successes and only 63 failures.
The experiment remains in progress only as substrate for [[328]]/[[329]]: reduce the
quoted block first and lower its scalar guard product natively before another full A/B.

Primary source: Static BBV reports that two versions per block eliminate 54–62% of dynamic
type tests and improve execution by about 10% on its evaluated Scheme implementations,
without profile-driven selection:
<https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2024.28>.

## Task 384 admission result

Typed copy-safe property admission made more Crypto blocks enter small stencil sequences
but caused a severe slowdown. Statically rejecting those guaranteed-failing regions cut
Crypto guard failures from 160,922 to 698, yet still failed the full-suite short gate at
-0.83% aggregate and -6.86% Richards. Both variants were removed. This confirms the
acceptance criterion above: do not broaden or prune block versions from guard counts
alone. The next realization must compete a coarse cooked block instance against the
shared kernel using physical costs, then keep values in registers across the selected
coarse region. See [[384-typed-property-region-admission-experiment]].
