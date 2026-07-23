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
  A5. S8 url over urlencoding       (URL Standard; before modules)
  A6. Remaining language stages     (for-of, expressions, …)
      — grow R1 only for ops you touch

Phase B — before grinding built-ins (~stage 71 Object)
  B1. Finish R1 (%ops% owns impls, not re-exports)
  B2. R0 self-host builtins in JS   (Object first, then dependency order)
  B3. R2 one iterator protocol      (with R0 Iterator.js)
  B4. R3 chrono for Date core       (builtins/core/date.rs)

Phase C — built-ins → annexB → Temporal
  C1. Built-ins stages in JS        (never re-expand Rust builtins)
  C2. S4 async→generator            (for-await-of / Promise / Async*; verify swc first)
  C3. R18 regex Unicode escapes     (RegExp \p{}; before stage 84)
  C4. Temporal last                 (temporal_rs + ICU4X; stage 120)
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

## S4 — Async-to-generator transform *(medium-high, Phase C)*

Generators already pass (stage 27 `done`). Async stages
(`async-generator`, `for-await-of` 1,234, `await-using`) plus `Promise`
729 + `AsyncFunction`/`AsyncGenerator*` reduce to generators + a job
queue if the transform runs at lower time.

**Confirmed (2026-07-23):** `oxc_transformer` does NOT have an
async-to-generator transform — it handles TypeScript stripping, JSX,
React, decorators, and ES2026→ES2015 lowering only.
`swc_ecma_compat_es2017::async_to_generator` (docs.rs confirmed) has the
transform. The risk is adding the full `swc_ecma_*` dependency stack (~10+
crates) alongside existing `oxc` (0.47), creating a second heavyweight
parser dependency.

Verification steps:
1. Does `swc_ecma_compat_es2017` work standalone (without the full swc
   parser), or does it require `swc_ecma_parser` as a peer?
2. Does it conflict with the current `oxc` version in `Cargo.toml`?
3. Run a subset of `for-await-of` (stage 40) tests against the transform
   output before committing.

If swc conflicts: fall back to hand-rolling async eval nodes in `eval/`
(~500 LOC, low risk). Boa (the reference Rust JS engine) uses a hand-rolled
async executor and achieves 94.12% test262 without a swc dependency.

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

`temporal_rs` confirmed production-grade (2026-07-23 research):
- Used by **Boa** (94.12% test262), **Kiesel**, and **V8/Chrome 144**
- 8 types: `ZonedDateTime`, `Instant`, `PlainDateTime`, `PlainDate`,
  `PlainTime`, `Duration`, `Calendar`, `TimeZone`
- ICU4X for calendar/timezone math; Diplomat for C++/Rust FFI
- ES Stage 4 (2026-03-11); spec stable
- `DEPENDENCIES.md` row in the same diff as Temporal stage start.
- Evaluate API surface vs. ES spec version before committing.

## S8 — `url` over `urlencoding` *(low effort, early Phase A/B)*

`urlencoding` only does `%`-encoding. ES modules need full URL resolution:
scheme parsing, path normalization, `data:` URLs, bare specifier resolution.
`url` (rust-url) implements the URL Standard and covers all of these.
Small diff: replace `urlencoding` dep with `url`; update `builtins/core/uri.rs`.
Do before stage 53 (`modules`).

## Stage difficulty matrix

Based on research across Boa (94.12%), Kiesel (94.2%), QuickJS (83%), and
Quench's own codebase (2026-07-23). Difficulty = spec complexity × test
count × external dependencies. `impl` = primary implementation language.

### Hardest (difficulty 7–9, ~10 stages, ~19k tests)

| Id | Path | Tests | Diff | Impl | Key challenge | Crate / approach |
|---|---|---|---|---|---|---|
| 120 | `built-ins/Temporal` | 4,603 | 9 | rust | Full Temporal API | `temporal_rs` + `zoneinfo_rs` |
| 118 | `built-ins/ShadowRealm` | 64 | 9 | rust | Isolated global per spec | Rust-level; not WASM |
| 115 | `built-ins/Proxy` | 311 | 8 | hybrid | 11 internal ops + invariants | Rust traps + JS invariants |
| 44 | `language/expressions` | 11,101 | 8 | rust | Many eval nodes, early errors | `oxc_semantic` |
| 16 | `statements/class` | 4,367 | 7 | rust | Object model, super, private | R5 object model fix |
| 84 | `built-ins/RegExp` | 1,879 | 7 | hybrid | Unicode property escapes `\p{}` | `regex` + `unicode-perl` |
| 38 | `statements/async-generator` | 301 | 7 | rust | Async→generator transform | S4: `swc_ecma_compat` or hand-rolled |
| 97 | `AsyncGeneratorFunction` | 23 | 7 | rust | Same as above | S4 |
| 98 | `AsyncGeneratorPrototype` | 48 | 7 | rust | Same as above | S4 |
| 40 | `statements/for-await-of` | 1,234 | 7 | rust | Async iteration + await + R2 | S4 + R2 iterator protocol |

### Medium-hard (difficulty 5–6, ~15 stages, ~12k tests)

| Id | Path | Tests | Diff | Impl | Key challenge |
|---|---|---|---|---|---|
| 71 | `built-ins/Object` | 3,411 | 6 | js | R0 self-hosting |
| 85 | `built-ins/Array` | 3,081 | 6 | js | R0 self-hosting |
| 82 | `built-ins/String` | 1,223 | 6 | js | R0 self-hosting |
| 102 | `TypedArray` | 1,446 | 6 | hybrid | Rust buffers + JS |
| 103 | `TypedArrayConstructors` | 738 | 6 | hybrid | Rust buffers + JS |
| 113 | `Promise` | 729 | 6 | hybrid | Rust job queue + JS |
| 39 | `await-using` | 94 | 6 | hybrid | Using + async |
| 41 | `using` | 78 | 6 | js | Disposable resource protocol |
| 72 | `Function` | 509 | 5 | js | R0 |
| 80 | `Math` | 327 | 5 | js | R0 |
| 53 | `module-code` | 599 | 5 | hybrid | `url` crate + JS |
| 83 | `Symbol` | 98 | 5 | js | R0 |
| 99 | `AsyncFunction` | 18 | 5 | rust | S4 async→generator |
| 100-101 | `ArrayBuffer`, `SharedArrayBuffer` | 325 | 5 | hybrid | Rust buffers |
| 105 | `DataView` | 561 | 5 | hybrid | Rust buffers + JS |
| 111 | `WeakRef` | 29 | 6 | hybrid | GC interaction |
| 112 | `FinalizationRegistry` | 47 | 5 | js | R0 |

### Easy (difficulty 1–4, ~68 stages, ~17k tests)

Most language stages (break, const, continue, debugger, do-while, for,
for-in, if, let, return, switch, try, variable, while, with, etc.) are
difficulty 1–3 and primarily Rust eval nodes. Builtins like `Number`,
`Error`, `Boolean`, `JSON`, `Map`, `Set`, `Reflect` are difficulty 3–4
and become trivial once R0 self-hosting is in place.

### macOS / Darwin notes

No Darwin-specific code needed for any remaining stage:
- **Atomics** (stage 106): `std::sync::atomic` works on macOS natively.
- **Date/Temporal**: `chrono`/`temporal_rs` handle timezones portably; no
  Darwin APIs needed.
- **SharedArrayBuffer**: requires cross-origin isolation headers; test262
  skips these tests when headers are absent — no OS-level work.
- All file I/O is in the harness (`tools/run-each.sh`), not the runtime.

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
- *oxc_transformer for async* — confirmed (2026-07-23) it has no
  async-to-generator; S4 now targets `swc_ecma_compat_es2017` with
  validation gates.
- *tokio for async* — overkill for a microtask queue; smol or
  hand-rolled is correct scope.
- *fancy-regex / re2 for RegExp* — too limited for ES2024 `\p{}`
  escapes; see R18 and `DEPENDENCIES.md`.
- *wasmtime for ShadowRealm* — ShadowRealm is a JS-level isolated
  global, not a WASM sandbox.

## CI regression gate

Run `ALL_STAGES=1` on every merge to `main`; a previously-done stage
regressing blocks the merge.
