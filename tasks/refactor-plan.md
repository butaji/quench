# Refactor Plan

Goal: 100% of test262, staged, minimum LOC. Architecture is a small
Rust core + self-hosted JS builtins (see `docs/architecture.md`).
Everything below follows the `AGENTS.md` failing-test-first cycle and
the linter gate (`-D warnings`; files ≤ 500 lines, functions ≤ 40
lines, complexity ≤ 10, ≤ 3 bool params, no `#[allow]` and no
deferrals).

Audit baseline (2026-07-22): ~42k production Rust LOC (tests excluded),
0 JS builtins — R0/R1 not started. Target after migration: **~8–12k
Rust** + **~3–5k JS**. File:line references in this plan and in
`tasks/review-2026-07-19*.md` are snapshots from 2026-07-19; re-locate
by symbol name before editing. Object-model audit (2026-07-22):
`tasks/review-2026-07-22-object-model.md` — R4/R5 updated from it.

## R0 — Self-host builtins in JS  *(highest, unblocks R4/R7/R8/R9/R13)*

Move every pure-spec builtin from `builtins/*.rs` to `builtins/*.js`.

- [ ] `builtins/*.js` tree, `include_str!`-embedded in `builtins/mod.rs`.
- [ ] `builtins/bootstrap.rs`: parse + eval each file in dependency order.
- [ ] Per builtin: failing `#[test]` → JS shell → delete the Rust `register_*`.
      Full `cargo test -p quench-runtime` green before next.
- [ ] Order: `Object` → `Function` → `Error` → `Symbol` → `Number` →
      `Boolean` → `String` → `Array` → `Iterator` → `Map`/`Set`/`Weak*`
      → `Promise` → `JSON` → `Reflect`/`Proxy` → `Math` → `RegExp`
      (shell over `core/regex.rs`) → `Date` (shell over `core/date.rs`)
      → `BigInt` (shell over `core/bigint.rs`) → `TypedArray`/
      `ArrayBuffer`/`DataView`/`Atomics` → URI.

## R1 — `eval/ops.rs` + `%ops%` bridge  *(blocker for R0)*

`eval/ops.rs` doesn't exist today; AGENTS.md names it canonical but
never enforced it. R0 cannot land without it.

- [ ] Create `src/eval/ops.rs`: `to_primitive`, `to_property_key`,
      `to_object`, `to_number`, `to_string`, `same_value`,
      `same_value_zero`, `is_callable`, `is_constructor`,
      `ordinary_has_property`, `create_data_property_or_throw`,
      `get_iterator`, `iterator_next`, `iterator_step`,
      `iterator_close`, `create_iter_result_object`, `native_fn`,
      `throw_type_error`.
- [ ] One `#[test]` per op (mirror `src/eval/string_methods.rs`).
- [ ] Expose frozen `%ops%` on the realm; each op a `NativeFunction`;
      parser resolves `%ops%` import at parse time (never user-visible).
- [ ] Replace every duplicated private copy in `builtins/*.rs` and `eval/`
      with calls into `eval/ops.rs`.

## R2 — One iterator protocol  *(HIGH)*

Four impls today: `eval/iteration.rs:16-58` (eager `Vec<Value>`, breaks
generators), `builtins/weak.rs:254-410` `for_each_on_iterable` (only
spec-shaped), `builtins/map.rs:212-247` `make_iterator` (never linked
to `%IteratorPrototype%`), `eval/object.rs:108-273` `obtain_iterator`
(destructuring only, re-evaluates getter each loop).

- [ ] R1 provides `get_iterator`/`iterator_next`/`iterator_step`/
      `iterator_close`. R0 builds `%IteratorPrototype%` once in JS;
      Array/String/RegExp/Map/Set iterators inherit via prototype chain.
- [ ] Delete all four Rust duplicates.

~400 LOC saved.

## R3 — `chrono`-backed Date core  *(HIGH)*

`builtins/date.rs:503-560` hand-rolls leap-year math under `chrono_*`
names but never imports `chrono`. `chrono_now().unwrap()` also panics.

- [ ] `builtins/core/date.rs`: `UtcTimestamp`, `YmdToMs`, `MsToYmd` over
      `chrono::NaiveDate` + `chrono::Utc` (ctx7-confirmed).
- [ ] `builtins/Date.js` becomes a thin shell.
- [ ] `#[test]` for `Date.UTC` covering leap years + pre-1970.

~50 LOC saved.

## R4 — Delete speculative `TComp` infra  *(HIGH, no blockers)*

Re-audit 2026-07-22 (`tasks/review-2026-07-22-object-model.md`): the
layer now lives in `value/object/vtable.rs` (274 LOC),
`value/object/array.rs` (91), `Key`/`Desc`/`VTable`/`Slots`/`ThisMode`
in `value/object/helpers.rs` (~80), plus the `props`/`slots`/`vtable`
fields on `Object` and their init. Grep-verified: zero callers outside
`src/value/object/` — `.vtable` written 3×, read 0×; `.props`
write-only; `slots` never read; only `ObjData::{Ordinary,Array,Idx}`
constructed. The dead copy disagrees with the live store on attribute
defaults (`unwrap_or(false)` vs legacy `unwrap_or(true)`).

- [ ] `#[test]`: array assign + defineOwnProperty survives (refactor pin).
- [ ] Delete the lot, including re-exports and `props` sync writes in
      `new_array`.

~470 LOC saved. No R0 dependency — nothing routes through it.

## R5 — Collapse `Object` property storage + fix spec semantics  *(HIGH)*

Re-audit 2026-07-22 (`tasks/review-2026-07-22-object-model.md`).
`value/object/helpers.rs:268-288`: parallel maps (`properties`,
`elements`, `getters`, `setters`, `descriptors`, `symbol_properties`,
`holes`) plus a hand-rolled third lookup path in
`eval/member/object_member.rs` (own prototype walk, non-canonical index
parsing, ignores holes/setters/descriptors, fresh Date prototype per
access). Three same-shaped descriptor types (`PropertyFlags`,
`PropertyDescriptor`, `Desc`); six accessor types (four dead → R9).

Spec bugs to fix while collapsing — failing reproducer `#[test]` first,
each one:

- Attribute defaults inverted: `Object::define_own_property` defaults
  writable/enumerable/configurable `true` (`property.rs:261-263`);
  `Object.defineProperty` spec default is `false`.
- `Object::set` silently swallows writes to non-writable props /
  non-extensible objects (`property.rs:40-49`); strict mode must throw
  TypeError, sloppy must no-op.
- No ValidateAndApplyPropertyDescriptor: non-configurable invariants
  (redefine, data↔accessor, writable flip) never checked.
- `Symbol` has no identity id (`val.rs:37-40`); `symbol_properties`
  keyed by desc — two `Symbol("x")` collide, all `Symbol()` share `""`.
  AGENTS.md mandates `desc\0id`.
- Key ordering: `keys.rs:54` excludes the string `"length"`
  unconditionally; array `own_keys` lists hole indices; symbols never
  appear in `own_keys`.
- Seal/freeze uncomputable: one `extensible` bool; the elements path
  creates no descriptor entries.
- `get_own_property` lies about element attributes and drops `set` when
  a getter+setter pair exists; `to_object("ab")` puts the whole string
  in element `"0"`.

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
      hand-rolled walk in `object_member.rs` (~50 LOC + a bug class).
- [ ] Easier to land after R0 (JS never touches storage directly).

~170 LOC saved + symbol-key correctness + the spec bugs above.

## R6 — `Realm` owns intrinsic prototypes; `%ThrowTypeError%` identity  *(MED-HIGH)*

`context/mod.rs:166-199`, `builtins/mod.rs:118-152`. `Context::reset`
clears only 2 of ~14 thread_local proto pointers → captured closures
leak into dead realms. `%ThrowTypeError%` doesn't exist (test262
`Throw*` skip-listed at `test262/runner.rs:61`).

- [ ] `#[test]`: after `reset`, a native getter resolves against new realm.
- [ ] `Realm` struct owns intrinsic prototypes once; `Context::new`
      clones from a `Realm` template. `bootstrap.rs` runs once per `Realm`.
- [ ] `reset` clears all thread_local proto pointers consistently
      (ideally zero — they live on `Realm`).
- [ ] `%ThrowTypeError%` constructed once per `Realm` with stable identity.

Unblocks the `ThrowTypeError` stage (`tasks/index.json`).

## R7 — One `to_object`  *(absorbed by R1)*

Three divergent boxers: `eval/object.rs:15-62`, `eval/member.rs:154-207`,
`value/convert.rs:765-810` (boxes `undefined`/`null`, spec-incorrect).
R1's `to_object` replaces all three; delete on touch.

## R8 — `panic!` → `throw_type_error`  *(MED)*

Panics: `builtins/uri.rs:289,297,305`, `builtins/date.rs:714,730`,
`builtins/number.rs:334,341,356`,
`builtins/array/methods/rearrange.rs:142`, `builtins/date.rs:507`.
Mixed throws: `Err(JsError::from("TypeError:..."))` in `map.rs`,
`weak.rs`, `object_static.rs`.

- [ ] `value::error::throw_type_error(msg) -> JsError` one-line helper.
- [ ] `#[test]` per panic site: catchable `TypeError`, expected message.
- [ ] Replace all panics + `JsError::from` string-form throws.

Most callsites vanish under R0 (JS throws natively); R8 sweeps the rest.

## R9 — Dead code sweep  *(LOW-MED)*

After R0/R1/R4 reduce the surface:

- `value/convert.rs:448-460,491-525` `to_primitive_for_compare` +
  `object_to_primitive_for_compare` (`#[allow(dead_code)]`).
- `interpreter.rs:36-44` `is_control_flow_set`, `:54-56`
  `set_max_call_depth`.
- `value/object.rs:218-257` `Getter`/`Setter`/`GetterBody`/`SetterBody`
  (re-confirmed 2026-07-22: now in `value/object/helpers.rs:194-220`,
  zero uses outside re-export lists).
- `value/object/helpers.rs`: `ThisMode`, `ObjData::{String,Func,Proxy,
  Args}` (never constructed).
- `value/object/object.rs`: merge `new`/`with_prototype` (25 LOC dup).
- `value/object/property.rs:311-371`: one-line delegation wrappers —
  call `accessor::`/`keys::` directly (~40 LOC).
- `value/object/keys.rs`: collapse `own_keys`/`own_property_names` into
  one fn with an `enumerable_only` flag (~25 LOC; fixes the `"length"`
  exclusion in passing).
- `value/kind.rs`: collapse 4-helper Display into one 13-arm match
  (~40 LOC; the split exists only to dodge the complexity lint).
- `value/val.rs`: split `ClassValue` + tests out (700 → ≤ 500).
- `value/object.rs:270-288` `PropertyFlags::default_data/default_accessor`.
- `value/convert.rs:144-158` `to_number_unchecked`.
- `builtins/mod.rs:42-93` `JsValueProxy` serde glue.
- `builtins/intl.rs` (100 LOC) — never registered; out of scope per
  `tasks/index.json`. Delete; re-add as `builtins/Intl.js` stub *only*
  if a stage needs it.

~620 LOC saved.

## R10 — RAII `CURRENT_CONTEXT`; collapse thread-locals  *(MED)*

`context/mod.rs:55-64,101-103` open-coded save/restore; `return Err`
paths skip restore → dangling pointer. `Cell<Option<Value>>::take`+`set`
peek pattern used 150+ times.

- [ ] `struct CtxGuard { prev } impl Drop`; `eval_impl` constructs one.
- [ ] `RefCell<Option<Value>>` + `.borrow().clone()` instead of take+set.
- [ ] R6 moves per-realm pointers onto `Realm`; `reset` becomes trivial.

## R11 — `Context::call_js_function` → `eval::function::call_value`  *(LOW)*

`context/mod.rs:524-581` reimplements param binding/defaults/arrow
dispatch for tests. Diverges from `eval::function::call_value`.

- [ ] Route `Context::call_function` through `call_value_with_this`
      (`this = Undefined`). Delete the duplicates.

~55 LOC saved.

## R12 — Split `eval/object.rs`  *(DONE)*

`eval/object.rs` is 466 lines (2026-07-22); the split landed as
`eval/object/` submodules. Remaining over-500 offenders are tracked in
R15.

## R15 — Linter-gate sweep: files > 500 lines, `#[allow(...)]`  *(HIGH)*

The gate is policy; the repo still violates it. Catalogue (2026-07-19,
re-verify with `wc -l` / `rg '#\[allow\('`): T4 in
`tasks/review-2026-07-19.md`.

- [ ] Split or shrink every production file > 500 lines
      (`eval/statement.rs`, `eval/class/helpers.rs`, `interpreter/`,
      `value/function/value_function.rs` (975), `value/val.rs` (700),
      `value/error.rs` (574), `value/generator.rs` (690),
      `eval/object.rs` (539), `builtins/json.rs`, … — 2026-07-22 counts).
      Most shrink under R0/R5; split what remains.
- [ ] Remove every `#[allow(...)]` in `src/` — delete the dead code or
      refactor until the lint passes unsuppressed.
- [ ] Acceptance: `rg '#\[allow\(' crates/quench-runtime/src` zero hits;
      no production file > 500 lines; clippy clean.

## R16 — Drop `FROZEN_OBJECTS` thread_local  *(MED, soundness)*

`builtins/object_static.rs` stores `Rc::as_ptr` in a thread_local
`Vec<usize>` on `Object.freeze`; never cleared on `reset`, so a reused
address reports frozen. Details + repro test: T14 in
`tasks/review-2026-07-19.md`.

- [ ] Use the existing `Object.extensible` field instead; delete
      `FROZEN_OBJECTS` + `is_frozen_object`.

## R13 — `object_static.rs` cleanup  *(absorbed by R0 + R5)*

- `object_from_entries` (`object_static.rs:69-107`) iterates `elements`
  directly. R0 reimplements in JS via `for_each_iterable`.
- `FROZEN_OBJECTS` thread_local `Vec<usize>` leaks frozen objects;
  use the existing `Object.extensible` field.

## R14 — `lower_expr` fail-loud on unknown  *(LOW)*

`lower/expr.rs:60-73` silently returns `Expression::Undefined` for
unknown variants — BigInt literals become `undefined`. Switch the
catch-all to `Err` so new OXC variants surface at lower time.

## R17 — OXC early errors via `oxc_semantic`  *(HIGH, strategy S3)*

A large slice of the 23,711 `test/language` tests are static-semantics
early errors (duplicate declarations, invalid assignment targets,
strict-mode restrictions, label rules). Hand-rolling them in `lower/`
is thousands of LOC plus endless edge cases; OXC already implements
them.

- [ ] `DEPENDENCIES.md` row for `oxc_semantic` / `oxc_diagnostics` in
      the same diff.
- [ ] `#[test]`: a known early-error case (e.g. duplicate `let` in one
      block) surfaces as a JS `SyntaxError`, catchable via
      `assert.throws(SyntaxError)` semantics.
- [ ] Parse → semantic check → SyntaxError before lowering; delete any
      hand-rolled early-error checks it makes redundant.

## Sequencing

```
R1 → R0   (R1 is the blocker; R0 is the pivot)
R0 → R2 R7 R8 R13
R4 anytime — dead code, no blockers (~470 LOC; do first)
R6 parallel to R0; unblocks the ThrowTypeError stage
R3 part of R0 (Date.js)
R5 after R4 (+ R0 ideally) — includes the spec-bug reproducers
R9 after R4
R17 parallel; highest test-per-LOC ratio in the language half
R10 R11 R14 R16 anytime after their blockers
R15 continuous gate; final sweep after R0/R5
```

Every item lands with `cargo test -p quench-runtime` +
`cargo clippy -p quench-runtime --all-targets` clean. test262 stage
gate (`tasks/index.json`) must not regress.