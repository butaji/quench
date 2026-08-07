# Strategy — fastest path to 100% test262 (min LOC)

Data-driven (per-stage counts live in `tasks/index.json`):

- **42,892 tests** across 122 stages: language 23,711 · built-ins
  23,668 · annexB 1,086 · harness 116.
- **27,323 passed (63.7%)** as of 2026-07-23 full digest. In progress:
  stage 16 `class` — 4,367.
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

> **LOC estimates:** `~Rust` = incremental Rust LOC to implement; `~JS` = JS builtins LOC after R0. JS builtins are ~1/3 the LOC of equivalent Rust per the architecture doc — high Rust LOC in this table = prime target for R0 self-hosting.

| Id | Path | Tests | Diff | Impl | ~Rust LOC | ~JS LOC | Key challenge | Crate / approach |
|---|---|---|---|---|---|---|---|---|
| 120 | `built-ins/Temporal` | 4,603 | 9 | rust | ~200 | ~1,200 | Full Temporal API | `temporal_rs` + `zoneinfo_rs` |
| 118 | `built-ins/ShadowRealm` | 64 | 9 | rust | ~150 | ~400 | Isolated global per spec | Rust-level; not WASM |
| 115 | `built-ins/Proxy` | 311 | 8 | hybrid | ~600 | ~200 | 11 internal ops + invariants | Rust traps + JS invariants |
| 44 | `language/expressions` | 11,101 | 8 | rust | ~1,500 | 0 | Many eval nodes, early errors | `oxc_semantic` |
| 16 | `statements/class` | 4,367 | 7 | rust | ~800 | 0 | Object model, super, private | R5 ✓ object model fix |
| 84 | `built-ins/RegExp` | 1,879 | 7 | hybrid | ~300 | ~400 | Unicode property escapes `\p{}` | `regex` + `unicode-perl` |
| 38 | `statements/async-generator` | 301 | 7 | rust | ~400 | 0 | Async→generator transform | S4: `swc_ecma_compat` or hand-rolled |
| 97 | `AsyncGeneratorFunction` | 23 | 7 | rust | ~100 | 0 | Same as S4 | S4 |
| 98 | `AsyncGeneratorPrototype` | 48 | 7 | rust | ~100 | 0 | Same as S4 | S4 |
| 40 | `statements/for-await-of` | 1,234 | 7 | rust | ~300 | 0 | Async iteration + await + R2 | S4 + R2 iterator protocol |

> **LOC note:** Built-in stages marked `js` are prime R0 targets — Rust implementation would be ~3x more LOC than the JS version. Target the JS LOC as the cost; Rust LOC = 0 (JS wins).

| Id | Path | Tests | Diff | Impl | ~Rust LOC | ~JS LOC | Key challenge |
|---|---|---|---|---|---|---|---|
| 71 | `built-ins/Object` | 3,411 | 6 | js | 0 | ~900 | R0 self-hosting |
| 85 | `built-ins/Array` | 3,081 | 6 | js | 0 | ~600 | R0 self-hosting |
| 82 | `built-ins/String` | 1,223 | 6 | js | 0 | ~400 | R0 self-hosting |
| 102 | `TypedArray` | 1,446 | 6 | hybrid | ~500 | ~200 | Rust buffers + JS |
| 103 | `TypedArrayConstructors` | 738 | 6 | hybrid | ~200 | ~100 | Rust buffers + JS |
| 113 | `Promise` | 729 | 6 | hybrid | ~400 | ~300 | Rust job queue + JS |
| 39 | `await-using` | 94 | 6 | hybrid | ~100 | ~50 | Using + async |
| 41 | `using` | 78 | 6 | js | 0 | ~150 | Disposable resource protocol |
| 72 | `Function` | 509 | 5 | js | 0 | ~350 | R0 |
| 80 | `Math` | 327 | 5 | js | 0 | ~200 | R0 |
| 53 | `module-code` | 599 | 5 | hybrid | ~100 | ~200 | `url` crate + JS |
| 83 | `Symbol` | 98 | 5 | js | 0 | ~150 | R0 |
| 99 | `AsyncFunction` | 18 | 5 | rust | ~100 | 0 | S4 async→generator |
| 100-101 | `ArrayBuffer`, `SharedArrayBuffer` | 325 | 5 | hybrid | ~300 | ~50 | Rust buffers |
| 105 | `DataView` | 561 | 5 | hybrid | ~200 | ~100 | Rust buffers + JS |
| 111 | `WeakRef` | 29 | 6 | hybrid | ~150 | ~100 | GC interaction |
| 112 | `FinalizationRegistry` | 47 | 5 | js | 0 | ~150 | R0 |

### Easy (difficulty 1–4, ~68 stages, ~17k tests)

> LOC estimates for easy stages vary: language stages (break, const, for-in, etc.) are
> pure Rust eval nodes (~50–300 LOC each); R0-builtins (Error, Number, Boolean, Map,
> Set, JSON, Reflect) are ~100–250 JS LOC each. Most are sub-200 LOC.

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

## S9 — Release `opt-level = 3` *(done 2026-07-23, 1 line)*

`.cargo/config.toml`: `opt-level = "z"` → `opt-level = 3`.

Compared to `"z"`, opt-level 3 allows more inlining and vectorization of
loops, trading ~5–15% binary size for measurable runtime speedup. The
test262 runner is throughput-sensitive; a faster binary unlocks more
iterations per development session.

Confirmed by the [Rust Performance Book](https://nnethercote.github.io/perf-book/build-configuration.html):
"Compared to opt-level = 'z', it allows slightly more inlining and also the
vectorization of loops." Source: `nnethercote.github.io/perf-book`.

## S10 — `bumpalo` arena allocation *(high, Phase B, ~300 LOC)*

Most `JsValue` objects and eval frames are short-lived and freed in LIFO
order. `bumpalo` (244M+ downloads, Rust-native) allocates from a
pre-reserved arena — a single bump pointer with no per-allocation
bookkeeping. `bump_scope` (crate, ~2x faster in micro-benchmarks) uses
scope-based lifetime for arena resets; both are viable.

Key constraint: **no `Drop` on freed objects.** Types that need finalizers
(e.g. closing file handles) must use `bumpalo::boxed::Box` which runs Drop
on scope exit; heap allocation is acceptable for those. Most JS value
types (`Object`, `Array`, `String`) have no Drop impl.

Usage in Quench:
- Parsing: `Arena` lives for the parse phase; all `NodeId`s / AST nodes
  allocated from it; freed in one shot when the arena drops.
- Eval frames: `Bump` in `Context`; eval loop allocates Value slots from
  it; each top-level eval call resets with `Bump::new`.
- NaN-boxed `JsValue` layout (S11): inline scalar types in 64 bits,
  heap allocations only for objects/strings that overflow the NaN payload.

Boa historically used individual heap allocation; switching to arena
allocation (when paired with S11 NaN boxing) is a measurable memory and
throughput win. Boa v0.21 benchmarks on microbenchmarks show significant
improvement with NaN boxing alone; arena allocation compounds this.

- [ ] `bumpalo = "3"` or `bump_scope` in `Cargo.toml`.
- [ ] `DEPENDENCIES.md` row.
- [ ] `#[test]`: no Drop impls on freed arena objects.
- [ ] Migration: eval frames first, then parser, then Value constructors.

## S11 — NaN-boxed `JsValue` *(high, Phase B, ~600 LOC)*

JS values are either 31-bit integers, quiet NaN payloads (strings,
symbols, objects, undefined, null, booleans), or 64-bit IEEE754 numbers.
Rust's `enum JsValue` with inline/`Box`/Rc variants costs 2 words per
Value + heap traffic for every object. NaN boxing stores everything in a
single `u64`: integers in the top 33 bits, pointers in the low 49 bits
of a quiet NaN, with a tag field to distinguish pointer-slot from
integer-slot from raw number.

**Confirmed (2026-07-23):**
- Boa v0.21 switched from enum to NaN-boxed `JsValue` (October 2025).
- Boa v0.21 achieves 94.12% test262 conformance.
- SpiderMonkey and JSC use NaN boxing; V8 uses tagged pointers.
- No dedicated Rust crate — implement with `unsafe` in `value/` module.
- Quiet NaN (qNaN) only — signaling NaN never appears in IEEE754 JS
  values.

Bit layout (leaves room for 2^16 object types in the tag space):

```
63       49     48     32     31     0
[unused][tag ][integer payload    ] — integer slot (tag = 0xFFFF)
63       49     48     32     31     0
[unused][tag =0][pointer payload          ] — pointer slot (tag = 0x0000)
```

Tag values: `0xFFFF` → Integer, `0x0000` → Pointer, `0x7FFC` → Double.
Pointer encoding uses 2^49 offset to distinguish from canonical NaN
(`0x7FFC000000000000`). `JsValue` becomes a newtype `u64` with accessor
methods; `JsValue::new_integer(i32)`, `JsValue::new_object(*mut Object)`,
`JsValue::new_double(f64)`, `JsValue::unbox()`.

Migration order: implement alongside `bumpalo` (S10), as NaN boxing reduces
heap pressure that arena allocation handles. Do **not** migrate before R5
(object model correctness) — NaN boxing must pair with the correct property
store, not the current buggy one.

- [ ] `value/value_nan.rs` — `JsValue` newtype + all accessor methods.
- [ ] `#[test]`: integer round-trip, object round-trip, double round-trip,
      `undefined`, `null`, `true`, `false`, `NaN`, `Infinity`.
- [ ] `#[test]`: NaN-boxed value survives a bumpalo round-trip.
- [ ] `#[test]`: `Object.is` / `SameValue` on NaN-boxed values.

## S12 — String interning / atom table *(medium-high, Phase B, ~400 LOC)*

JavaScript string comparisons are pervasive: property key lookup, `===`
comparison, `Map`/`Set` hashing. Un-interned `String` objects do O(n)
byte-by-byte comparison on every `==`; an atom table deduplicates strings
so pointer comparison (`==`) is O(1).

**Confirmed (2026-07-23):**
- `string_interner` crate: widely used, thread-safe variant available,
  O(1) get/create.
- `fnv = "2"`: Fast HashMap with good distribution for small keys;
  standard `HashMap` is fine for the atom table scale.
- QuickJS uses a global atom table in `JSRuntime` with ~50k atom slots.
- `string_cache` (Servo): unmaintained since ~2020.
- `rustc-hash`: fastest overall but lower-quality hash; fine for atom
  table where attack resistance is not a concern.

Usage:
- Property keys (string and Symbol): interned key lookup for `Object`
  property map; `Key = InternedKey(KeyId)` from `string_interner`.
- String literals: intern at parse time; string comparison in eval uses
  `KeyId::eq` (pointer compare).
- String values: stored as `Rc<str>` or interned StringId; comparison
  via `StringId::eq`.

Implementation: `string_interner::StringInterner` in `Context`; every
`Value::String(s)` wraps a `StringId`. `#[test]`: "abc" == "abc" pointer
compare; `Map` with 10k string keys (performance regression test).

- [ ] `string_interner = "0.18"` + `fnv = "2"` in `Cargo.toml`.
- [ ] `DEPENDENCIES.md` row.
- [ ] `value/string_interner.rs` — `Interner` on `Context`, `StringId`
      type wrapping `string_interner::DefaultSymbol`.
- [ ] `#[test]`: interned string pointer equality.
- [ ] `#[test]`: `Map` with 10k distinct string keys — baseline benchmark.

### Total work remaining (approximate)

| Category | Rust LOC | JS LOC | Notes |
|---|---|---|---|
| R5 object model (partial ✓) | ~300 remaining | — | eval nodes + property store |
| R0 self-hosting (0 started) | 0 | ~8,000 | Object→TypedArray; 3x smaller than Rust equivalent |
| R17 oxc_semantic early errors | ~500 | — | ES2015+ static semantics |
| S4 async→generator | ~400 | — | swc or hand-rolled |
| R2/R3 iterator + Date | ~100 | ~100 | |
| R18 RegExp Unicode | ~300 | ~400 | `regex` crate |
| S10 bumpalo | ~300 | — | arena allocation |
| S11 NaN boxing | ~600 | — | Value newtype |
| S12 string interning | ~400 | — | string_interner |
| Hardest stages (Temporal, Proxy, etc.) | ~1,500 | ~2,000 | Temporal via `temporal_rs` |
| Medium-hard stages | ~2,500 | ~2,500 | TypedArray, Promise, buffers, R0 |
| Easy stages (~68) | ~4,000 | ~4,000 | Language eval nodes + R0 builtins |
| **Total (rounded)** | **~11,000** | **~17,000** | ~28k combined |
| **Target (aspirational, 100%)** | **~8–12k** | **~19k** | Per Boa ~25k Rust → 94% |

Current: ~57k Rust + ~14k builtins Rust = ~71k production Rust. Strategy
brings Rust down to ~20–28k + JS up to ~17k = ~37–45k total, a ~35–50%
reduction, with the heavy spec logic moved to self-hosted JS.

## CI regression gate

Run `ALL_STAGES=1` on every merge to `main`; a previously-done stage
regressing blocks the merge.
