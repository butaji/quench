# Refactor Plan — Highest Priority

The architecture is a small **Rust core** plus a **self-hosted JS
builtins layer** (see `docs/architecture.md`). JS is ~1/3 the LOC of
equivalent Rust and easier to keep spec-faithful, so everything that
can be written in JS authored on top of the canonical spec ops, **is**.
Crate-backed areas stay in Rust (regress, chrono, num-bigint,
serde_json, urlencoding, oxc).

All items below follow the `AGENTS.md` "unit tests, not guesswork"
cycle. The audit baseline is ~32,339 Rust LOC with ~1,400–1,700 LOC of
duplicated/dead/speculative code. After the migration we expect the
Rust core to shrink to **~8–12k LOC** plus a JS builtins layer of
**~3–5k LOC** — total roughly half the current size, with strict
spec correspondence.

Sequenced so each item unblocks the next. **R0 is the pivot; R1 must
land before any builtin JS work; everything else follows.**

## R0 — Self-host builtins in JS  *(HIGHEST, blocker)*

Move every builtin that is pure spec algorithm on top of core ops from
`builtins/*.rs` to `builtins/*.js`. See `docs/architecture.md` for the
target layout and `%ops%` bridge.

- [ ] Create `crates/quench-runtime/builtins/` source tree, embedded
      via `include_str!` in `builtins/mod.rs`.
- [ ] Add `builtins/bootstrap.rs`: parse + eval each `.js` file in
      dependency order (see architecture doc).
- [ ] Per-builtin migration: write a failing `#[test]` covering the
      target behavior, then implement the JS shell, then delete the
      Rust `register_*` for that builtin. One builtin at a time, full
      `cargo test -p quench-runtime` green before moving to the next.
- [ ] Order: `Object.js` → `Function.js` → `Error.js` → `Symbol.js`
      → `Number.js` → `Boolean.js` → `String.js` → `Array.js`
      → `Iterator.js` → `Map.js`/`Set.js`/`Weak*.js` → `Promise.js`
      → `JSON.js` → `Reflect.js`/`Proxy.js` → `Math.js` →
      `RegExp.js` (shell over `core/regex.rs`) →
      `Date.js` (shell over `core/date.rs`) →
      `BigInt.js` (shell over `core/bigint.rs`) →
      `TypedArray.js`/`ArrayBuffer.js`/`DataView.js`/`Atomics.js`
      → URI helpers.

Migration milestone: when the first test262 stage passes with its
relevant builtins live from JS, lock the pattern in.

## R1 — `eval/ops.rs` canonical home + `%ops%` bridge  *(HIGH, blocker for R0)*

`eval/ops.rs` does not exist today. AGENTS.md names it as the canonical
home for spec abstract ops, but the rule was never enforced. R0 cannot
land without it — every JS builtin calls `%ops%`.

- [ ] Create `src/eval/ops.rs` exporting: `to_primitive`, `to_property_key`,
      `to_object`, `to_number`, `to_string`, `same_value`,
      `same_value_zero`, `is_callable`, `is_constructor`,
      `ordinary_has_property`, `create_data_property_or_throw`,
      `get_iterator`, `iterator_next`, `iterator_step`,
      `iterator_close`, `create_iter_result_object`, `native_fn`,
      `throw_type_error`.
- [ ] One `#[test]` per op, mirroring `src/eval/string_methods.rs`.
- [ ] Expose a frozen `%ops%` object onto the realm with each op as a
      bound `NativeFunction`. JSON `import` of `%ops%` is resolved by
      the parser at parse time (never user-visible).
- [ ] Replace every duplicated private copy of these ops in
      `builtins/*.rs` (during R0 migration) and `eval/` with calls
      into `eval/ops.rs`.

## R2 — One iterator protocol *(HIGH)*

Four implementations today:
- `eval/iteration.rs:16-58` `get_iterator` — eagerly materializes
  `Vec<Value>`, breaks generators/infinite iterators.
- `builtins/weak.rs:254-410` `for_each_on_iterable` — only spec-shaped one.
- `builtins/map.rs:212-247` `make_iterator` — bespoke, never linked to
  `%IteratorPrototype%`.
- `eval/object.rs:108-273` `obtain_iterator`/`take_iterator_value`/
  `call_iterator_return` — destructuring only; re-evaluates getter
  every loop.

R1 already provides `get_iterator` / `iterator_next` / `iterator_step`
/ `iterator_close` in `eval/ops.rs`. R0 builds `%IteratorPrototype%`
in JS once; Array/String/RegExp/Map/Set iterators inherit via the
prototype chain. Delete the four Rust duplicates.

## R3 — Use `chrono` for `date.rs` core  *(HIGH)*

`builtins/date.rs:503-560` hand-rolls leap-year math under names
`chrono_*` but `chrono` is never imported. Migrate `Date` to a Rust
core in `builtins/core/date.rs` exposing primitives
(`UtcTimestamp`, `YmdToMs`, `MsToYmd`) backed by `chrono::NaiveDate`
+ `chrono::Utc`. `builtins/Date.js` becomes a thin shell. Delete the
hand-rolled `is_leap_year`/`days_in_month`/`days_from_ymd`. Verified
via `ctx7` (`/websites/rs_chrono_chrono`, `/chronotope/chrono`).

## R4 — Delete speculative `TComp` infra *(HIGH)*

`value/object.rs:18-345, 381-388, 630-733`: `Key`/`Desc`/`ObjData`/
`VTable`/`Slots`/`props`/`slots`/`data`/`vtable`. Only `Ordinary`/`Array`
variants constructed; zero `obj.vtable.X()` dispatches; `Slots` field
never used. R0 makes these unreachable from JS (which only goes
through `%ops%`) so there's no reason to keep any of them.

- [ ] `#[test]` proving array assignment + defineOwnProperty still
      works after removal.
- [ ] Delete the lot; route `new_array`/`array_define_own_property`
      through `properties`/`elements` directly.

Estimated ~330 LOC saved.

## R5 — Collapse `Object` property storage *(HIGH)*

`value/object.rs:347-389`. Five parallel maps (`properties`,
`elements`, `getters`, `setters`, `descriptors`, `symbol_properties`,
`holes`). Every `set` writes a sync `descriptors` entry just to keep
maps aligned. `symbol_properties` keyed by symbol *description string*
— two same-description symbols collide.

- [ ] `#[test]`: two `Symbol("x")` keys on the same object must not
      collide.
- [ ] Collapse to `own_props: IndexMap<Key, Prop>` where
      `Prop = Value | Accessor { get, set }` carrying
      `PropertyAttributes`. `Key::Sym(Rc<Symbol>)`. Array elements as
      `Vec<Option<Value>>` with `Value::Hole` for holes (or side
      `Vec<Value>` + `HashSet<usize>`).

Once R0 lands, JS never reaches directly into `Object` storage, so
the storage layout is purely an internal performance/correctness
question — easier to land the collapse.

## R6 — `Realm` owns intrinsic prototypes; `%ThrowTypeError%` identity  *(MED-HIGH)*

`context/mod.rs:166-199`, `builtins/mod.rs:118-152`. `Context::reset`
clears only 2 of ~14 thread_local proto pointers; captured closures
leak into dead realms. `%ThrowTypeError%` doesn't exist (test262
`Throw*` cases skip-listed at `test262/runner.rs:61`).

- [ ] `#[test]`: after `Context::reset`, a native getter on a captured
      intrinsic resolves against the new realm.
- [ ] Introduce a `Realm` struct owning intrinsic prototypes once.
      `Context::new` clones from a `Realm` template; bootstrap runs
      once per `Realm`, not per `Context`.
- [ ] `Context::reset` clears **all** thread_local proto pointers
      consistently (ideally they live on `Realm` and `reset` touches
      none of them).
- [ ] Define `%ThrowTypeError%` with stable identity, constructed once
      per `Realm`.

Estimated LOC net change ~0; correctness win unblocks stage 41.

## R7 — One `to_object` in core  *(MED, absorbed by R1)*

Three divergent boxing impls today: `eval/object.rs:15-62`
`box_primitive_for_set`, `eval/member.rs:154-207` `box_primitive`,
`value/convert.rs:765-810` `to_object` (boxes `undefined`/`null`,
spec-incorrect). R1's `to_object` replaces all three; delete the
duplicates as the migration reaches each callsite.

## R8 — `panic!` in builtins → `throw_type_error`  *(MED)*

Panics: `builtins/uri.rs:289,297,305`, `builtins/date.rs:714,730`,
`builtins/number.rs:334,341,356`,
`builtins/array/methods/rearrange.rs:142`, `builtins/date.rs:507`.

- [ ] `value::error::throw_type_error(msg) -> JsError` helper (one line
      performing `create_js_error_with_type` + `set_thrown_value`).
- [ ] `#[test]` per panic site: produces catchable `TypeError` with
      expected message.
- [ ] Replace every `panic!`/`unwrap()` in `builtins/` and every
      `JsError::from("TypeError: …")` with `throw_type_error`.

Note: most of these callsites disappear under R0 (JS throws naturally).
R8 is the cleanup pass after the Rust surface has shrunk to the core.

## R9 — Dead code sweep  *(LOW-MED)*

- `value/convert.rs:448-460,491-525` `to_primitive_for_compare` and
  `object_to_primitive_for_compare` — `#[allow(dead_code)]`.
- `interpreter.rs:36-44` `is_control_flow_set`,
  `interpreter.rs:54-56` `set_max_call_depth` (pub, no callers).
- `value/object.rs:218-257` `Getter`/`Setter`/`GetterBody`/`SetterBody`
  structs — never instantiated.
- `value/object.rs:270-288` `PropertyFlags::default_data`,
  `default_accessor` — no callers.
- `value/convert.rs:144-158` `to_number_unchecked` — one trivial
  caller.
- `builtins/mod.rs:42-93` `JsValueProxy` serde glue —
  `#[allow(dead_code)]`, no callers.
- `builtins/bigint.rs` (316 LOC) and `builtins/intl.rs` (100 LOC)
  exist but `register_builtins` never invokes them. R0 supersedes:
  BigInt migrates to `builtins/BigInt.js` over `core/bigint.rs`;
  Intl moves to `builtins/Intl.js` stub (or stays out of scope per
  `tasks/index.json`).

Tasks: delete after confirming zero callers via `grep`.
Estimated ~200 LOC + ~420 LOC saved.

## R10 — RAII `CURRENT_CONTEXT` guard; collapse thread-locals  *(MED)*

`context/mod.rs:55-64,101-103` saves/restores `prev_ctx` in two
open-coded branches; inner `return Err` paths
(`reject_eval_var_lexical_conflict`) skip the restore, leaving a
dangling pointer. `Cell<Option<Value>>::take`+`set` peek pattern used
150+ times.

- [ ] `struct CtxGuard { prev: Option<*mut Context> } impl Drop`.
  `eval_impl` constructs one; never hand-restores.
- [ ] `RefCell<Option<Value>>` + `.borrow().clone()` instead of
  `Cell<Option<Value>>` take+set.
- [ ] R6 moves per-realm pointers onto `Realm`; `reset` becomes
  trivial.

## R11 — `Context::call_js_function` → `eval::function::call_value` *(LOW)*

`context/mod.rs:524-581` reimplements param binding / default /
arrow-body dispatch for test/host integration. Defaults, strictness,
`arguments` already diverge from `eval::function::call_value`.

- [ ] Route `Context::call_function` through
      `eval::function::call_value_with_this` with `this = Undefined`.
- [ ] Delete `call_js_function`, `bind_params`, `resolve_param_value`,
      `eval_arrow_body`.

Estimated ~55 LOC saved.

## R12 — Split `eval/object.rs` (1847 LOC)  *(LOW, move-only)*

Mixes assignment + destructuring (~600 LOC) + accessor dispatch +
boxing + super. R0 reduces the surface that calls into these helpers
(they become JS), shrinking this file naturally. Remaining Rust is
assignment-target resolution + accessor internal calls.

- [ ] `eval/assign.rs` — identifier + member assignment.
- [ ] `eval/destructuring.rs` — array + object patterns.
- [ ] `eval/accessor.rs` — `call_getter` / `call_setter` / super access.
- [ ] Boxing moves to `eval/ops.rs::to_object` (R1).

No LOC change; collision/readability win.

## R13 — `object_static.rs` cleanup  *(LOW-MED)*

- `object_from_entries` (`object_static.rs:69-107`) iterates `elements`
  directly. R0 reimplements it in JS via `for_each_iterable` naturally;
  delete the Rust version.
- `FROZEN_OBJECTS` thread_local `Vec<usize>` leaks frozen objects
  forever; `Object.extensible` field already exists. Use
  `extensible: bool`.

Estimated ~25 LOC saved + leak fix.

## R14 — `lower_expr` fail-loud on unknown  *(LOW)*

`lower/expr.rs:60-73` silently returns `Expression::Undefined` for
unknown variants — BigInt literals become `undefined`.

Tasks: switch the catch-all to return an `Err` so additions to the OXC
AST surface at lower time, not silently misbehave at eval time.

## Sequencing summary

```
R1 (eval/ops.rs + %ops%)              blocks  R0
R0 (self-hosted builtins)             blocks  R4 R7 R8 (most surfaces)
R2 (one iterator protocol)            depends on R1
R3 (chrono core for Date)             part of  R0 (Date.js)
R4 (delete TComp infra)               after   R0 made it unreachable
R5 (collapse Object storage)          after   R0 (JS never touches storage)
R6 (Realm + %ThrowTypeError%)         unblocks stage 41
R7 (one to_object)                    absorbed by R1
R8 (panic→throw cleanup)              after    R0 collapses surface
R9 (dead code sweep)                  anytime after R0
R10 (RAII CURRENT_CONTEXT)            anytime, parallel to R0
R11 (Context::call_js_function)       after R0
R12 (split eval/object.rs)            after R0
R13 (object_static cleanup)           absorbed by R0 + R5
R14 (lower_expr fail-loud)            anytime
```

Every item must reach `cargo test -p quench-runtime` +
`cargo clippy -p quench-runtime --all-targets` clean before merge.
test262 stage gate is unchanged — `tasks/index.json` stages still drive
the conformance bars each refactor must not regress.