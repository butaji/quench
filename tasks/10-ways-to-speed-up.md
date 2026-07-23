# Strategy — fastest path to 100% test262 (min LOC)

Data-driven (per-stage counts live in `tasks/index.json`):

- **48,581 tests** across 122 stages: language 23,711 · built-ins
  23,668 · annexB 1,086 · harness 116.
- Done: ~2,154 (4%). In progress: stage 16 `class` — 4,367.
- Largest remaining: `expressions` 11,101 · `Temporal` 4,603 ·
  `class` 4,367 · `Object` 3,411 · `Array` 3,081 · `RegExp` 1,879 ·
  `TypedArray` 1,446 · `for-await-of` 1,234 · `String` 1,223 ·
  `Promise` 729.

Speed = fixes-per-week × tests-unlocked-per-fix. Rank levers by that
metric. End-state shape (small Rust + JS builtins) is fixed in
`docs/architecture.md`; **execution order** is what this file decides.

## Critical path (do in this order)

```
Phase A — clear language stages (now → ~stage 70)
  A1. R4 delete dead TComp          (~470 LOC, no blockers)
  A2. R5 object-model correctness   (unblocks class + Object + eval)
  A3. Stage-16 class via S2 digest  (root-cause clusters, not per-test)
  A4. R17 oxc_semantic early errors (language half, high tests/LOC)
  A5. Remaining language stages     (for-of, expressions, …)
      — grow R1 only for ops you touch

Phase B — before grinding built-ins (~stage 71 Object)
  B1. Finish R1 (%ops% owns impls, not re-exports)
  B2. R0 self-host builtins in JS   (Object first, then dependency order)
  B3. R2 one iterator protocol      (with R0 Iterator.js)

Phase C — built-ins → annexB → Temporal
  C1. Built-ins stages in JS        (never re-expand Rust builtins)
  C2. S4 async→generator            (for-await-of / Promise / Async*)
  C3. Temporal last                 (evaluate temporal_rs; S7)
```

Do **not** pause stage 16 for a full R0 migration. R0’s throughput
payoff starts at the built-ins half; R5 unblocks the stage you are on.

## S1 — Object model + stage digest before full R0 *(highest now)*

Stage 16 (`class`) and most language failures share one property
store. Land R4 → R5, then fix class by S2 failure clusters. Full R1→R0
is the highest lever for the **built-ins** half (23,668 tests) — schedule
it as Phase B, immediately before `Object`/`Array`/`String` stages.
Do not grind those stages in Rust; that work dies under R0.
See `tasks/refactor-plan.md`.

## S2 — Fix by root cause, not by test *(high, every stage)*

One spec-op / storage fix unblocks hundreds of tests. Per stage:

1. `TEST262_DIGEST=1 TEST262_STAGE=N cargo test …` (or `tools/run-each.sh`).
2. Group failures by error / op / missing intrinsic.
3. Write one reproducer `#[test]` for the dominant cluster → fix → re-digest.
4. Append a one-line history entry on `tasks/stage-NN.md`.

Prefer a root-cause that also helps later stages over a narrow patch.
The sequential stage gate still applies; fix *order inside* a stage
follows cluster frequency.

## S3 — OXC early errors via `oxc_semantic` *(high, Phase A)*

A large slice of the 23,711 language tests are static-semantics early
errors. Hand-rolling in `lower/` is thousands of LOC; OXC already
implements them. Route parse → `oxc_semantic` → SyntaxError before
lowering (R17; `DEPENDENCIES.md` row in the same diff).

## S4 — OXC `async-to-generator` transform *(medium-high, Phase C)*

Generators already pass (stage 27 `done`). Async stages
(`async-generator`, `for-await-of` 1,234, `await-using`) plus `Promise`
729 + `AsyncFunction`/`AsyncGenerator*` reduce to generators + a job
queue if the transform runs at lower time. Verify `oxc_transformer`
semantics match ES before committing.

## S5 — Parallel in-stage runner + failure digest tooling *(medium — active)*

Stages stay a sequential gate; tests *within* a stage are independent —
run on all cores. Pair with S2: machine-readable digests
(`tasks/failures-N.json`), failed-only rerun, QUICK triage.

**Landed (2026-07-23):** runner split (`runner/{collect,execute,digest,flags}`),
parallel digest, explicit skips, JSON output, `TEST262_FAILED_JSON`,
prebuilt `run-test` isolation, shared `run_single_test` path.
Detail: `tasks/harness-roadmap.md`.

**Next:** cluster filter, digest diff script, progress fields in
`index.json`, zero crash-file skips.

## S6 — Disciplined unit tests *(ongoing practice)*

Per `AGENTS.md`: reproducers, core invariants, refactor pins only.
test262 is the spec-behavior suite; duplicating it is waste.

## S7 — Crate-first for every remaining primitive *(ongoing)*

Policy (`DEPENDENCIES.md`): regress, chrono, num-bigint, serde_json,
urlencoding, oxc. **Long pole: `Temporal` (4,603)** — staged last.
Evaluate `temporal_rs` before hand-rolling anything.

## Rejected / low value

- *Full R0 before finishing language stages* — delays the current gate;
  schedule as Phase B.
- *R15 file-split sweeps ahead of failing clusters* — lint gate stays
  enforced on touched code; wholesale splits unlock ~0 tests.
- *Parallel stage execution* — hides root causes; use in-stage
  parallelism (S5).
- *Coverage-driven unit tests* — see S6.
- *Incremental-compile tuning / generic profiling* — only if the test
  loop itself is the bottleneck.
- *Per-stage checkpoint/skip lists* — "no checkpoints, no skips"; a
  skip list is a lie that compounds.
- *Grinding Object/Array/String in Rust* — deleted by R0.

## CI regression gate

Run `ALL_STAGES=1` on every merge to `main`; a previously-done stage
regressing blocks the merge.
