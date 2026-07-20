# Code Review — Duplications, Unnecessary Complexity, Messy Parts (2026-07-19)

Companion to `tasks/review-2026-07-19.md` (arch+ranked findings).
This document focuses on **duplication**, **unnecessary complexity**,
and **messy patterns** at the code-idiom level — the smaller-scale
cruft that the architecture-level findings don't enumerate. Each
section is ranked by severity. Cross-references to the existing
R0–R14 / T1–T14 entries are explicit; new IDs are `Dn` (dup),
`Un` (complexity), `Mn` (messy). Nothing was modified or executed;
every claim is verifiable with `rg` / `wc -l` / `Read`.

---

## Duplications

### D1 — **Critical**. `same_value_zero` copy-pasted 3× with identical bodies

Verified byte-equivalent core (modulo `match` vs `if let` reshaping):

```
builtins/map.rs:16              fn same_value_zero(a: &Value, b: &Value) -> bool
builtins/weak.rs:240            fn same_value_zero(a: &Value, b: &Value) -> bool
builtins/array/methods/search.rs:64  fn same_value_zero(a: &Value, b: &Value) -> bool
```

Body in all three: NaN-aware number equality, otherwise `strict_eq`.
The canonical home is `value/convert.rs::same_value` (already exists,
`convert.rs:302`) — this is `same_value`'s NaN-tolerant sibling.

- [ ] **Repro test:** `#[test]` mirroring `convert.rs::same_value`'s
      test for the hoisted `same_value_zero` (NaN, +0/-0, Symbol,
      Object identity, String).
- [ ] Hoist to `value/convert.rs` (`pub fn same_value_zero`).
- [ ] Delete the three private copies.
- [ ] **Acceptance:** `rg -n 'fn same_value_zero'` returns exactly one
      hit in `src/`.

### D2 — **High**. Three divergent `to_object` boxers

Already T2 / R7. Listing here for ranking parity: `eval/object.rs:15-62`,
`eval/member.rs:154-207`, `value/convert.rs:765-810` (last boxes
`undefined`/`null` — spec-incorrect). R1's `ops::to_object` is the
canonical home.

### D3 — **High**. Four iterator protocol implementations

Already T2 / R2. Listing for ranking parity: `eval/iteration.rs:16-58`
(eager `Vec<Value>` — breaks generators, the smoking gun),
`builtins/weak.rs:254-410`, `builtins/map.rs:212-247 make_iterator`,
`eval/object.rs:108-273 obtain_iterator`. None linked to
`%IteratorPrototype%`.

### D4 — **Med-High**. Three independent "use strict" directive detectors

```
strict_reserved.rs:86   fn has_use_strict_directive(program)
interpreter.rs:557      fn check_use_strict_directive(statements: &[Statement])
eval/function.rs:263   fn check_use_strict(body: &[Statement])
```

All scan leading statements for a `"use strict"` string literal. The
lowering already tags directives (`parser.rs:30` calls
`has_use_strict_directive` against OXC program); the runtime should
read *one* AST-driven strict flag instead of re-scanning statement
lists at every function call (`call_js_function_impl_with_strict:118-121`
re-runs `check_use_strict` on every call).

- [ ] **Repro test:** deep eval-of-function-in-script that exercises
      the strict-inference chain — assert that strictness captured at
      definition matches what `has_use_strict_directive` would return
      once.
- [ ] Promote `has_use_strict_directive` to the single detector; have
      lowering write a `body_is_strict: bool` on `ValueFunction` at
      creation time (or read once into `f.strict`).
- [ ] Delete `check_use_strict_directive` and `check_use_strict`.

### D5 — **Med**. Two argument-object creators

```
eval/function.rs:285   fn create_arguments_object(f: &ValueFunction, args, strict_mode: bool)
eval/class.rs:445      fn create_arguments_object_simple(args: Vec<Value>) -> Value
```

`class.rs` re-rolls a stripped-down version because `f` isn't available
at class-constructor parameter-binding time. Live duplication of the
arguments-object spec algorithm.

- [ ] Hoist the arguments-object factory to `eval/function.rs` taking
      `Option<&[Param]>` (or a small `ArgObjSpec`), so class + ordinary
      both thread through one path.
- [ ] **Repro test:** `arguments[0]` after class constructor with
      rest param, after ordinary function with rest param — both via
      the hoisted factory.

### D6 — **Med**. `call_value` and `Context::call_js_function` re-roll param binding

Already T13 / R11. Listing for ranking parity: `eval/function.rs:25-160`
(`call_value_impl`, `call_js_function_impl_with_strict`, env setup,
arrow dispatch, sloppy-this boxing, depth guard) vs
`context/mod.rs:602-659` (`call_js_function`, `bind_params`,
`resolve_param_value`, `eval_arrow_body` — none of: arguments object,
strict-mode boxing, depth guard, new.target binding, sloppy-this
boxing). Diverges on every one of those points.

### D7 — **Med**. Three JsError-throwing idioms + dual-state error model

| Idiom | Sites | What it does |
|-------|-------|--------------|
| `JsError("...")` plain | 108 | String only — no JS Error object |
| `JsError::from("...")` | 27 | String only — no JS Error object |
| `create_js_error_with_type(msg, "TypeError")` + `set_thrown_value(err)` | ~10 | Constructs a JS Error object AND sets `THROWN_VALUE` |

Messy because the runtime has **two parallel error channels**:
`Result<_, JsError>` (the Rust Result) and `THROWN_VALUE` thread_local
(the JS-side error object). 79 sites call `set_thrown_value` /
`get_thrown_value`. `eval_impl` (`context/mod.rs:111-117`) catches an
`Err` and *re-reads* the message from `THROWN_VALUE` (falling back to
"unknown eval error" if the producer forgot to `set_thrown_value`,
silently losing the actual message string).

- [ ] Canonical: `throw_type_error(msg) -> JsError` (already in R8/T6)
      that returns *one* `JsError` carrying both the message and the
      constructed JS Error object as a `Value` field.
- [ ] Eliminate `THROWN_VALUE` thread_local; `catch` reads the
      `JsError`'s embedded Value directly.
- [ ] Audit all 135 throw sites; replace string-form and
      `JsError::from(\"TypeError: ...\")` with typed helpers
      (`throw_type_error`, `throw_range_error`, `throw_syntax_error`,
      `throw_reference_error`).
- [ ] **Repro test:** every throw site catchable as a JS TypeError from
      JS code (e.g. `try { ... } catch (e) { assert(e instanceof TypeError) }`),
      and the Rust `JsError.0` message matches the JS
      `Error.prototype.message`.

### D8 — **Med**. Three `JsError` constructors that aren't `JsError::new`

`JsError::new`, `JsError::from(&str)`, `JsError::from(String)`, plus
the struct field `JsError(pub String)` constructed directly — four
different ways to make a `JsError`. Pick one (`JsError::new`) and make
the field private.

### D9 — **Med**. `Object` API surface has 4× getters, 4× setters, 8× predicates

Verified `pub fn` count in `value/object.rs`: 46. Subset by concern:

| Concern | Methods | Count |
|---------|---------|-------|
| Get | `get`, `get_own_value`, `get_property`, `get_own_property` | 4 |
| Set | `set`, `define`, `set_symbol`, `set_symbol_value` | 4 |
| Has | `has`, `has_symbol`, `has_getter`, `has_setter`, `is_accessor`, `is_data`, `is_enumerable`, `is_generic` | 8 |
| Accessor | `set_getter`, `set_getter_func`, `set_setter`, `set_setter_func`, `define_accessor` | 5 |
| Keys | `own_keys`, `own_property_names` | 2 |
| New | `new`, `new_array`, `new_array_checked`, `with_prototype` | 4 |

Spec §6.1.7.2 is one `[[Get]]`/`[[Set]]`/`[[DefineOwnProperty]]`/`[[GetOwnProperty]]`/`[[HasProperty]]` algorithm.
R5 collapses the storage; collapse the **API** in the same PR: one
`get_own_property(key: &Key) -> Option<Prop>`, one
`define_own_property(key, desc)`, one `has_property`, one
`own_property_keys() -> Vec<Key>`. Spec-accessor-only shims (like
`is_enumerable`) become a one-line check on the returned
`PropertyAttributes`.

- [ ] Fold this API consolidation into R5 (storage collapse); the
      list of callers is large enough that doing it in the same PR as
      the storage migration is cheaper than two passes.

---

## Unnecessary complexity

### U1 — **Med-High**. Two parallel object-typing systems

`ObjectKind` (13 variants in `value/kind.rs:7-21`, 250 references) and
`ObjData` (in `value/object.rs:99`, 6 construction sites total —
`Ordinary` and `Array` only). The `key.rs:29-77` `Display` impl splits
the 13 kind names across 4 helper `match`es purely to dodge the
clippy complexity lint (artificial decomposition). Either:
- Collapse to the single `Ordinary`/`Array` dichotomy if exotic
  behavior lives elsewhere, OR
- Drop `ObjectKind` and route through `ObjData` if that's the planned
  migration path.

[ ] **Repro test:** exhaustive `Object::new(kind)` round-trip for each
    kind used in production (verified by `rg 'ObjectKind::'`).
[ ] Decide one model; delete the other; eliminate the `simple_kind_name`
    / `medium_kind_name` / `weak_kind_name` / `complex_kind_name`
    scaffolding.

### U2 — **Med-High**. `TComp` speculative infrastructure (`Key`/`Desc`/`VTable`/`Slots`)

Already A3 / T5 / R4. Ranked here as *unnecessary complexity*:
~330 LOC of dispatch machinery that has zero `vtable.X()` callers and
zero `Slots` users. Delete in the same PR as R0 makes it unreachable.

### U3 — **Med**. Five parallel property-storage maps on `Object`

Already R5/T3. Ranked here as complexity: `properties`, `elements`,
`getters`, `setters`, `descriptors`, `symbol_properties`, `holes` — all
on `value/object.rs:347-389`. Collapse to one `IndexMap<Key,
Prop+Attributes>`; array as `Vec<Option<Value>>` with `Value::Hole`.

### U4 — **Med**. `call_js_function_impl_with_strict` and its `force_strict: bool`

`eval/function.rs:103-107`. The `force_strict` flag is documented as
"used by `call_getter` (ES §10.4.3) where getter functions must execute
in the strict mode of the call site." Per ES §8.1.1.5
`GetThisEnvironment` + §10.2.1.2, **function strictness is captured at
definition, never at the call site** (`AGENTS.md` enforces this). The
`force_strict` parameter is speculative generality / spec
misremembering: getters' strictness comes from their bodies / enclosing
class bodies, not the caller. Combine with the `is_arrow: bool` flag
already on `ValueFunction` and the boolean-parameter count approaches
`.clippy.toml`'s `max-fn-params-bools = 3`.

- [ ] **Repro test:** a getter defined in a sloppy body called from
      strict code -- per spec, `this` inside the getter is the sloppy
      boxed receiver; with `force_strict` it loses boxing.
- [ ] Verify no test262 case relies on the broken behavior; delete
      `force_strict` and `call_js_function_impl_with_strict`; fold into
      `call_js_function_impl`.

### U5 — **Med**. `Expression::ForOf` / `Expression::ForIn` on the Expression enum

`ast.rs:166-...`: `ForOf { variable, iterable, body }` and
`ForIn { ... }` are modeled as `Expression` variants. Lowering wraps
them in `Statement::Expression(Box<Expression>)`
(`lower/control_flow.rs:97, 126`) and then `eval/expression.rs` and
`interpreter.rs:718, 730, 736` and `lower/control_flow.rs:97, 126`
both *un-fake* the statement. A for-loop is a statement. The
`Statement` enum should have `ForOf`/`ForIn`; current shape adds
boiler-plate at every match site for no benefit.

- [ ] Add `Statement::ForOf { variable, iterable, body }` and
      `Statement::ForIn { variable, object, body }`; lower directly.
- [ ] Remove `Expression::ForOf`/`ForIn`; cleanup match sites in
      `eval/expression.rs:220-225`, `interpreter.rs:718-738`,
      `lower/control_flow.rs:97, 126`.
- [ ] **Acceptance:** `rg 'Expression::ForOf|Expression::ForIn'`
      returns zero hits.

### U6 — **Med**. Five parser entry points; one is `parse_jsx` byte-identical to `parse_script`

`parser.rs` exposes:

```
parse_script(source: &str)
parse_es_module(source: &str)
parse_jsx(source: &str)        # byte-identical to parse_script
parse_typescript(source: &str) # line-based strip_imports_exports hack
parse_ts(source: &str)         # #[allow(dead_code)] -- dead
```

`parse_jsx` and `parse_script` are byte identical (both
`SourceType::default().with_jsx(true)`). `parse_ts` is annotated dead
(R9). `parse_typescript` strips imports/exports by *line-prefix
matching* (`strip_imports_exports` at `parser.rs:91`) — line comments
inside templates containing "import " would be stripped; it's the
sort of fragile thing that breaks silently.

- [ ] Delete `parse_jsx` (callers go through `parse_script`).
- [ ] Delete `parse_ts` (R9 dead-code sweep).
- [ ] Replace `strip_imports_exports` line filtering with OXC's
      `with_typescript(true)` + lowered-AST filtering (or reject the
      unsupported forms with a proper SyntaxError instead of silently
      dropping them).

### U7 — **Med**. `REGEX_CACHE` thread_local lives in `context/mod.rs`

`context/mod.rs:21-25` declares a single-character regex cache
specific to one test262 hot path; never cleared on `reset()`. Belongs
in `builtins/regex.rs` (or its eventual `core/regex.rs`) as a
realm-scoped cache, not as a context-level responsibility. Same
cross-realm leak class as the ~12 proto thread-locals catalogued in
T9.

- [ ] Move to `builtins/regex.rs`; clear on `Context::reset` (with R6
      the cache becomes a `Realm` field).
- [ ] **Repro test:** create a context, allocate a single-char regex,
      `reset()`, then verify the cached regex's `lastIndex` and
      prototype come from the new realm, not the old one.

### U8 — **Low-Med**. `strip_imports_exports` fragile line-prefix stripping

Covered under U6 — `parser.rs:91-106`. Filter on `import `, `export `,
`import =`, `export =`, `export {` only at line-start (after trim).
Inside template literals / multi-line strings containing those tokens,
stripping is silent and wrong. Folded into the U6 fix.

---

## Messy parts

### M1 — **High**. Dual error-state model: `THROWN_VALUE` thread_local + `Result<_, JsError>`

Already D7. Ranked here as the messiness surface: 79 sites touching
`set_thrown_value`/`get_thrown_value`/`take_thrown_value`; `eval_impl`
peek-doesn't-take fallback path on producer omission sinks to
`"unknown eval error"`. Two redundant states drift silently. R8's
`throw_type_error` collapses to one channel.

### M2 — **Med**. `set_thrown_value` / `get_thrown_value` / `take_thrown_value` lifecycle is ambiguous

Three accessors for one thread_local. `assert.throws`
(`test262/harness/assert_helpers.rs:95, 102, 130`) uses
`get_thrown_value` (peek), then *sometimes* re-`set_thrown_value` to
restore; `eval_impl:112` peeks; `eval_try_catch` calls
`take_thrown_value` (consume). No documented protocol. Pre-R8 fix:
define "producers write, consumers `take`" — peeks without ownership
transfer are a bug pattern.

- [ ] Land with R8 / T6: `JsError` carries the Error Value as a field;
      `THROWN_VALUE` deleted; `take`/`get`/`set_*` removed.

### M3 — **Med**. `CURRENT_CONTEXT` open-coded save/restore with one known hole

Already T8. Ranked here as messy: hand-rolled `prev = *borrow(); *borrow_mut() =
Some(...); ... *borrow_mut() = prev` on three paths
(`context/mod.rs:62-68, 73-75, 104-106`), and the `?` at line 82
skips the line-104 restore — verified. R10 RAII `CtxGuard { prev }
impl Drop` removes the failure-mode class entirely.

### M4 — **Med**. Mixed throw-message conventions

`JsError("Maximum call stack size exceeded")`, `JsError("not a function")`,
`JsError("Invalid array length")`, `JsError("Delete should be handled specially")`
— string-form with no error-type prefix. Meanwhile `JsError("TypeError: Value
is not iterable")` (iteration.rs:20), `JsError("ReferenceError: super is only
valid in class methods")` (call.rs:75), `JsError(format!("SyntaxError:
Unexpected strict mode reserved word: {}", name))` (parser.rs:37) carry the
type as a prefix string. The downstream consumer (`eval/operators.rs:82-124`,
`eval_impl:111-117`) parses the prefix out by string match. Replace with
typed constructors per D7.

### M5 — **Med**. `StringInterner` (92 LOC) bypassed by `Key::Str(Rc<str>)`

`interner.rs` exists and `Context` carries `string_interner:
StringInterner`, but property keys live in `Key::Str(Rc<str>)`
(`value/object.rs:23`) and `own_keys()` returns `Vec<String>`
(`value/object.rs:1293`); the interner is never threaded through the
property hot path. Either:
- Wire the interner into `Key` (`Key::Sym(Rc<Symbol>)` already uses
  `Rc`; `Key::Str` should similarly be cheaply-clonable, whether or
  not the interner produces it), OR
- Delete `StringInterner` (R9 dead-code sweep).

[ ] Decision: if the interner isn't paying for itself, delete it. If
    keeping it, the `Key::Str(Rc<str>)` path must thread through it.

### M6 — **Med**. `eval/object.rs` is 1848 lines because `mod tests` lives inline

The file's `mod tests` block runs from line 1143 to the end — ~700
lines of 13 `debug_*` crates-driven end-to-end tests
(`debug_new_target_arrow_2`, `debug_typeof_undeclared`, `debug_assign_to_global`,
…). These belong in `crates/quench-runtime/tests/` next to the
existing stage tests; the `src/eval/object.rs` should carry only
focused `#[test]`s for *specific ops*, mirroring the pattern in
`src/eval/string_methods.rs`.

- [ ] Move `eval/object.rs` `mod tests` debug_* crates-driven tests to
      `crates/quench-runtime/tests/object_ops.rs`; keep focused unit
      tests next to the code.
- [ ] Helps R12: bringing `eval/object.rs` under 500 is feasible once
      the tests relocate.

### M7 — **Low**. Dead helpers in `eval/class.rs`

```
eval/class.rs:17   fn class_static_field_this_name() { let _ = 42; }   // #[allow(dead_code)]
eval/class.rs:90  fn infer_class_name_from_env(_env) -> Option<String> { None }
                   // #[allow(dead_code)]
```

Both empty stubs retained under `#[allow(dead_code)]`. `AGENTS.md`
treats this as a "TODO(delete)" marker. Edit and remove in the same
diff or never.

- [ ] Delete both; verify `cargo test` / `cargo clippy --all-targets`
      clean (one would expect zero callers as they're already dead).

### M8 — **Med**. 10+ cross-cutting `thread_local`s resolving global call-frame state

`interpreter.rs:24-130` enumerates: `CONTROL_FLOW`, `CURRENT_EVAL_ENV`,
`DIRECT_EVAL`, `CURRENT_THIS`, `CALL_THIS`, `SUPER_CLASS`, `STRICT_MODE`,
`CURRENT_DEPTH`, plus per-realm proto pointers (~12) and `THROWN_VALUE`
and `CURRENT_CONTEXT` in `context/mod.rs` + `value/error.rs`. Every
runtime state lives in a hidden global rather than an explicit
`CallFrame` struct passed via `&mut`. This makes reasoning about
recursion, async, generator resumption, and realm isolation all
harder (see T8 / T9 / R10).

- [ ] Long-term: introduce a `CallFrame { this_val, super_class,
      strict, depth, eval_env, new_target }` passed through the eval
      functions; thread-locals only carry the *root* call frame.
- [ ] Short-term: move proto pointers onto `Realm` (R6) and RAII
      `CURRENT_CONTEXT` (R10) — these two alone remove the soundness
      holes even without a full `CallFrame` pivot.

### M9 — **Med**. `Object::get(key: &str)` etc. have no `&Key` overload

`value/object.rs:834` `pub fn get(&self, key: &str) -> Option<Value>`
constructs the key from a string every time; symbol-keyed access needs
`set_symbol`/`has_symbol`/`set_symbol_value` as *separate* methods
because the `&str` API can't represent them. After R5 collapses the
storage, the public API should be `get(&self, key: &Key) -> Option<Prop>`
and `define_own_property(&mut self, key: &Key, desc)`, with `&str` and
`Rc<Symbol>` upcast conveniences at the call boundary.

### M10 — **Low-Med**. `PromiseObjectData` lives on `Object`, accessed by its own methods scattered across `builtins/promise/`

`value/object.rs:78-95` defines `PromiseState` + `PromiseObjectData`
(with methods `fulfill`, `reject`, `add_fulfilled_callback`,
`add_rejected_callback`). Used from `builtins/promise/constructor.rs`,
`callbacks.rs`, `helpers.rs`, `static_methods.rs`. The data is on the
`Object` but the methods aren't co-located. After R0 reimplements
promise reactions in JS, `PromiseObjectData` becomes internal Rust
state only — collapse to one file and one accessor surface.

### M11 — **Low**. `is_array_index` uses a non-spec upper bound

`value/object.rs:52-71`: `MAX_ARRAY_ELEMENTS = 1 << 20` (1_048_576)
is the dense-array cutoff. Spec §6.1.7.0: an array index is an
integer `0 ≤ i < 2^32 - 1`. The 1 MiB cutoff is an implementation
choice but is silently imposed on `as_array_index`, so
`o[2_000_000] = 1` stores as a plain property, not an element, and
`o.length` doesn't reflect it. Spec-correct because array indices
above 2^32-2 are also as non-index string keys; but the *current* code
mixes spec-array-index and internal-dense-cutoff into one function,
which is confusing. Splitting:

- `is_canonical_array_index(s) -> bool` — spec definition (≤ 4294967294).
- `as_dense_index(s) -> Option<usize>` — internal cutoff (≤
  `MAX_ARRAY_ELEMENTS`).

[ ] One-line refactor on R5 (storage collapse).

---

## Summary ranking (all D + U + M, plus cross-refs)

| # | Sev | Class | Title | Cross-ref |
|---|-----|-------|-------|-----------|
| D1 | Critical | Dup | `same_value_zero` 3× byte-equivalent | new (ops.rs) |
| D2 | High | Dup | Three `to_object` boxers (one spec-incorrect) | T2 / R7 |
| D3 | High | Dup | Four iterator protocols | T2 / R2 |
| D7 | Med | Dup | Three JsError idioms + dual error model | T6 / R8 |
| D9 | Med | Dup | `Object` API surface 4×/4×/8×/5×/2× | R5 |
| D4 | Med-High | Dup | Three "use strict" detectors | new |
| D5 | Med | Dup | Two arguments-object creators | new |
| D6 | Med | Dup | `call_value` vs `Context::call_js_function` | T13 / R11 |
| D8 | Med | Dup | Four `JsError` constructors | T6 / R8 |
| U1 | Med-High | Complexity | Two object-typing systems (`ObjectKind`/`ObjData`) | new |
| U2 | Med-High | Complexity | `TComp` speculative machinery | T5 / R4 |
| U3 | Med | Complexity | Five parallel property maps | T3 / R5 |
| U4 | Med | Complexity | `force_strict` bool (spec-incorrect) | new |
| U5 | Med | Complexity | `ForOf`/`ForIn` on Expression enum, statement as expression | new |
| U6 | Med | Complexity | 5 parser entrys, 1 dead, 1 byte-identical dup | new |
| U7 | Med | Complexity | `REGEX_CACHE` cross-realm leak | T9 / R6 / new |
| U8 | Low-Med | Complexity | `strip_imports_exports` fragile line filter | folded in U6 |
| M1 | High | Messy | Dual error-state model | D7 / T6 |
| M2 | Med | Messy | `set`/`get`/`take_thrown_value` ambiguous lifecycle | D7 |
| M3 | Med | Messy | `CURRENT_CONTEXT` open-coded save/restore + 1 hole | T8 / R10 |
| M4 | Med | Messy | Throw message convention (prefixed vs un-prefixed) | D7 |
| M5 | Med | Messy | `StringInterner` bypassed by `Rc<str>` path | new |
| M6 | Med | Messy | `eval/object.rs` 1848 lines (inline `mod tests`) | T4 / R12 |
| M7 | Low | Messy | Dead stubs in `eval/class.rs` | T11 / R9 |
| M8 | Med | Messy | 10+ thread-locals = hidden call-frame state | T8+T9 / R10 |
| M9 | Med | Messy | `Object::get(&str)` no `&Key` overload | R5 |
| M10 | Low-Med | Messy | `PromiseObjectData` methods scattered | R0 fold |
| M11 | Low | Messy | `is_array_index` mixes spec + internal | R5 fold |

---

## Sequencing

```
R1 ops.rs       absorbs D1, D2, D3 (the canonical home for the dupes)
R5 collapse     absorbs D9, U3, M9, M11, U1 (object-typing decision)
R8 throw_*_err  absorbs D7, D8, M1, M2, M4
R6 Realm        absorbs U7 (REGEX_CACHE), M8 (thread-local reduction)
R10 RAII guard  absorbs M3
R12 + tests/    absorbs M6
R9 dead-codes   absorbs M7, U6 (parse_ts dead), M5 (StringInterner decision)
R0 self-host    absorbs D5, D6, M10 (JS reimplements)
NEW (after R5)  D4 use-strict singleton
NEW (after R1)  U4 force_strict removal (verify test262 first)
NEW (after R0)  U5 ForOf/ForIn on Statement enum
NEW (after R9)  U6/U8 parser entry consolidation + strip_imports_exports
```

Every item lands with `cargo test -p quench-runtime` +
`cargo clippy -p quench-runtime --all-targets` clean and the current
test262 stage (`tasks/index.json`) not regressing. `tests/test262.rs`
and `tests/test262/` are never edited.