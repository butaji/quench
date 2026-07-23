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

## Status (2026-07-23)

| Metric | Value |
|--------|-------|
| Production Rust LOC | ~57k (`src/`; tests excluded) |
| Builtins Rust LOC | ~14k |
| JS builtins | **0** — R0 not started |
| `%ops%` / `eval/ops.rs` | **scaffold** — re-exports + thin `%ops%` wrapper; not yet the single owner |
| Target after migration | **~8–12k Rust** + **~3–5k JS** |
| Current stage | 16 `class` (4,367 tests) |

File:line references in this plan and in `tasks/review-2026-07-19*.md`
are snapshots; re-locate by symbol name before editing. Object-model
audit: `tasks/review-2026-07-22-object-model.md`.

## Critical path (ASAP × min LOC)

```
Phase A — language (now)
  R4 → R5 → stage-16 S2 digest → R17 → remaining language stages
  R1 grows only for ops touched by those fixes

Phase B — immediately before built-ins stages
  Finish R1 (ops own impls) → R0 (Object first) → R2

Phase C — built-ins / async / Temporal
  Built-ins in JS · S4 async→generator · Temporal last (crate)
```

Priority legend used below:

- **NOW** — unblocks stage 16 / language
- **PHASE-B** — required before grinding Object/Array/…
- **LATER** — hygiene, LOC, or stage-specific; never ahead of NOW

---

## R4 — Delete speculative `TComp` infra  *(NOW, no blockers)*

Re-audit 2026-07-22 (`tasks/review-2026-07-22-object-model.md`): the
layer lives in `value/object/vtable.rs` (274 LOC),
`value/object/array.rs` (91), `Key`/`Desc`/`VTable`/`Slots`/`ThisMode`
in `value/object/helpers.rs` (~80), plus `props`/`slots`/`vtable` on
`Object`. Grep-verified: zero callers outside `src/value/object/` —
`.vtable` written 3×, read 0×; `.props` write-only; `slots` never read.
Dead copy disagrees with live store on attribute defaults.

- [ ] `#[test]`: array assign + defineOwnProperty survives (refactor pin).
- [ ] Delete the lot, including re-exports and `props` sync writes in
      `new_array`.

~470 LOC saved. Do first.

## R5 — Collapse `Object` property storage + fix spec semantics  *(NOW)*

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

- [ ] `#[test]`: two `Symbol("x")` keys on one object don't collide.
- [ ] `#[test]`: `Object.keys({length:1})` → `["length"]`; symbols in
      `ownPropertyKeys` after string keys; holes skipped.
- [ ] `#[test]`: strict write to non-writable throws TypeError;
      `Object.defineProperty(o,"x",{value:1})` yields
      non-writable/non-enumerable/non-configurable.
- [ ] Give `Symbol` a unique id (`desc\0id`); key by identity.
- [ ] Collapse to `own_props: IndexMap<Key, Prop>` where
      `Prop = Value | Accessor{get,set}` + `PropertyAttributes`;
      `Key::Sym(Rc<Symbol>)`. Array as `Vec<Option<Value>>` with
      `Value::Hole` for holes. One descriptor type, one accessor type.
- [ ] Route eval member access through the collapsed store; delete the
      hand-rolled walk in `object_member.rs`.

~170 LOC saved + correctness that unblocks `class` and later `Object`.
Do **not** wait for R0 — language stages need this now. Prefer landing
spec-bug fixes even if the full IndexMap collapse follows in a second
PR, as long as there is one canonical lookup path afterward.

## R17 — OXC early errors via `oxc_semantic`  *(NOW / Phase A)*

High tests-per-LOC for the language half. Hand-rolling early errors in
`lower/` is thousands of LOC.

- [ ] `DEPENDENCIES.md` row for `oxc_semantic` / `oxc_diagnostics` in
      the same diff.
- [ ] `#[test]`: duplicate `let` in one block → catchable `SyntaxError`.
- [ ] Parse → semantic check → SyntaxError before lowering; delete
      redundant hand-rolled checks.

## R1 — `eval/ops.rs` + `%ops%` bridge  *(incremental NOW; finish PHASE-B)*

**Status:** `src/eval/ops.rs` and `builtins/core/ops_wrapper.rs` exist
as a scaffold (re-exports + frozen `%ops%`). Not yet the single owner —
private copies remain in `builtins/*.rs` and `eval/`.

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
      (never user-visible).
- [ ] On touch: replace the local duplicate; do not leave two owners.
- [ ] **Phase B gate:** before R0 / Object stage, zero private copies of
      the ops list above remain outside `eval/ops.rs`.

## R0 — Self-host builtins in JS  *(PHASE-B — before built-ins stages)*

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

## R2 — One iterator protocol  *(PHASE-B, with R0 Iterator.js)*

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

## R3 — `chrono`-backed Date core  *(PHASE-B / with Date.js)*

`builtins/date.rs` hand-rolls leap-year math under `chrono_*` names but
never imports `chrono`.

- [ ] `builtins/core/date.rs`: `UtcTimestamp`, `YmdToMs`, `MsToYmd` over
      `chrono::NaiveDate` + `chrono::Utc`.
- [ ] `builtins/Date.js` thin shell.
- [ ] `#[test]` for `Date.UTC` covering leap years + pre-1970.

~50 LOC saved.

## R6 — `Realm` owns intrinsic prototypes; `%ThrowTypeError%`  *(LATER / stage-gated)*

`Context::reset` clears only 2 of ~14 thread_local proto pointers.
`%ThrowTypeError%` missing (skip-listed in runner).

- [ ] `#[test]`: after `reset`, a native getter resolves against new realm.
- [ ] `Realm` owns intrinsic prototypes; `Context::new` clones from a
      `Realm` template; bootstrap once per `Realm`.
- [ ] `reset` clears all proto pointers consistently (ideally zero — they
      live on `Realm`).
- [ ] `%ThrowTypeError%` once per `Realm` with stable identity.

Do when the `ThrowTypeError` stage (or a digest cluster) demands it.

## R7 — One `to_object`  *(absorbed by R1)*

Three divergent boxers (one boxes `undefined`/`null`). Delete on touch
as R1 owns `to_object`.

## R8 — `panic!` → `throw_type_error`  *(LATER; most vanish under R0)*

- [ ] `value::error::throw_type_error(msg) -> JsError`.
- [ ] `#[test]` per panic site that must remain in Rust (`core/`).
- [ ] Replace panics + `JsError::from("TypeError:…")` string throws.

Prefer fixing a panic when a stage digest hits it; otherwise sweep with
R0.

## R9 — Dead code sweep  *(LATER, after R4 / with R0)*

After R4/R1/R0 reduce the surface: dead convert helpers, unused
`Getter`/`Setter*` types, `ObjData` variants never constructed,
`intl.rs` (out of scope — delete), one-line wrappers, etc.

~620 LOC saved. Opportunistic on touch; no dedicated queue jump.

## R10 — RAII `CURRENT_CONTEXT`; collapse thread-locals  *(LATER)*

Open-coded save/restore skips restore on some `Err` paths.

- [ ] `CtxGuard` + `Drop`; `RefCell` peek instead of take+set.
- [ ] Pairs with R6.

## R11 — `Context::call_js_function` → `eval::function::call_value`  *(LATER)*

~55 LOC. Delete when touching call paths.

## R12 — Split `eval/object.rs`  *(DONE)*

Remaining over-500 offenders tracked in R15.

## R13 — `object_static.rs` cleanup  *(absorbed by R0 + R5)*

Including `FROZEN_OBJECTS` → see R16.

## R14 — `lower_expr` fail-loud on unknown  *(LATER)*

Catch-all → `Err` so new OXC variants surface at lower time.

## R15 — Linter-gate sweep  *(continuous on touch; final sweep LATER)*

**Not a test262 unlock.** Enforced on every PR for files you edit.
Wholesale split of untouched >500-line files waits until after R0/R5
shrink the surface — do not prioritize ahead of Phase A/B.

- [ ] On touch: file ≤ 500, fn ≤ 40, complexity ≤ 10, no new `#[allow]`.
- [ ] Final sweep: `rg '#\[allow\(' crates/quench-runtime/src` zero hits;
      no production file > 500 lines; clippy clean.

## R16 — Drop `FROZEN_OBJECTS` thread_local  *(LATER / with R5 freeze path)*

Use `Object.extensible` (and proper descriptors from R5); delete
`FROZEN_OBJECTS` + `is_frozen_object`. Details: T14 in
`tasks/review-2026-07-19.md`.

---

## Sequencing (summary)

```
NOW:     R4 → R5 → stage 16 (S2) → R17 → language stages
         R1 incremental on every op touch
PHASE-B: R1 complete → R0 → R2 (+ R3 with Date.js)
LATER:   R6 R8 R9 R10 R11 R14 R16 as stages/digests demand
         R15 on every touch; repo-wide sweep after R0/R5
```

Every item lands with `cargo test -p quench-runtime` +
`cargo clippy -p quench-runtime --all-targets` clean. test262 stage
gate (`tasks/index.json`) must not regress.
