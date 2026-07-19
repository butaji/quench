# Refactor Plan — Highest Priority

Cross-cutting simplification pass. Goal: zero duplicated spec ops, zero
dead code, dead-simple runtime. Every item below follows the
`AGENTS.md` "unit tests, not guesswork" cycle — write a `#[test]` that
asserts the current broken/divergent behavior first, watch it fail, do
the minimal move-and-route fix, verify, leave the test in.

Driven by the architecture review. Total conservative estimate:
**~1,400–1,700 LOC removable** plus correctness wins (live iterator
protocol, realm reuse, catchable TypeError, panic-free builtins).

Sequenced so each item unblocks the next:

## R0 — `eval/ops.rs` as the one canonical home for spec ops  *(HIGH, blocker)*

`eval/ops.rs` does not exist today. AGENTS.md names it as the canonical
home for `ToPrimitive`, `ToPropertyKey`, `IteratorNext`, `IteratorClose`,
`CreateDataPropertyOrThrow`, `OrdinaryHasProperty`, `IsCallable`, … but
the rule was never enforced. Every item below depends on it.

- [ ] Create `src/eval/ops.rs` with: `to_primitive`, `to_property_key`,
      `to_object`, `ordinary_has_property`, `is_callable`, `is_constructor`,
      `create_data_property_or_throw`, `same_value_zero`, `native_fn`.
- [ ] One `#[test]` per op asserting observable behavior (mirror
      `src/eval/string_methods.rs` style).
- [ ] Route *every* builtin and eval node through `eval/ops.rs`. Delete
      the per-builtin private copies as they're touched.

## R1 — One iterator protocol  *(HIGH)*

Four independent iterator implementations today:

- `eval/iteration.rs:16-58` `get_iterator` — eagerly materializes
  `Vec<Value>`, never calls `.next()`, breaks generators/infinite
  iterators.
- `builtins/weak.rs:254-410` `for_each_on_iterable` — only spec-shaped
  one.
- `builtins/map.rs:212-247` `make_iterator` — bespoke `{next()}` never
  linked to `%IteratorPrototype%`.
- `eval/object.rs:108-273` `obtain_iterator` /
  `take_iterator_value` / `call_iterator_return` — used only by array
  destructuring; re-evaluates getter every loop.

Tasks:
- [ ] `eval/ops.rs`: `iterator_next`, `iterator_step`, `iterator_close`,
      `get_iterator` (returns a handle, not `Vec<Value>`),
      `for_each_iterable`, `make_iterator_object`.
- [ ] Build `%IteratorPrototype%` once at realm init (see R5); all
      custom iterators link via prototype chain.
- [ ] Replace `get_iterator` callers (`promise/static_methods.rs:531`,
      `eval/literal.rs:228`, `eval/call.rs:56`) with the streaming
      protocol.
- [ ] Delete `map.rs::make_iterator`, `weak.rs::for_each_on_iterable`,
      `eval/object.rs::obtain_iterator*`.

Estimated ~400 LOC saved.

## R2 — Delete speculative `TComp` infra  *(HIGH)*

`value/object.rs:18-345, 381-388, 630-733`:
- `pub enum Key { Str, Idx, Sym }` — `Sym` constructed nowhere.
- `pub struct Desc` — built, no consumer except dead `props`.
- `pub enum ObjData { Ordinary, Array, String, Func, Proxy, Args, Idx }` —
  only `Ordinary`/`Array` constructed; `Idx` written once, never read.
- `pub struct VTable { 13 fields }` — zero `obj.vtable.X()` dispatches.
- `pub type Slots` + `pub slots` field — zero insert/get calls.
- `pub props` field — 15 uses vs 78 for `.properties`.

Tasks:
- [ ] `#[test]` proving array assignment + `defineOwnProperty` still
      works after removing `Key`/`Desc`/`ObjData`/`VTable`/`Slots`/
      `props`/`slots`/`data`/`vtable` fields.
- [ ] Delete the lot. Re-route `new_array` / `array_define_own_property`
      to write into `properties` / `elements` directly.

Estimated ~330 LOC saved.

## R3 — Use `chrono` for `date.rs`  *(HIGH)*

`builtins/date.rs:503-560` hand-rolls leap-year / days-from-ymd math
under names `chrono_*` but `chrono` is never imported. `chrono` is
already a dep. `chrono_now().unwrap()` also panics.

Tasks:
- [ ] `#[test]` for `Date.UTC(year, month, day, …)` covering leap years
      and pre-1970 dates.
- [ ] Swap to `NaiveDate::from_ymd_opt` + `num_days_from_ce`;
      `Utc::now().timestamp_millis()` for `now`. Delete the hand-rolled
      helpers.

Estimated ~50 LOC saved.

## R4 — Collapse `Object` property storage  *(HIGH)*

`value/object.rs:347-389`. Five parallel maps: `properties`,
`elements`, `getters`, `setters`, `descriptors`, `symbol_properties`,
`holes`, plus dead `props`. Every `set` writes a sync `descriptors`
entry. `symbol_properties` keyed by symbol *description string* — two
symbols with the same description collide.

Tasks:
- [ ] `#[test]`: two `Symbol("x")` keys on the same object must not
      collide.
- [ ] Replace with `own_props: IndexMap<Key, Prop>` where
      `Prop = Value | Accessor { get, set }` carrying `PropertyAttributes`.
      `Key::Sym(Rc<Symbol>)`. Array elements: `Vec<Option<Value>>` with
      `Value::Hole` for holes (or side `Vec<Value>` + `HashSet<usize>`).

Estimated ~120 LOC saved + correctness on symbol keys.

## R5 — Share intrinsic prototypes across realms; add `%ThrowTypeError%`  *(MED-HIGH)*

`context/mod.rs:166-199`, `builtins/mod.rs:118-152`. `Context::reset`
clears only 2 of ~14 thread_local prototype pointers; native-getter
closures keep pointing into the dead realm. `%ThrowTypeError%` doesn't
exist as a function (test262 `Throw*` cases are skip-listed at
`test262/runner.rs:61`).

Tasks:
- [ ] `#[test]`: after `Context::reset`, getter on a captured intrinsic
      still resolves against the new realm.
- [ ] Introduce a `Realm` struct owning intrinsic prototypes once.
      `Context::new` clones from a realm template rather than rebuilding.
- [ ] `Context::reset` clears **all** thread_local proto pointers
      consistently (or — better — they live on `Realm` and `reset`
      never touches them).
- [ ] Define `%ThrowTypeError%` with stable identity as a single shared
      `NativeFunction` per realm.

## R6 — One `to_object`  *(MED)*

Three divergent boxing impls:
- `eval/object.rs:15-62` `box_primitive_for_set`
- `eval/member.rs:154-207` `box_primitive`
- `value/convert.rs:765-810` `to_object` — boxes `undefined`/`null`
  (spec-incorrect; should throw `TypeError`).

Tasks:
- [ ] `eval/ops.rs::to_object` (rejects `undefined`/`null`).
- [ ] Route the two `box_primitive*` callers through it.
- [ ] Delete both `box_primitive*` copies.

Estimated ~70 LOC saved.

## R7 — `panic!` in builtins → `JsError`; one throw helper  *(MED)*

Panics: `builtins/uri.rs:289,297,305`, `builtins/date.rs:714,730`,
`builtins/number.rs:334,341,356`,
`builtins/array/methods/rearrange.rs:142`, `builtins/date.rs:507`.

Mixed throw styles: `Err(JsError::from("TypeError: …"))` (no JS object,
loses identity/stack/subclass) in `map.rs`, `weak.rs`,
`object_static.rs`; spec-compliant `create_js_error_with_type` +
`set_thrown_value` elsewhere.

Tasks:
- [ ] Add `value::error::throw_type_error(msg) -> JsError` helper
      collapsing the dual-write protocol to one line.
- [ ] `#[test]`: each panic site produces a catchable `TypeError` with
      the expected message.
- [ ] Replace every `panic!`/`unwrap()` in `builtins/` and every
      `JsError::from("TypeError: …")` with `throw_type_error`.

## R8 — Wire or delete `BigInt` / `Intl` builtins  *(MED)*

`builtins/bigint.rs` (316) and `builtins/intl.rs` (100) exist but
`register_builtins` never calls them. `BigInt.asIntN` unreachable. Per
AGENTS.md "No speculative generality", unreachable code shouldn't be
in the tree.

Tasks:
- [ ] If this is stage-50 work: delete both modules now; re-add with the
      failing-test-first cycle when stage 50 lands.
- [ ] Either way, drop ~420 LOC of unreachable code.

## R9 — Dead code sweep  *(LOW-MED)*

- `value/convert.rs:448-460,491-525` `to_primitive_for_compare` and
  `object_to_primitive_for_compare` — both `#[allow(dead_code)]`.
- `interpreter.rs:36-44` `is_control_flow_set`, `:54-56`
  `set_max_call_depth` (pub, no callers).
- `value/object.rs:218-257` `Getter` / `Setter` / `GetterBody` /
  `SetterBody` structs — never instantiated.
- `value/object.rs:270-288` `PropertyFlags::default_data`,
  `default_accessor` — no callers.
- `value/convert.rs:144-158` `to_number_unchecked` — one trivial
  caller.
- `builtins/mod.rs:42-93` `JsValueProxy` serde glue —
  `#[allow(dead_code)]`, no callers.

Tasks: delete all of the above after confirming no callers via `grep`.
Estimated ~200 LOC saved.

## R10 — RAII `CURRENT_CONTEXT` guard; collapse thread-local count  *(MED)*

`context/mod.rs:55-64,101-103` saves/restores `prev_ctx` in two
open-coded branches; inner `return Err` paths (e.g.
`reject_eval_var_lexical_conflict`) skip the restore, leaving a dangling
pointer. `Cell<Option<Value>>::take`+`set` peek pattern used 150+ times.

Tasks:
- [ ] `struct CtxGuard { prev: Option<*mut Context> } impl Drop`.
  `eval_impl` constructs one; never hand-restores.
- [ ] Replace `Cell<Option<Value>>` peek+restore with
  `RefCell<Option<Value>>` + `.borrow().clone()`.
- [ ] Audit the 22+ thread-locals; hoist per-realm pointers onto `Realm`
  (R5) so `reset` becomes trivial.

## R11 — `Context::call_js_function` → `eval::function::call_value`  *(LOW)*

`context/mod.rs:524-581` reimplements param binding / default /
arrow-body dispatch for test/host integration. Defaults, strictness,
`arguments` already diverge from `eval::function::call_value`.

Tasks:
- [ ] Route `Context::call_function` through
      `eval::function::call_value_with_this` with `this = Undefined`.
- [ ] Delete `call_js_function`, `bind_params`, `resolve_param_value`,
      `eval_arrow_body`.

Estimated ~55 LOC saved.

## R12 — Split `eval/object.rs` (1847 LOC)  *(LOW, move-only)*

Mixes assignment + destructuring (~600 LOC) + accessor dispatch +
boxing + super.

Tasks:
- [ ] `eval/assign.rs` — identifier + member assignment.
- [ ] `eval/destructuring.rs` — array + object patterns.
- [ ] `eval/accessor.rs` — `call_getter` / `call_setter` / super access.
- [ ] Boxing moves to `eval/ops.rs::to_object` (R6).

No LOC change; collision/readability win.

## R13 — `object_static.rs` cleanup  *(LOW-MED)*

- `object_from_entries` (`object_static.rs:69-107`) iterates `elements`
  directly → `Object.fromEntries(new Map(...))` fails. Route through
  `for_each_iterable` (R1).
- `FROZEN_OBJECTS` thread_local `Vec<usize>` leaks frozen objects
  forever; `Object.extensible` field already exists. Use
  `extensible: bool`.

Tasks: route `fromEntries` through iterator protocol; delete
`FROZEN_OBJECTS`; asserts on freeze in `Object::freeze`.

Estimated ~25 LOC saved.

## R14 — `lower_expr` fail-loud on unknown  *(LOW)*

`lower/expr.rs:60-73` silently returns `Expression::Undefined` for
unknown variants — BigInt literals become `undefined`.

Tasks: switch the catch-all to return an `Err` so additions to the OXC
AST surface at lower time, not silently misbehave at eval time.