# 00 — Repeatable optimization routine

Status: complete

Use one loop for every optimization: identify a measured bottleneck, preserve a baseline binary, make one coherent change, run correctness tests, alternate candidate/baseline samples, accept only a repeatable gain, then update this ledger.

Before implementation, require a reach-and-boundary preflight. Record the dynamic event
count or sampled share the candidate can affect, name the complete host/helper/frame/guard
boundary it removes, and state the expected hot-path disassembly. A candidate that merely
changes metadata or substitutes one Rust callback for another does not pass preflight.
After Tasks 352 and 353, a call optimization below roughly one million executions per
200 ms suite window is diagnostic work unless it composes into a larger boundary-erasing
region. This is a prioritization floor, not a source-specific selector or runtime threshold.

**Mandatory ordering, not optional guidance:** work proceeds bottom-up through
[[387]]/[[388]]'s staged complexity axis, measuring and improving one stage before the
next becomes a priority — the simplest load (`deegen-curriculum` stage 1: straight-line
arithmetic/branch/loop/call/exceptions) first, then call-IC, then property-generic-IC,
and so on through full-system-closure. [[398-bottom-up-stage-gated-optimization]] is
the standing rule this preflight enforces, not a separate suggestion a contributor may
skip: a candidate targeting stage N+1 while stage N's measured ratio-to-Node/Bun (per
[[388]]'s headroom curve) remains below its named threshold does not pass preflight
unless it explicitly argues, per [[398]]'s exception clause, that its target is
genuinely independent of stage N's slack. A stage does not count as finished merely
because tasks were attempted against it — per [[398]]'s status taxonomy, it is finished
only at `threshold-met`; running out of cheap candidates before the threshold is a
signal to find a harder, structural one for that same stage, not to move on. This
generalizes the "one million executions"
reach floor above with a complexity-tier floor: reach alone is not sufficient
justification to skip ahead of an unsettled simpler stage.

Evidence: `scripts/perf-cycle.sh` supports smoke, record, compare, accept, profile, and rank operations with structured result records. `scripts/run-v8v7.sh` supports an explicit engine binary and sample count.

`scripts/check-task-ledger.sh` enforces the inventory mechanically: task ids are
contiguous and unique; every indexed item has exactly one detail file and vice versa;
filename/heading/title/status agree; statuses are valid; and dependency ids exist.
This makes the manifest the one canonical fact and derives integrity from it.

Next use: every task marked as an optimization must attach its preflight reach, expected
boundary removal, before/after result, and regression explanation before its status becomes
complete.

This routine governs how to validate *one already-chosen* change. For which task to pick up next out of the full backlog, and how to tell when that priority ordering itself needs revisiting, see [[301-strategic-priority-tiers-and-review-methodology]].

Research round thirty-five found that the existing complete-suite development runner is not
upstream-equivalent: it launches a fresh process per suite and aggregates formatted scores,
where upstream V8v7 retains all suites in one process and aggregates raw timing/reference
ratios after its warmup and measurement protocol. Until [[365]] lands, short and split-suite
scores are diagnostics, not a final proof of the 10000 objective. The accepted final lane
must use at least nine randomized process-level A/B pairs and confidence intervals; one
upstream-shaped process retains the fixed suite order.

Task 365 now supplies that exact lane. Task 375's accepted candidate is the current image;
its clean source-rebuild one-process raw score is 2404.3308063773734 (the preserved
accepted artifact scored 2375.0906728638083 in a separate standalone run). Task 373's standalone
2391.135445752378 remains historical evidence, not proof that the older image is faster:
single exact runs are visibly noisy. Task 375's nine-pair exact comparison is the current
acceptance evidence: +0.64% with a 95% paired interval of [+0.10%, +1.01%], on top of Task
373's accepted +2.42% [+2.05%, +2.86%]. Use `compare-exact` to accept incremental changes
and `accept`/`goal` for the 10000 objective gate.

Task 377 adds a selection stage before implementation. Aggregate residual blocks by the
set of missing capability families and rank singleton and joint families by weighted
whole-block closure under measured physical cost. This prevents a frequent opcode from
winning merely because it is frequent, and exposes complementary families that only pay
off together. Its first unit-cost report identifies call as the largest singleton frontier
at 4,060,419 entries and call plus name at 5,502,818; those are structural reach figures,
not predicted speedups. Physical tile costs remain to be imported before Task 377 is
complete. The planner is advisory; the exact A/B gate remains authoritative.

Task 378 validates that loop on a full implementation. Three eager snapshot realizations
of name coverage regressed by 5.84%, 5.22%, and 0.23% and were rejected. The lazy POD
lexical-address IC passed the nine-pair exact gate at +1.45% with a 95% interval of
`[+0.67%, +2.13%]`. The current accepted paired geometric mean is 2326.86 versus its
same-run Task 375 baseline of 2293.53. Treat this paired value as the current acceptance
estimate; it is still far below the 10000 objective.

Task 381 repeats the loop with a refreshed atom-level residual profile. Direct dense
computed stencils passed the nine-pair exact gate at +1.57%, 95% interval
`[+0.03%, +3.36%]`; the same-run candidate geometric mean is 2297.84. A complementary
bitwise/numeric-unary family looked positive in a three-pair screen (+1.14%) but failed
the exact gate at -3.45% `[-8.65%, +0.49%]` and was reverted. This is now the standing
example that a whole-block closure estimate is a preflight, not permission to retain a
larger stencil catalog without exact evidence.

Task 384 adds the complementary warning: reducing guard failures is not itself a speed
metric. Copy-safe property admission was catastrophically slow because it replaced one
coarse kernel with many tiny stencil transfers; rejecting those regions removed almost
all Crypto guard failures but still lost 0.83% in the three-pair screen. Both were
reverted. Task 382 now supplies `profile-pair` and `profile-delta`, but its retroactive
check also showed that top-of-stack samples cannot see iteration-count or anonymous-JIT
effects. Future preflight must combine counters, physical cover cost, and score—not treat
any one as sufficient.

Task 386 tests final-link relaxation independently of semantic coverage. Replacing an
adjacent `b +1` connector with a same-width `NOP` screened at +1.03% over five 200 ms
pairs but failed the exact gate at -0.07%, interval [-0.82%, +0.60%]. Removing the word
entirely was rejected at -1.33% aggregate and -11.34% Navier-Stokes: layout-preserving and
byte-compacting realizations are different physical candidates even when they denote the
same categorical identity edge, and neither was retained.

Task 389 adds a stricter reach-times-savings rule. Dynamic frequency alone is inadequate:
record the number of complete regions a candidate can select and the instructions,
loads, conversions, helper calls, or allocations removed per selected region. Coverage
instrumentation must not disable the direct path it claims to measure. A new leaf family
that increases catalog coverage but leaves frame/site materialization at every seam is
rejected at preflight; prefer one context-carrying composite that closes a measured
cross-operation or call dependency chain.

Ledger contract:

1. `tasks/index.json` is the canonical inventory of work-item identity, title, status, and dependencies.
2. Every manifest item has exactly one `tasks/NN-*.md` detail file, and every such detail file is indexed.
3. Planned work records scope and acceptance criteria. In-progress or completed work additionally records the actual implementation, evidence, measurements, and unresolved limitations.
4. Rejected experiments remain in the relevant work item with their measurements and reason for rejection; they are not rewritten as successes or silently removed.
5. Add or update the manifest and detail file together whenever work is discovered, started, measured, accepted, rejected, blocked, or completed.
