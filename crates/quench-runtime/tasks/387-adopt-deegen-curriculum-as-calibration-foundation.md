# 387 — Adopt `deegen-curriculum` as the graduated-complexity calibration foundation

Status: planned

**Do not build a new graduated-complexity load suite.** `../quench/quench-bench/deegen-curriculum/`
already is one: 38 instrumentation-verified cases across 8 named stages, staged from
bare interpreter dispatch through full-system closure and adversarial fallback-safety,
each explicitly mapped to a section of the Deegen paper itself (arXiv:2411.11469) via
`manifest.json`'s `paper_section` field:

```
1. interpreter-dispatch (§2-3, cases 1-5)       — arithmetic, branch, loop, call, exceptions
2. call-inline-cache (§3, §5.2, cases 6-10)      — mono → bounded-poly → mega → re-stabilize → recursion
3. property-generic-ic (§4, §5.2, cases 11-16)   — same progression for property access
4. fast-path-kernel-admission (§5.1, cases 17-21)
5. tier-up-threshold (§3, cases 22-24)           — correctness-only, instrumentation gap noted
6. osr-entry (§7.1, cases 25-27)                 — correctness-only, instrumentation gap noted
9. full-system-closure + adversarial fallback-safety (cases 28-38)
```

This is exactly the right foundation for [[331]]'s calibration-log gap identified in
this session: a latency/critical-path prediction model needs to be validated starting
from the simplest possible cases (where the predicted mechanism is unambiguous — stage
1's straight-line arithmetic recurrence, `001.js`) before trusting it on the full V8v7
corpus's compound, multi-mechanism suites. Grepping this project's own repository finds
**zero references to `deegen-curriculum` anywhere** — it is currently unused by this
project's own tasks despite existing specifically to validate Deegen mechanisms.

**Real calibration findings already recorded in its manifest, not hypothetical:**
`manifest.json`'s `instrumentation_gaps` field already documents that bounded-
polymorphic/multi-argument recursive call sites (cases 007, 029) do not move the
`quickening.Call` hit/miss counters as a naive model predicts, and per-access property-
read counters (`lanes.l0.property_hit/miss`, `quickening.GetN`) do not scale with
iteration count once a site stabilizes (cases 011, 015) — with `lanes.l2.vm_share_ppm`/
`lanes.l3.handlers` identified as the most reliable real signal instead. This is a
calibration-log entry that already exists, seeded from real measurement, that [[331]]'s
new calibration-log mechanism should absorb directly rather than rediscover.

Concrete steps:
1. Run this project's own release binary against `deegen-curriculum`'s cases via its
   existing `deegen-verify.mjs`/`execution_trace.rs` instrumentation path, confirming
   the harness actually works against this specific binary (not assumed from the
   sibling repo's own testing against whatever engine it was built for).
2. Seed [[331]]'s calibration log with the manifest's already-documented findings
   (`quickening.Call` unreliability, property-counter non-scaling, `lanes.l2`/`lanes.l3`
   as the reliable proxy) as its first rows, cited from this source rather than
   re-derived.
3. Use the staged structure directly as the incremental-complexity validation order for
   any new latency/critical-path prediction work: validate the model against stage 1
   (unambiguous mechanism) before stage 2, stage 2 before stage 3, and so on — a model
   that cannot correctly predict stage 1's straight-line arithmetic case has no business
   being trusted on stage 9's full-system-closure case.
4. Close the two documented instrumentation gaps (stage 5 tier-up, stage 6 OSR-entry
   have no live per-run counter, only correctness checks) if and when this project's own
   tier/OSR-adjacent work ([[22]], [[45]]) needs that signal — the manifest already
   states the exact fix (`tier_transitions`/`osr_fires` counters in
   `execution_trace.rs`), so this is a scoped, known addition, not open-ended
   instrumentation design.

Acceptance: this project's binary runs successfully against `deegen-curriculum` via its
existing verification tooling; [[331]]'s calibration log is seeded with the manifest's
existing findings rather than starting empty; at least one future latency-model
validation explicitly follows the staged order (simple mechanism first) rather than
being validated directly against a full V8v7 suite; the two noted instrumentation gaps
are either closed or explicitly deferred with the reason stated.

Source: `../quench/quench-bench/deegen-curriculum/manifest.json`,
`docs/deegen-micro-curriculum.md`, `docs/deegen-alignment.md` (sibling repository, read
directly).
