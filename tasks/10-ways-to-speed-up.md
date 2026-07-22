# Strategy — fastest path to 100% test262

Data-driven (per-stage counts live in `tasks/index.json`):

- **48,581 tests** across 122 stages: language 23,711 · built-ins
  23,668 · annexB 1,086 · harness 116.
- Done: 1,487 (3%). In progress: stage 16 `class` — 4,367 (9%).
- Largest single stages: `expressions` 11,101 · `Temporal` 4,603 ·
  `class` 4,367 · `Object` 3,411 · `Array` 3,081 · `RegExp` 1,879 ·
  `TypedArray` 1,446 · `String` 1,223 · `Promise` 729.

Speed = fixes-per-week × tests-unlocked-per-fix. The levers below are
ranked by that metric, not by novelty.

## S1 — Land R1 → R0 before grinding builtins stages *(highest leverage)*

Half the corpus (23,668 tests) is built-ins. Today every builtin fix
costs Rust edit + full recompile; after R0 the same fix is a JS edit
with no recompile, at ~1/3 the LOC. The `%ops%` bridge (R1) plus the
self-hosted builtins pivot (R0) is the single biggest throughput
multiplier. Do not grind `Object`/`Array`/`String` stages in Rust
first — that work gets deleted by R0. See `tasks/refactor-plan.md`.

## S2 — Fix by root cause, not by test *(high)*

One spec-op fix (`ToPropertyKey`, iterator protocol, argument
coercion) unblocks hundreds of tests across many stages at once.
Tactic: run `ALL_STAGES=1` periodically, harvest failures into a
digest grouped by error message / op, and fix clusters in dependency
order. The stage gate still applies — but the fix order inside a stage
should follow root-cause frequency, and a root-cause fix that also
helps future stages is preferred over a narrow one.

## S3 — OXC early errors via `oxc_semantic` / `oxc_diagnostics` *(high)*

A large slice of the 23,711 language tests are static-semantics early
errors (duplicate declarations, invalid assignment targets, `with` in
strict mode, label rules…). Hand-rolling these in `lower/` is
thousands of LOC and endless edge cases; OXC already implements them.
Route parse → `oxc_semantic` → SyntaxError before lowering (R17 in
`tasks/refactor-plan.md`; needs a `DEPENDENCIES.md` row).

## S4 — OXC `async-to-generator` transform *(medium-high)*

Async stages (`async-function`, `async-generator`, `for-await-of`
1,234, `await-using`) plus `Promise` 729 + `AsyncFunction`/
`AsyncGenerator*` built-ins all reduce to generators + a job queue if
the transform runs at lower time. Implement generators once; async
falls out. Verify `oxc_transformer` semantics match ES before
committing — a transform bug miscompiles silently.

## S5 — Parallel in-stage runner + failure digest tooling *(medium)*

Stages stay a sequential gate (policy), but tests *within* a stage are
independent: run them on all cores instead of one-thread-per-test
sequential. Pair with S2's digest: machine-readable failure list per
run (`tasks/failures-*.md` generated, not hand-maintained). Faster
loop + better fix targeting; no policy change.

## S6 — Disciplined unit tests *(ongoing practice, not a work item)*

Per `AGENTS.md`: reproducers, core invariants, refactor pins only.
test262 is the spec-behavior suite; duplicating it is waste.

## S7 — Crate-first for every remaining primitive *(ongoing)*

Already policy (`DEPENDENCIES.md`): regress, chrono, num-bigint,
serde_json, urlencoding, oxc. **Known long pole: `Temporal` (4,603
tests)** — no mature Rust crate exists; it is staged last for a
reason. When its stage approaches, evaluate `temporal_rs` before
hand-rolling anything.

## Rejected / low value

- *Parallel stage execution* — stages are a sequential 100% gate;
  skipping ahead hides root causes. In-stage parallelism (S5) gets the
  same cores.
- *Coverage-driven unit tests* — see S6; waste.
- *Incremental-compile tuning, generic profiling* — cargo defaults are
  fine; profile only if the test loop itself becomes the bottleneck.
- *Per-stage checkpoint/skip lists* — "no checkpoints, no skips" is
  the policy; a skip list is a lie that compounds.

## CI regression gate

Run `ALL_STAGES=1` on every merge to `main`; a previously-done stage
regressing blocks the merge. Cheap insurance once stage count grows.
