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
by symbol name before editing.

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

## R4 — Delete speculative `TComp` infra  *(HIGH)*

`value/object.rs:18-345, 381-388, 630-733`: `Key`/`Desc`/`ObjData`/
`VTable`/`Slots`/`props`/`slots`/`data`/`vtable`. Only `Ordinary`/`Array`
constructed; zero `vtable.X()` dispatches; `Slots` never used.

- [ ] R0 makes them unreachable from JS (JS only calls `%ops%`).
- [ ] `#[test]` array assign + defineOwnProperty survives.
- [ ] Delete the lot.

~330 LOC saved.

## R5 — Collapse `Object` property storage  *(HIGH)*

`value/object.rs:347-389`. Five parallel maps (`properties`, `elements`,
`getters`, `setters`, `descriptors`, `symbol_properties`, `holes`).
`symbol_properties` keyed by symbol *description string* — same-desc
symbols collide.

- [ ] `#[test]`: two `Symbol("x")` keys on one object don't collide.
- [ ] Collapse to `own_props: IndexMap<Key, Prop>` where
      `Prop = Value | Accessor{get,set}` + `PropertyAttributes`;
      `Key::Sym(Rc<Symbol>)`. Array as `Vec<Option<Value>>` with
      `Value::Hole` for holes.
- [ ] Easier to land after R0 (JS never touches storage directly).

~120 LOC saved + symbol-key correctness.

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
- `value/object.rs:218-257` `Getter`/`Setter`/`GetterBody`/`SetterBody`.
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
      `value/function/value_function.rs`, `builtins/json.rs`, …). Most
      shrink under R0/R5; split what remains.
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
R0 → R2 R4 R7 R8 R9 R13
R6 parallel to R0; unblocks the ThrowTypeError stage
R3 part of R0 (Date.js)
R5 after R0 (storage is internal-only then)
R17 parallel; highest test-per-LOC ratio in the language half
R10 R11 R14 R16 anytime after their blockers
R15 continuous gate; final sweep after R0/R5
```

Every item lands with `cargo test -p quench-runtime` +
`cargo clippy -p quench-runtime --all-targets` clean. test262 stage
gate (`tasks/index.json`) must not regress.