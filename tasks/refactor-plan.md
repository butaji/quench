# Refactor Plan

Goal: 100% of test262, staged, **as soon as possible**, with **minimum
LOC**. Architecture is a small Rust core + self-hosted JS builtins
(see `docs/architecture.md`). Execution order is decided by
`tasks/10-ways-to-speed-up.md` (Phases A → B → C) — this file is the
work queue behind that path.

Everything below follows the `AGENTS.md` failing-test-first cycle and
the linter gate (`-D warnings`; files ≤ 500 lines, functions ≤ 40
lines, complexity ≤ 10, ≤ 3 bool params, no `#[allow]` and no
deferrals). Lint limits apply to every touched file; do not queue
repo-wide split sweeps ahead of failing test262 clusters.

## Status (2026-07-24)

| Metric | Value |
|--------|-------|
| Production Rust LOC | ~68k `src/` total: ~46k core+eval+value, ~16k builtins, ~6k test262 harness (tests excluded; measured 2026-07-24) |
| JS builtins | **0** — R0 not started |
| `%ops%` / `eval/ops.rs` | **scaffold** — re-exports + thin `%ops%` wrapper; not yet the single owner |
| Target (realistic) | **~20–28k Rust** + **~8–12k JS** for 95%+ |
| Target (aspirational) | **~8–12k Rust** + **~19k JS** (100%) |
| Benchmarks | Boa ~25k Rust → 94%; Kiesel ~50k Zig → 94%; QuickJS ~80k C → 83% |
| Current stage | 25 `for-of` (751 tests; 698 pass / 53 fail per `tasks/failures-25.json`) · full digest 27,323/42,892 = 63.7% (2026-07-23) |
| Crate candidates | `DEPENDENCIES.md` — verified 2026-07-24; new: `bumpalo`, `string_interner`, `oxc_semantic`, `regex` (for Unicode) |

**Build gate (2026-07-24):** `cargo test -p quench-runtime` currently
fails to compile — `tests/for_of_yield_repro.rs` is missing
`use quench_runtime::test262::Test262Host;` (E0599 ×4) and
`src/lower/expr/helpers_expr.rs:133` emits warnings under `-D warnings`.
Fixing this precedes any stage work; the linter gate is meaningless on a
tree that does not build.

File:line references in this plan and in `tasks/review-2026-07-19*.md`
are snapshots; re-locate by symbol name before editing. Object-model
audit: `tasks/review-2026-07-22-object-model.md`. Crate candidates:
`DEPENDENCIES.md`.

## Critical path (ASAP × min LOC)

```
Phase A — language (now)
  R4 ✓ → R5 ✓ → stages 16–24 ✓ → stage-25 S2 digest → R17 →
  remaining language stages
  S5 harness active · R1 grows only for ops touched by fixes

Phase B — immediately before built-ins stages
  Finish R1 (ops own impls) → R0 (Object first) → R2

Phase C — built-ins / async / Temporal
  Built-ins in JS · S4 async→generator · Temporal last (crate)
```

Priority legend used below:

- **NOW** — unblocks stage 25 / language
- **PHASE-B** — required before grinding Object/Array/…
- **LATER** — hygiene, LOC, or stage-specific; never ahead of NOW

---

## R4 — Delete speculative `TComp` infra  *(DONE 2026-07-23, diff=1)*

Re-audit 2026-07-22 (`tasks/review-2026-07-22-object-model.md`): the
layer lived in `value/object/vtable.rs` (274 LOC),
`value/object/array.rs` (91), `Key`/`Desc`/`VTable`/`Slots`/`ThisMode`
in `value/object/helpers.rs` (~80), plus `props`/`slots`/`vtable` on
`Object`. Grep-verified: zero callers outside `src/value/object/` —
`.vtable` written 3×, read 0×; `.props` write-only; `slots` never read.
Dead copy disagrees with live store on attribute defaults.

- [x] `#[test]`: array assign + defineOwnProperty survives (refactor pin).
- [x] Delete the lot, including re-exports and `props` sync writes in
      `new_array`.

~470 LOC saved. Commit `9822e375`.

## R5 — Collapse `Object` property storage + fix spec semantics  *(partial DONE 2026-07-23, diff=7)*

Highest language-stage lever. Parallel maps in
`value/object/helpers.rs` plus hand-rolled walk in
`eval/member/object_member.rs`. Spec bugs (each needs a failing
reproducer `#[test]` first):

- Attribute defaults inverted (`define_own_property` → `true`; spec `false`).
- Strict writes swallowed (`Object::set` no-ops; must TypeError in strict).
- No ValidateAndApplyPropertyDescriptor.
- `Symbol` has no identity id; `symbol_properties` keyed by desc
  (AGENTS.md: `desc\0id`).
- Key ordering: `"length"` excluded; holes listed; symbols absent from
  `own_keys`.
- Seal/freeze uncomputable; `get_own_property` lies about elements;
  `to_object("ab")` wrong.

- [x] `#[test]`: two `Symbol("x")` keys on one object don't collide.
- [x] `#[test]`: `Object.keys({length:1})` → `["length"]`; symbols in
      `ownPropertyKeys` after string keys; holes skipped.
- [x] `#[test]`: strict write to non-writable throws TypeError;
      `Object.defineProperty(o,"x",{value:1})` yields
      non-writable/non-enumerable/non-configurable.
- [x] Give `Symbol` a unique id (`desc\0id`); key by identity.
- [ ] Collapse to `own_props: IndexMap<Key, Prop>` where
      `Prop = Value | Accessor{get,set}` + `PropertyAttributes`;
      `Key::Sym(Rc<Symbol>)`. Array as `Vec<Option<Value>>` with
      `Value::Hole` for holes. One descriptor type, one accessor type.
- [ ] Route eval member access through the collapsed store; delete the
      hand-rolled walk in `object_member.rs`.

Spec-bug fixes landed (commit `28bc28b7`); full IndexMap collapse deferred.
Do **not** wait for R0 — language stages need this now.

## R17 — OXC early errors via `oxc_semantic`  *(NOW / Phase A, diff=4)*

High tests-per-LOC for the language half. Hand-rolling early errors in
`lower/` is thousands of LOC.

**Decision (2026-07-24): adopt `oxc_semantic = "0.47"`.** The `oxc`
0.47 umbrella crate does **not** include semantic (Cargo.lock shows only
`oxc_allocator`, `oxc_ast`, `oxc_diagnostics`, `oxc_parser`,
`oxc_regular_expression`, `oxc_span`, `oxc_syntax`), so this is a new
dependency with a `DEPENDENCIES.md` row. `SemanticBuilder::build(&program)`
runs on the oxc AST **before lowering** — it fits the internal-AST
walker unchanged (`docs/architecture.md` §Execution model).

The checker does not cover every ECMAScript early error, so landing is
two-step: first wire it in and count newly-caught failures on the
current digest; delete a hand-rolled check only when oxc_semantic
demonstrably fires for that case.

- [ ] Add `oxc_semantic = "0.47"` + `DEPENDENCIES.md` row.
- [ ] `#[test]`: duplicate `let` in one block → catchable `SyntaxError`.
- [ ] Wire parse → semantic check → SyntaxError before lowering; report
      the newly-caught failure count on the stage-25 digest.
- [ ] Delete redundant hand-rolled checks only where coverage is proven
      by the step above.

## R1 — `eval/ops.rs` + `%ops%` bridge  *(incremental NOW; finish PHASE-B, diff=3)*

**Status:** `src/eval/ops.rs` and `builtins/core/ops_wrapper.rs` exist
as a scaffold (re-exports + frozen `%ops%` with only `toPrimitive`,
`toNumber`, `toPropertyKey`). Not yet the single owner — private copies
remain in `builtins/*.rs` and `eval/`.

**Known defect (audit 2026-07-24):** the object is registered as a
global literally named `%ops%`, but `%` is not a valid JS identifier
character — JS can only reach it as `globalThis["%ops%"]`, and any test
or builtin evaluating `"%ops%.foo(...)"` as source should fail oxc
parsing. Until parse-time resolution lands, reference it via
`globalThis["%ops%"]` in JS/tests, or rename the binding to a valid
intrinsic name (e.g. `$ops`). Verify the `ops_wrapper` eval-based tests
when the build gate is fixed; if red, switch them to the string-key
form in the same diff.

- [ ] Own the implementations in `eval/ops.rs` (or thin wrappers that
      are the only call path): `to_primitive`, `to_property_key`,
      `to_object`, `to_number`, `to_string`, `same_value`,
      `same_value_zero`, `is_callable`, `is_constructor`,
      `ordinary_has_property`, `create_data_property_or_throw`,
      `get_iterator`, `iterator_next`, `iterator_step`,
      `iterator_close`, `create_iter_result_object`, `native_fn`,
      `throw_type_error`.
- [ ] One `#[test]` per op when it becomes owned here.
- [ ] `%ops%` stays frozen; parser resolves `%ops%` at parse time
      (never user-visible). When this lands, sync the AGENTS.md
      `%ops%` convention line with the implemented mechanism.
- [ ] On touch: replace the local duplicate; do not leave two owners.
- [ ] **Phase B gate:** before R0 / Object stage, zero private copies of
      the ops list above remain outside `eval/ops.rs`.

## R0 — Self-host builtins in JS  *(PHASE-B — before built-ins stages, diff=5)*

Move every pure-spec builtin from `builtins/*.rs` to `builtins/*.js`.
Do **not** start a full migration during stage 16; it does not unblock
`class`. Start when Phase A language stages are clear (or when the next
failing stage is a built-in you would otherwise enlarge in Rust).

- [ ] `builtins/*.js` tree, `include_str!`-embedded.
- [ ] `builtins/bootstrap.rs`: parse + eval each file in dependency order.
- [ ] Per builtin: failing `#[test]` → JS shell → delete the Rust
      `register_*`. Full `cargo test -p quench-runtime` green before next.
- [ ] Order: `Object` → `Function` → `Error` → `Symbol` → `Number` →
      `Boolean` → `String` → `Array` → `Iterator` → `Map`/`Set`/`Weak*`
      → `Promise` → `JSON` → `Reflect`/`Proxy` → `Math` → `RegExp`
      (shell over `core/regex.rs`) → `Date` (shell over `core/date.rs`)
      → `BigInt` (shell over `core/bigint.rs`) → `TypedArray`/
      `ArrayBuffer`/`DataView`/`Atomics` → URI.

Unblocks R2 / R7 / R8 / R13 cleanup. Never grind `Object`/`Array`/
`String` stages by growing Rust builtins first.

## R2 — One iterator protocol  *(PHASE-B, with R0 Iterator.js, diff=3)*

Four impls today: `eval/iteration.rs` (eager `Vec<Value>`, breaks
generators), `builtins/weak.rs` `for_each_on_iterable`,
`builtins/map.rs` `make_iterator`, `eval/object` `obtain_iterator`.

- [ ] R1 owns `get_iterator`/`iterator_next`/`iterator_step`/
      `iterator_close`. R0 builds `%IteratorPrototype%` once in JS;
      Array/String/RegExp/Map/Set iterators inherit via prototype chain.
- [ ] Delete all four Rust duplicates.

~400 LOC saved. If `for-of` / destructuring fails earlier on the eager
materializer, land the streaming `ops` path (and delete that one
duplicate) in Phase A without waiting for full R0.

## R3 — `chrono`-backed Date core  *(PHASE-B / with Date.js, diff=2)*

`builtins/date.rs` hand-rolls leap-year math under `chrono_*` names but
never imports `chrono` (confirmed via grep: zero `use chrono` hits). R3
implements the fix documented in `DEPENDENCIES.md`.

- [ ] `builtins/core/date.rs`: `UtcTimestamp`, `YmdToMs`, `MsToYmd` over
      `chrono::NaiveDate` + `chrono::Utc`.
- [ ] `builtins/Date.js` thin shell.
- [ ] `#[test]` for `Date.UTC` covering leap years + pre-1970.
- [ ] `DEPENDENCIES.md` row for the upgrade (if any).

~50 LOC saved.

## R6 — `Realm` owns intrinsic prototypes; `%ThrowTypeError%`  *(LATER / stage-gated, diff=5)*

`Context::reset` clears only 2 of ~14 thread_local proto pointers.
`%ThrowTypeError%` missing (skip-listed in runner).

- [ ] `#[test]`: after `reset`, a native getter resolves against new realm.
- [ ] `Realm` owns intrinsic prototypes; `Context::new` clones from a
      `Realm` template; bootstrap once per `Realm`.
- [ ] `reset` clears all proto pointers consistently (ideally zero — they
      live on `Realm`).
- [ ] `%ThrowTypeError%` once per `Realm` with stable identity.

Do when the `ThrowTypeError` stage (or a digest cluster) demands it.

## R7 — One `to_object`  *(absorbed by R1, diff=1)*

Three divergent boxers (one boxes `undefined`/`null`). Delete on touch
as R1 owns `to_object`.

## R8 — `panic!` → `throw_type_error`  *(LATER; most vanish under R0, diff=2)*

- [ ] `value::error::throw_type_error(msg) -> JsError`.
- [ ] `#[test]` per panic site that must remain in Rust (`core/`).
- [ ] Replace panics + `JsError::from("TypeError:…")` string throws.

Prefer fixing a panic when a stage digest hits it; otherwise sweep with
R0.

## R9 — Dead code sweep  *(LATER, after R4 / with R0, diff=2)*

After R4/R1/R0 reduce the surface: dead convert helpers, unused
`Getter`/`Setter*` types, `ObjData` variants never constructed,
`intl.rs` (out of scope — delete), one-line wrappers, etc.

Repo debris spotted in the 2026-07-24 audit (delete in the sweep):
`tasks/repro_overflow.rs`, `tasks/run_all_stage16.sh` (stage-16-era
leftovers), `tools/debug-test.js` (untracked debug script — AGENTS.md:
no debug code), `src/builtins/test_marker` (a 4-byte file containing
"test", referenced nowhere), and the unwired `patches/oxc_parser` copy
(R23).

Also measured 2026-07-24:

- **35 `#[allow(dead_code)]` markers in `src/`** — each is a
  `TODO(delete)` per AGENTS.md. R15's final sweep zeroes them; 35 is
  the baseline.
- **`console.rs` (87 LOC)** — `console` is not ECMA-262; test262 never
  tests it. Keep only as a host API behind a feature, else delete.
- **Native assert duplication (~2.1k LOC)** — `assert_helpers.rs`
  (1,155) + `property_helpers.rs` (947) reimplement test262's own JS.
  Shrinks as the official files load verbatim — tracked with removal
  criteria in `tasks/harness-roadmap.md` §Harness fidelity.

~620 LOC saved (original estimate; debris adds more). Opportunistic on
touch; no dedicated queue jump.

## R10 — RAII `CURRENT_CONTEXT`; collapse thread-locals  *(LATER, diff=4)*

Open-coded save/restore skips restore on some `Err` paths.

- [ ] `CtxGuard` + `Drop`; `RefCell` peek instead of take+set.
- [ ] Pairs with R6.

## R11 — `Context::call_js_function` → `eval::function::call_value`  *(LATER, diff=1)*

~55 LOC. Delete when touching call paths.

## R12 — Split `eval/object.rs`  *(DONE, diff=1)*

Remaining over-500 offenders tracked in R15.

## R13 — `object_static.rs` cleanup  *(absorbed by R0 + R5, diff=1)*

Including `FROZEN_OBJECTS` → see R16.

## R14 — `lower_expr` fail-loud on unknown  *(LATER, diff=1)*

Catch-all → `Err` so new OXC variants surface at lower time.

## R15 — Linter-gate sweep  *(continuous on touch; final sweep LATER, diff=2)*

**Not a test262 unlock.** Enforced on every PR for files you edit.
Wholesale split of untouched >500-line files waits until after R0/R5
shrink the surface — do not prioritize ahead of Phase A/B.

- [ ] On touch: file ≤ 500, fn ≤ 40, complexity ≤ 10, no new `#[allow]`.
- [ ] Hoist the 16 duplicate `fn eval(src: &str)` test helpers (grep
      count 2026-07-24) into one shared `#[cfg(test)]` util — zero-
      duplication rule applies to test code too.
- [ ] Final sweep: `rg '#\[allow\(' crates/quench-runtime/src` zero hits
      (baseline: 35 `#[allow(dead_code)]`, 2026-07-24); no production
      file > 500 lines; clippy clean.

## R16 — Drop `FROZEN_OBJECTS` thread_local  *(LATER / with R5 freeze path, diff=2)*

Use `Object.extensible` (and proper descriptors from R5); delete
`FROZEN_OBJECTS` + `is_frozen_object`. Details: T14 in
`tasks/review-2026-07-19.md`.

---

## R18 — RegExp Unicode property escapes  *(LATER / stage 84, diff=2)*

`regress` (ES2018 + `v` flag, confirmed in `DEPENDENCIES.md`) does NOT
support Unicode property escapes `\p{}`. Stage 84 tests `\p{Script}`,
`\p{Emoji}`, `\p{General_Category}`, etc.

**Decision (2026-07-24): two engines, one dispatch.** Keep `regress` as
the RegExp engine (it alone covers ES backreferences and lookbehind —
the `regex` crate has neither, so it can never replace regress). Add
`regex` with `unicode-perl` as the secondary engine used **only** when
the pattern contains `\p{`/`\P{` and no lookaround/backreferences —
exactly the slice `regex` covers. Patterns needing both `\p{}` and
backrefs are the accepted residual gap until regress grows `\p{}`
upstream. `fancy-regex` stays rejected (no `v`-flag Unicode sets).

- [ ] `regex = { version = "1", features = ["unicode-perl"] }` +
      `DEPENDENCIES.md` row.
- [ ] Dispatch in `builtins/core/regex.rs` on pattern scan; `#[test]`
      per branch.
- [ ] `#[test]`: `\p{Emoji}` matching, `\p{Script=Latin}`,
      `\p{General_Category=Number}`.

## R19 — `bumpalo` arena allocation  *(LATER / Phase B, diff=3)*

**Decision: `bumpalo = "3"`, not `bump_scope`.** `bump_scope` benches
~2x faster but is far less proven; `bumpalo` (244M+ downloads) is the
battle-tested choice (Boa, many WASM engines). Optimize later only if
profiling (R22) shows the arena itself hot.

Key constraint: **no `Drop` on freed objects.** `bumpalo::boxed::Box`
runs Drop on scope exit for types that need it; standard heap allocation
is acceptable for those. Most JS value types have no Drop impl.

Usage in Quench:
- Parsing: `Arena` lives for parse phase; all `NodeId`s / AST nodes freed
  in one shot when the arena drops.
- Eval frames: `Bump` in `Context`; eval loop allocates Value slots from
  it; each top-level eval call resets with `Bump::new`.
- NaN-boxed `JsValue` (R20): arena allocation pairs well — fewer heap
  objects means less GC pressure.

- [ ] `bumpalo = "3"` in `Cargo.toml` + `DEPENDENCIES.md` row.
- [ ] `#[test]`: no Drop impls on freed arena objects.
- [ ] Migration order: eval frames first, then parser, then Value constructors.

## R20 — NaN-boxed `JsValue`  *(LATER / Phase B, diff=3)*

Rust's `enum JsValue` with inline/`Box`/`Rc` variants costs 2 words per
Value plus heap traffic for every object. NaN boxing stores everything in
a single `u64` — integers in the top 33 bits, pointers in the low 49
bits of a quiet NaN, with tag bits distinguishing the slot type.

**Confirmed (2026-07-23):**
- Boa v0.21 switched from enum to NaN-boxed `JsValue` (October 2025).
- Boa v0.21 achieves 94.12% test262 conformance.
- SpiderMonkey and JSC use NaN boxing; V8 uses tagged pointers.
- No dedicated Rust crate — implement with `unsafe` in `value/` module.
- Quiet NaN (qNaN) only — signaling NaN never appears in IEEE754 JS values.

Bit layout:
```
63       49     48     32     31     0
[unused][tag=0xFFFF][integer payload] — integer slot
63       49     48     32     31     0
[unused][tag=0x0000][pointer payload    ] — pointer slot
```
Tag values: `0xFFFF` → Integer, `0x0000` → Pointer, `0x7FFC` → Double.
Pointer encoding uses 2^49 offset to distinguish from canonical NaN
(`0x7FFC000000000000`).

`JsValue` becomes a newtype `u64` with accessor methods:
`JsValue::new_integer(i32)`, `JsValue::new_object(*mut Object)`,
`JsValue::new_double(f64)`, `JsValue::unbox()`.

Do **not** migrate before R5 (object model correctness) — NaN boxing must
pair with the correct property store, not the current buggy one.

- [ ] `value/value_nan.rs` — `JsValue` newtype + all accessor methods.
- [ ] `#[test]`: integer, object, double round-trips; `undefined`, `null`,
      `true`, `false`, `NaN`, `Infinity`.
- [ ] `#[test]`: NaN-boxed value survives a bumpalo round-trip.
- [ ] `#[test]`: `Object.is` / `SameValue` on NaN-boxed values.

## R21 — String interning / atom table  *(LATER / Phase B, diff=2)*

JavaScript string comparisons are pervasive: property key lookup, `===`,
`Map`/`Set` hashing. Un-interned strings do O(n) byte-by-byte comparison
on every `==`; an atom table makes pointer comparison O(1).

**Confirmed (2026-07-24):**
- `src/interner.rs` already has a hand-rolled `StringInterner` on
  `Context` — with **zero call sites** outside its own module. R21
  deletes it in favor of the crate (decision below).
- `string_interner = "0.20"` is the atom table (decision 2026-07-24:
  crate over the unused hand-rolled `src/interner.rs`, which R21
  deletes). Hashing: `rustc-hash` (already vendored). Do NOT add `fnv` —
  latest release is 1.0.7 and slower than `rustc-hash`.
- QuickJS uses a global atom table in `JSRuntime`.
- `string_cache` (Servo): unmaintained since ~2020.

Usage:
- Property keys (string + Symbol): interned key lookup for `Object` property
  map; `Key = InternedKey(KeyId)`.
- String literals: intern at parse time; string comparison in eval uses
  `KeyId::eq` (pointer compare).
- String values: `StringId` type wrapping `string_interner::DefaultSymbol`.

- [ ] Adopt `string_interner = "0.20"` and **delete** the unused
      hand-rolled `src/interner.rs` in the same diff (AGENTS.md:
      prefer a crate over hand-rolling). `DEPENDENCIES.md` row.
      Hashing: the already-vendored `rustc-hash`.
- [ ] `#[test]`: interned string pointer equality; `"abc" == "abc"` pointer compare.
- [ ] `#[test]`: `Map` with 10k distinct string keys — baseline benchmark.

## R22 — Profiling tools on macOS  *(LATER / when needed, diff=1)*

When the test262 iteration loop is the bottleneck, profile before tuning:

**cargo-flamegraph** (most common, uses `xctrace` under the hood):
```bash
cargo install cargo-flamegraph
cargo flamegraph --bin run-test -- test262/sample.js
# opens Speedscope / Firefox Profiler format
```

**samply** (Rust-native alternative; Firefox Profiler UI):
```bash
cargo install samply
samply record -- cargo run --bin run-test -- test.js
samply codegen  # generates .profile.json
# open in https://profiler.firefox.com/
```

**xctrace CLI** ( Instruments.app on macOS):
```bash
xcrun xctrace list templates   # list available templates
xcrun xctrace record --template "Time Profiler" --output trace.trace -- cargo run -- test.js
open trace.trace               # open in Instruments.app
```

For flamegraph output: `~/.cargo/bin/cargo-flamegraph` or install via
`cargo install cargo-flamegraph`. `perf` on Linux is equivalent.

## R23 — Delete unwired `patches/oxc_parser`  *(NOW, diff=1)*

`patches/oxc_parser/` is a full vendored copy of `oxc_parser` 0.47.1
(~4.6k LOC) that **nothing references**: no `[patch.crates-io]` in the
workspace `Cargo.toml`, no path dependency, and `Cargo.lock` resolves
`oxc_parser` from crates.io. Dead weight under the dead-code rule —
anyone auditing the tree cannot tell which parser source is real.

- [ ] `git rm -r patches/oxc_parser` (recoverable from git history if a
      parser patch is ever actually needed — at which point it must be
      wired via `[patch.crates-io]` with a `DEPENDENCIES.md` note).

---

## Sequencing (summary)

```
NOW:     fix build gate (for_of_yield_repro import; see Status) →
         stage 25 for-of (S2 digest, 53 fails) → R17 → language stages
         R23 delete patches/oxc_parser (1 command, no risk)
         R1 incremental on every op touch
         S5 harness (parallel digest, failed-only rerun) — active
PHASE-B: R1 complete → R0 → R2 (+ R3 with Date.js) → R18
         R19 (bumpalo) + R20 (NaN boxing) + R21 (interning)
LATER:   R6 R8 R9 R10 R11 R14 R16 as stages/digests demand
         R22 profiling when loop is the bottleneck
         R15 on every touch; repo-wide sweep after R0/R5
```

Every item lands with `cargo test -p quench-runtime` +
`cargo clippy -p quench-runtime --all-targets` clean. test262 stage
gate (`tasks/index.json`) must not regress.
