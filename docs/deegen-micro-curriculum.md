# A gradual micro-corpus that proves the Deegen implementation, mechanism by mechanism

## Why this is separate from `quench-bench/micros/`

`quench-bench/micros/` (100 cases, `001.js`-`100.js`, family-organized:
numeric/control/arrays/objects/functions/strings/iterables/collections/
typed-memory/meta) is the project's **neutral regression corpus** — general,
representative JS shapes used to catch perf/correctness regressions across
*any* change. Tasks 027-037's gates all depend on this exact corpus staying
stable (100 cases, same families, same `verify.mjs` contract). Repurposing it
would break every gate already written. It stays as-is.

What's missing is a **different kind of suite**: not "does the VM handle
general JS," but "does each specific mechanism the Deegen paper describes
actually engage, in the order a VM would naturally acquire them, with
evidence beyond wall-clock time." Per the research into the paper's own
evaluation (§8): Deegen's authors validate via **44 whole-Lua-benchmark
comparisons and qualitative disassembly inspection** — they do *not* publish
per-mechanism ablation microbenchmarks or IC-hit-rate/tier-transition
counters. That's a real gap in their own methodology, one this project can
close: we have `execution_trace.rs`'s per-opcode counters and `machine.rs`'s
`tier_profile()` already built for exactly this purpose, just unused for
verification. This curriculum uses them.

## Directory and harness

New: `quench-bench/deegen-curriculum/`, numbered `001.js`, `002.js`, ...,
mirroring `micros/`'s manifest-driven pattern but with one addition per case:
an `expect` block declaring what instrumentation signal proves the mechanism
under test actually fired — not just that the output matched Node.

```json
// manifest.json entry shape
{
  "id": "007",
  "file": "007.js",
  "stage": "call-ic-monomorphic",
  "paper_section": "§3, §5.2",
  "mechanism": "Call site sees one stable callee across N calls; interpreter installs a monomorphic call IC.",
  "expect": {
    "observable": "oracle-match",
    "instrumentation": {
      "source": "execution_trace",
      "counter": "call_ic.guarded_hit",
      "assert": ">= N - 1"
    }
  }
}
```

New runner `deegen-verify.mjs` (same CLI shape as `verify.mjs`: `--engine`,
`--oracle`, `--from`/`--to`, `--out`) adds one step per case: run the engine
build with `QUENCH_EXEC_TRACE=1`, capture the JSON report
`execution_trace.rs` already emits, and assert every case's `expect.
instrumentation` clause against the parsed counters — in addition to the
existing oracle-match check `verify.mjs` already does. A case whose output
matches Node but whose instrumentation clause fails is a **fail**: it proves
the mechanism did not engage even though the program happened to compute the
right answer via a slower path (e.g. permanently falling back to the ordinary
interpreter). This is the actual point of the suite — output-correctness
alone already exists in `micros/`; this corpus exists to prove *mechanism
engagement*, not just correctness.

Every scenario must still be a **general, neutral JS program** whose runtime
shape naturally exercises the mechanism at realistic iteration counts — never
a program that detects the engine or hardcodes VM internals, per
`docs/benchmark-integrity.md` and `AGENTS.md` rule 12 ("never optimize
through observable JavaScript behavior"). A monomorphic-call-IC case is
"call the same function 500 times in a loop," not "call
`__quench_internal_probe()`."

## Stage curriculum

Ordered so each stage's cases only pass once the prior stage's mechanism is
correctly in place — `001.js` is the simplest possible interpreter dispatch
proof, and later stages compose on it. Case counts per stage are estimates;
build as many as needed to cover each mechanism's degrade/fallback states,
not a fixed total.

### Stage 1 — Interpreter dispatch baseline (paper §2-3) — ~6 cases
Straight-line arithmetic, a branch, a loop, a plain function call, a thrown/
caught exception, a generator's `next()`/`return()`. **Instrumentation**:
per-opcode dispatch counters (`execution_trace`) equal the expected static
instruction count × iteration count exactly — proves no double-dispatch, no
skipped handler, no silent fallback to a debug/slow interpreter.

### Stage 2 — Call inline caching (paper §3) — ~4 cases
(a) One callee called N times: expect monomorphic hit ratio ≈ 100%.
(b) 2-3 rotating callees: expect bounded-polymorphic chain, not degrade.
(c) Unbounded distinct callees (a closure factory per call): expect degrade
to the fallback path past the bound — and correctness preserved throughout.
(d) A call site that starts monomorphic then permanently switches callee:
expect a single re-install, not repeated thrashing.

### Stage 3 — Generic IC λi/λe, property access (paper §4, §5.2) — ~5 cases
Mirrors stage 2's shape for property get/set: stable shape (monomorphic),
small rotating shape set (polymorphic, up to `MAX_MISSES`/degrade-ladder
bound in `quickening.rs`), many distinct shapes (megamorphic ceiling),
correctness preserved at every degrade step, and one case exercising both a
hit and a guard invalidation (add a property mid-loop) to prove the λi
re-probe actually re-runs rather than trusting a stale λe.

### Stage 4 — Type-check elimination / region-stencil admission (paper §5.1; `docs/copy-and-patch-jit.md`) — ~4 cases
A numeric-add-heavy loop that should admit the fused Number-Add+Return
region. **Instrumentation**: `RegionKey` selection/hit counters
(`stencil_select.rs`/`stencil_lifecycle.rs`) show the region actually
rendered and was hit, not silently falling back to generic per-op dispatch.
Pair each positive case with a "hostile" counterpart — mixed numeric/string
operands that must defeat the Proven-fact admission — verifying the guard
correctly refuses the region and falls back to the complete ordinary path
with correct output (this is the fallback-safety half of the same proof).

### Stage 5 — Tier-up threshold (paper §3; `machine.rs` `TierState`) — ~3 cases
A function invoked exactly at, one below, and one above the retired-bytecode
threshold (`threshold: 32` today). **Instrumentation**: `tier_profile()`
shows `ExecutionTier::Interpreter` below threshold and `::Baseline` at/above
it, at the exact invocation boundary — an off-by-one here is a real bug this
stage exists to catch.

### Stage 6 — OSR-entry (paper §7.1) — ~3 cases
The paper's own motivating case: a function called **once**, containing a
loop hot enough to justify JIT compilation mid-execution — tier-up-on-entry
can never fire here since there's no second invocation. **Instrumentation**:
`tier_profile().osr_entries > 0`. Pair with a `for...in`/structured `ForI`
loop case that must correctly *not* attempt OSR (per the documented
exclusion) and still produce correct output via the ordinary path.

### Stage 7 — JIT-side polymorphic IC / inline slab (paper §7.1; gated on task 033) — ~4 cases
Once task 033 lands: a call/property site inside already-tiered-up code
seeing 2-4 distinct shapes/callees. **Instrumentation**: stub-chain length
counter and inline-vs-outlined placement (new counters task 033 should
expose, mirroring stage 2/3's shape one tier up).

### Stage 8 — Layout/codegen optimizations (paper §5.3, §6.1, §7.2; gated on tasks 030/031/034) — ~3 cases, best-effort verification
Tag register optimization, register pinning, and hot-cold splitting are
codegen-layout changes with no JS-observable signature — the paper itself
evaluates these qualitatively (disassembly inspection), not via
microbenchmark counters (confirmed: no ablation studies in the paper for
these). Verify via `architecture-size-report`/code-size deltas and, where
feasible, `perf stat` icache/branch-miss counters rather than
`execution_trace` — document this stage's verification as best-effort and
say so plainly rather than inventing a synthetic pass/fail signal.

### Stage 9 — Full-system closure workloads — ~6-10 cases
General, neutral programs (object-oriented traversal, recursive algorithms,
polymorphic dispatch mixed with hot loops) that exercise several mechanisms
*simultaneously* and at realistic scale — this project's own analog to the
paper's 44-whole-benchmark evaluation, proving the mechanisms compose
correctly under one program rather than only in isolation. **Instrumentation**:
assert every stage 1-7 counter that's relevant to the program's shape is
non-zero, plus oracle-match correctness — a program complex enough to
plausibly hit tier-up, OSR, and polymorphic ICs that doesn't trigger all
three is itself a finding.

### Stage 10 — Fallback-safety adversarial cases — ~5 cases
Deliberately construct inputs that must fail every fast path at maximum tier
(a `Proxy`-wrapped object defeating shape assumptions, a getter with a
side-effect that must run exactly once, a call target reassigned via
`arguments.callee`-style aliasing) — proving `Guarded`/`Unknown` correctness
holds even when every optimization is engaged, per `AGENTS.md` rule 12. This
stage exists because a suite that only proves optimizations fire without also
proving they fail safe is incomplete evidence.

## Calibration findings (2026-09, against a real build)

The suite was built with guessed instrumentation thresholds, then actually
run against a compiled `quench-node --features execution-trace` and
recalibrated against real numbers. Two categories of finding came out of
that pass, both recorded in `manifest.json`'s `instrumentation_gaps` and
reflected in the current `expect` clauses:

**Instrumentation gaps** (counters that don't move the way a naive model
predicts, so the affected cases are marked correctness-only rather than
asserting something false): bounded-polymorphic and multi-argument-shaped
recursive call sites don't move `quickening.Call` (cases 007, 029); per-access
property-read counters don't scale with iteration count once a site
stabilizes (cases 011, 015). The lane-share proxy
(`lanes.l2.vm_share_ppm` / `lanes.l3.handlers`) proved the most reliable real
signal across the board and is now the primary source for stages 1 and 4.

**Real performance findings** (not suite bugs — genuine gaps this suite exists
to surface, expected given tasks 027-037 are still in progress): running the
full 38-case suite with `--wall-ratio-max 3 --rss-ratio-max 1.5` against Node,
RSS is consistently ~0.5x Node (genuinely lighter representation) but several
cases run far past the 3x wall-time bar — recursion (case 010: fib(22), 15x;
case 029: ackermann, correctness-only but slow), megamorphic property access
(case 014: 400 distinct shapes, 11x), single-invocation hot loops (case 025:
35x; case 027: nested loops, 29x), and general closure workloads (case 028:
tree traversal, 47x; case 032: string/regex processing, 30x). These track
directly onto the still-open backlog items (027/028 tier-up+OSR verification,
029 slow-path outlining, 032 quickening, 033 JIT-side IC) — the curriculum
proves the gap exists, closing it is VM work for that backlog, not a change
to the suite itself.

**Confirmed correctness-relevant performance bug**: case 026 (`for...in` over
a plain object) is not merely slow, it's a scaling cliff. Isolated
measurement: building 500 dynamic properties takes 0.55s (already ~20x
Node's near-zero, itself notable), but the same object enumerated with
`for...in` and read back takes 5.73s total — i.e. the enumeration step alone
costs ~5.2s for 500 keys, versus Node's entire build+enumerate run at 0.026s
(~200x slower for the enumeration step in isolation). The case's actual
n=3000 body exceeds any reasonable timeout entirely. Strongly suggestive of
an O(n²) or worse cost in property enumeration or the underlying shape/slot
lookup it depends on — reported to codex as a VM-level fix, not something
this suite's tuning can address.

## What "done" means for this curriculum

Not a fixed case count. Done means: every mechanism in
`docs/deegen-alignment.md`'s status table (once all Present) has at least one
positive case proving it engages and, where the mechanism has a degrade/
fallback path, at least one case proving that path is also correct. Record
the final case count and per-stage breakdown in `docs/architecture-evidence.md`
alongside the neutral-corpus snapshot, not as a substitute for it.
