# Code Audit 2026-07-25

Read-only scan of all production code in `crates/quench-runtime/src/`
(160 `.rs` files). Findings are categorized by test262 impact and
ranked by severity within each category.

## Spec-violation bugs (block test262 stages)

These are correctness bugs that cause test262 failures or will cause
failures as the relevant stages are reached.

### B1. `Array.prototype.reduce` — accumulator never passed to callback

**`src/builtins/array/methods/transformation.rs:102-127`**

`call_callback` passes `elem` as the first argument instead of the
accumulator. The spec says `callback(accumulator, element, index, array)`.
`[1,2,3].reduce((a,b) => a+b)` returns `NaN` because `a` is always the
current element, not the running total.

### B2. `await` discarded during lowering

**`src/lower/expr/helpers.rs:32`**

`AwaitExpression` is lowered as its argument directly — the `await`
semantics are completely lost. The runtime AST may never see an
`Expression::Await` variant. This blocks every async-function stage.

### B3. `get_setter_func` returns different function object each call

**`src/value/object/accessor.rs:163-181`**

When a setter has `s.func == None` but `s.body` is non-empty, a new
`ValueFunction` is created dynamically on every call. This means
`Object.getOwnPropertyDescriptor(obj, key).set === Object.getOwnPropertyDescriptor(obj, key).set`
returns `false`. Violates the spec requirement that the same descriptor
returns the same function object.

### B4. `JSON.stringify` — functions/symbols serialized as `"null"` instead of skipped

**`src/builtins/json.rs:39-49`**

The `_ => Some("null".to_string())` catch-all maps `Value::Function`,
`Value::NativeFunction`, `Value::NativeConstructor`, `Value::Class`,
`Value::Generator`, and `Value::Symbol` to `"null"`. Per spec, functions
and symbols in objects should be **skipped** (not serialized), and
`Symbol` values should be excluded entirely.

### B5. `hex_float` uses `10.0_f64.powf` instead of `2.0_f64.powf`

**`src/builtins/date/helpers.rs:176`**

`0x1.2p3` uses a binary exponent (`p` = ×2^). Using `10.0_f64.powf`
means `parseFloat('0x1p3')` returns `10.0` instead of `8.0`.

### B6. `String.prototype.substring` / `slice` — byte-based indexing for char iteration

**`src/builtins/string/methods/slice.rs:21, 43-46`**

`start` and `end` are computed as byte offsets (`to_number(v) as usize`),
but `s.chars().skip(start)` skips chars, not bytes. For multibyte UTF-8,
this skips too many characters. The spec says these operate on UTF-16
code unit indices.

### B7. `String.prototype.at` — uses `chars().count()` instead of UTF-16 code unit count

**`src/builtins/string/methods/at.rs:20`**

`s.chars().count()` counts Unicode scalar values, not UTF-16 code units.
For supplementary characters (emoji, etc.), `chars().count()` differs
from `s.encode_utf16().count()`. Compare with `string_length_impl` in
`basic.rs:9` which correctly uses `encode_utf16().count()`.

### B8. `Array.prototype.groupBy` — wrong `this` and callback arguments

**`src/builtins/array/methods/grouping.rs:8-15, 31-36`**

`get_this_array` extracts the first argument instead of `this`. The
callback receives a single array argument `[elem, index, array]` instead
of three separate arguments `(elem, index, array)`. Same issue in
`groupByToMap` (line 72-77).

### B9. `Promise.resolve` — doesn't unwrap thenables

**`src/builtins/promise/promise_all.rs:14-38`**

Only checks `ObjectKind::Promise` — if the value is a thenable (has
`.then`), it should be adopted, not wrapped in a new fulfilled promise.
The `settle_resolve` in `callbacks.rs` does handle thenable unwrapping,
but `promise_resolve_impl_static` bypasses it.

### B10. Object rest pattern captures full object, not remaining properties

**`src/lower/pattern.rs:460-468`**

`let {a, ...rest} = obj` — `rest` is initialized to the full source
object, not the object with `a` excluded. The destructured properties
are not removed from the rest object.

### B11. `OptionalChain` — `PrivateFieldExpression` and `CallExpression` ignore `optional` flag

**`src/lower/opt_chain.rs:143-175, 102-142`**

`PrivateFieldExpression` and `CallExpression` branches always wrap in
nullish checks, even when `optional` is `false`. This means
`obj.#field` (non-optional) undergoes unnecessary nullish checking.

### B12. `to_object` doesn't throw for `null`/`undefined`

**`src/value/primitive.rs:244-246`**

Per ES spec, `ToObject(undefined)` and `ToObject(null)` should throw
TypeError. The current code returns a plain empty object, with a test
comment saying this is intentional. Will cause test262 failures in the
`ToObject` stage.

### B13. `to_primitive_object` — missing TypeError when exactly one method exists and returns an object

**`src/value/primitive.rs:162-171`**

TypeError is only thrown when both `valueOf` and `toString` exist and
both return objects, or when neither exists. The case where exactly one
exists and returns an object should also throw, per spec.

### B14. Double evaluation of `right` in `eval_expression` Assignment

**`src/eval/expression.rs:157-284`**

When the identifier is not found in any scope (line 269), `right` is
re-evaluated at line 272 after already being evaluated at line 173.
Side effects in `right` happen twice. Same issue in `CompoundAssignment`
(lines 327-328).

### B15. `eval_for_in` — head lexical scope popped before iterable evaluation

**`src/eval/iteration.rs:580-590`**

The head lexical scope (with TDZ-declared bindings) is popped before the
iterable expression is evaluated. This means the iterable expression can
access the for-in variable, which should be in TDZ.

### B16. Generator `throw()` doesn't propagate error into generator body

**`src/value/generator.rs:326-335`**

Marks the generator as `Completed` immediately and returns an error,
without implementing the correct semantics where the error should be
thrown into the yield point, allowing `try/catch` to handle it.

### B17. `register_error_constructor` — unknown error types overwrite `ERROR_PROTOTYPE`

**`src/value/error.rs:144-148`**

Only `"TypeError"` gets special handling. Registering `SyntaxError`,
`RangeError`, etc. falls through to `_ => ERROR_PROTOTYPE.with(...)`,
overwriting the `Error` prototype. This breaks subsequent `Error`
construction.

### B18. `JSON.parse` — out-of-range numbers silently become `0.0`

**`src/builtins/json.rs:219`**

`n.as_f64().unwrap_or(0.0)` — if a JSON number doesn't fit in `f64`,
it silently returns `0.0` instead of throwing `SyntaxError` per spec.

### B19. `CURRENT_SOURCE` not set in `eval_typescript` / `eval_es_module`

**`src/context/mod.rs:117-184`**

`eval()` sets `CURRENT_SOURCE` for `function.source_text` capture, but
`eval_typescript()` and `eval_es_module()` do not. Functions defined
through these paths will have broken `Function.prototype.toString()`.

---

## Unsoundness / potential panics

### U1. `transmute` lifetime extension in `CURRENT_SOURCE`

**`src/context/mod.rs:85`**

`source` is `&str` with caller lifetime; `transmute` forcibly casts to
`&'static str`. If a panic occurs before the cleanup on line 92, the
thread-local holds a dangling reference.

### U2. `CURRENT_CONTEXT` dangling pointer on panic

**`src/context/mod.rs:44-55, 78-101`**

`CURRENT_CONTEXT` is set to point to a stack-local `ctx`. If
`init_builtins` (line 50) or any eval path panics, the thread-local
retains a dangling pointer. No RAII guard.

### U3. `wtf8_for_of_iterate` — `from_utf8_unchecked` on arbitrary byte slices

**`src/value/wtf8.rs:144-146`**

Assumes `bytes[i..end]` is always valid UTF-8. If the input contains
lone surrogates mixed with invalid bytes, this produces invalid UTF-8
`String`, violating Rust's safety guarantee.

### U4. `error.rs` — bare pointer dereference

**`src/value/error.rs:288`**

`let ctx = unsafe { &*p }` — if `CURRENT_CONTEXT` is accessed outside
eval (e.g., during initialization), this dereferences a dangling pointer.

### U5. `Rc::get_mut().unwrap()` — fragile

**`src/builtins/typed_array.rs:90-92`**

If `typed_array_ctor_rc` is cloned elsewhere before this call, the
`unwrap()` panics. Currently safe by code order, but fragile.

---

## Dead code (violates AGENTS.md "dead code is a bug")

### D1. `#[allow(dead_code)]` markers — total count now ≥ 41

Previously counted 35 (2026-07-24). New findings:

| File | Line | Symbol |
|------|------|--------|
| `eval/class.rs` | 19-22 | `class_static_field_this_name()` |
| `eval/class.rs` | 265-268 | `infer_class_name_from_env()` |
| `eval/operators.rs` | 364-381 | `get_prototype_from_class_val()` |
| `eval/statement.rs` | 130-133 | `acc_stack_top()` |
| `eval/generator.rs` | 67-78 | `yield_value()` |
| `eval/generator.rs` | 83-85 | `yield_delegate()` |
| `lower/expr.rs` | 29-33 | `lower_member_prop` |
| `lower/stmt/mod.rs` | 198-232 | `lower_export_named_local` |
| `lower/stmt/mod.rs` | 234-324 | `lower_export_default_decl_local` |
| `lower/stmt/mod.rs` | 363-377 | `collect_export_from_specs_mod` |
| `lower/stmt/mod.rs` | 379-386 | `module_export_name_to_string_mod` |
| `lower/stmt/exports.rs` | 221-237 | `lower_export_from` |
| `interpreter.rs` | 23 | `ControlFlow` variants |
| `interpreter.rs` | 60-61 | `is_control_flow_set()` |
| `interpreter.rs` | 77-78 | `set_max_call_depth()` |
| `interpreter.rs` | 121-122 | `set_generator_return()` |

### D2. Uncalled functions

- `builtins/intl.rs:9` — `register_intl` never called in `register_builtins`
- `builtins/typed_array.rs:306` — `register_typed_array_iterator` has zero call sites
- `builtins/test_marker` — 4-byte file containing "test", referenced nowhere

### D3. Dead code blocks

- `value/compare.rs:25-28` — `matches!((Undefined, Undefined) | (Null, Null))` in `strict_eq` — both arms have same discriminant, never reachable
- `value/object/helpers.rs:226-233` — `PromiseState::fulfill`/`reject` accept a value but discard it
- `value/object/accessor.rs:106` — `let _ = matches!(func, ...)` computes and discards a bool
- `builtins/promise/promise_all.rs:110-115` — `PromiseAllContext` struct defined but never used as a struct
- `builtins/uri.rs:118` — `let _ = keep_reserved` dead assignment
- `builtins/string/methods/search.rs:160-172` — `match`/`search` installed then overwritten by `regex/string_methods.rs`
- `builtins/string/methods/concat.rs:8-13` — `split` installed then overwritten by `regex/string_methods.rs`

### D4. `#[allow(clippy::complexity)]` violations (3)

- `ast.rs:132` — `has_explicit_return()` — complexity > 10
- `ast.rs:555` — `BinaryOp::precedence()` — complexity > 10
- `ast.rs:613` — `CompoundOp::to_binary()` — complexity > 10

---

## Duplicate code (violates AGENTS.md "zero duplication")

### X1. `parse_number_string` vs `string_to_number` — near-identical parsing

**`value/compare.rs:145-169` vs `value/coerce.rs:253-287`**

Both parse hex/binary/octal/decimal. `parse_number_string` returns
`Option<f64>` and lacks Infinity/NaN handling; `string_to_number` returns
`f64`. One should delegate to the other.

### X2. `NativeFunction` constructors — 5 constructors with identical boilerplate

**`value/function/native_function.rs:44-148`**

`new`, `new_named`, `new_with_name`, `new_with_prototype`,
`new_with_fn_as_prototype` all repeat the same 8 lines. `new_named` and
`new_with_name` are **byte-for-byte identical** (lines 65-100).

### X3. `extract_property_name` — two near-identical implementations

**`eval/call.rs:521-553` and `eval/object.rs:76-105`**

Diverge on `computed` vs non-computed handling. Maintenance hazard.

### X4. `is_anonymous_function_definition` — duplicated

**`eval/class/helpers.rs:719-729` and `eval/object/helpers/destructuring.rs:899-908`**

Identical. Should be hoisted.

### X5. `get_prototype_from_class_val` — three copies

**`eval/operators.rs:364-381` (dead), `eval/class/helpers.rs:465-494`, `eval/member.rs:297-317`**

### X6. `same_value_zero` — three copies

**`builtins/map/helpers.rs:11-16`, `builtins/array/methods/search.rs:64-69`, `builtins/weak/weakset.rs:8-13`**

### X7. `get_this_array` — three divergent copies

**`builtins/array/methods/accessors.rs:14-36`, `transformation.rs:15-36`, `search.rs:6-22`**

The `search.rs` version rejects non-Array array-likes, while the other
two handle them. Inconsistency.

### X8. `make_array` — four identical copies

**`builtins/array/methods/{accessors,transformation,mutation,rearrange}.rs`**

### X9. `lower_prop_name_key` — three implementations with different BigInt handling

**`lower/pattern.rs:274-291`, `lower/expr/helpers_class.rs:131-166`, `lower/stmt/declarations.rs:384-402`**

`declarations.rs` uses `format!("{}n", b.raw)` for BigInt; the other two
use `b.raw.to_string()` — missing the `n` suffix. The `helpers_class.rs`
version also handles `TemplateLiteral` which the others don't.

### X10. Class member lowering — duplicated for `Result` vs `Option` return

**`lower/expr/helpers_class.rs` and `lower/stmt/declarations.rs`**

`lower_class_member`, `lower_constructor`, `lower_method`,
`lower_class_prop` — all exist in two copies, one returning `Result`,
the other returning `Option` (silently dropping errors).

### X11. Export/import lowering — duplicated

**`lower/stmt/exports.rs` and `lower/stmt/mod.rs`**

`lower_export_default_decl`, `lower_export_named`, `lower_export_star_from`,
`lower_import`, `module_export_name_to_string` — all duplicated. The
`exports.rs` version of `lower_export_default_decl` is likely broken
(is missing `exports.default = id` assignment).

### X12. Primitive boxing — three copies

**`eval/member.rs:427-484`, `eval/object/helpers/destructuring.rs:11-58`, `eval/object/helpers/member.rs:84-135`**

Same boilerplate: create boxed object, set prototype, set exotic kind,
set `_value` property.

### X13. `native_fn` helpers — two copies

**`builtins/map/helpers.rs:92-94` and `builtins/weak/registration.rs:16-18`**

---

## Inconsistent / missing error handling

### E1. `JsError::new("TypeError: ...")` vs `create_js_error_with_type`

**Multiple files: `array_buffer.rs`, `data_view.rs`, `weak_ref.rs`, `typed_array.rs`, `function.rs`**

`JsError::new` does not set `__thrown_value` on the context.
`create_js_error_with_type` does. Inconsistent error propagation means
some thrown errors are not catchable via `try/catch`.

### E2. `NativeConstructor::set_property` returns `()` vs `NativeFunction::set_property` returns `Result`

**`value/function/native_constructor.rs:103-105` vs `native_function.rs:186-215`**

Callers can't handle both uniformly. `NativeConstructor` silently
succeeds; `NativeFunction` respects writable flags.

### E3. `Object::set` — strict mode writes silently ignored

**`value/object/property.rs:56-106`**

Doesn't distinguish strict vs sloppy mode. Strict-mode writes to
non-writable / non-extensible targets should throw TypeError.

### E4. `lower` — `Result` vs `Option` split

**Throughout `lower/`**

`lower_expr` returns `Result<Expression, LowerError>`; `lower_stmt`
returns `Option<Statement>`. Many lowering errors are silently discarded.
`lower_script` propagates errors; `lower_module` does not.

### E5. `set_property` results silently ignored

**Multiple files: `array_buffer.rs`, `data_view.rs`, `weak_ref.rs`, `typed_array.rs`, `symbol.rs`**

`let _ = nf.set_property(...)` silently swallows `Result`. In most cases
these are statically known to succeed, but the pattern hides bugs.

### E6. `eval_try` — `take_thrown_value()` without checking

**`eval/statement.rs:1091`**

Uses `unwrap_or(Value::Undefined)` so safe, but if there's no thrown
value, the catch block receives `Value::Undefined` which is wrong.

### E7. `eval_try` — control flow not preserved when rethrowing

**`eval/statement.rs:1121-1139`**

`_pending_cf` captured but never used. Control flow from try body
(break/continue/return) is lost when no catch handler exists and a
finally block runs.

---

## Missing edge cases / incomplete implementations

### M1. Boxed string objects not supported in ~10+ string methods

**`builtins/string/methods/{basic,case,slice,at,pad,search,concat}.rs`**

Most string methods only handle `get_native_this() == Value::String(_)`.
Boxed `String` objects (`new String("hello")`) return `Value::Undefined`.

### M2. `eval_member_access` for `Generator` — no prototype lookup

**`eval/member.rs:87-125`**

For unknown properties, creates a new empty `Object` instead of looking
up on `Generator.prototype`. `Generator.prototype.constructor` and
inherited properties are not accessible.

### M3. Catch parameter only supports simple identifiers

**`lower/control_flow.rs:181-191`**

Destructuring in catch clauses (`catch ({ message })`) silently dropped.

### M4. `eval_for` — no `Yield`/`YieldDelegate` handling from body

**`eval/statement.rs:977-1014`**

If the body contains `yield`, control flow is silently ignored.

### M5. `eval_while` — no `YieldDelegate` handling from condition

**`eval/statement.rs:781-818`**

### M6. `AsyncGenerator` — `return()` skips finally blocks

**`value/generator.rs:359-376`**

`async_generator_return_fn` immediately marks the generator as completed
without executing active finally blocks. Per spec, `return()` should
execute active finally blocks.

### M7. `FROZEN_OBJECTS` — memory leak + potential use-after-free

**`builtins/object_static/freezing.rs:12-15`**

Stores raw `usize` pointers. If a frozen object is garbage collected,
its pointer remains in the vec. A new object could be allocated at the
same address and incorrectly considered frozen.

### M8. `REGEX_CACHE` — unbounded growth, broken UTF-8 key

**`context/helpers.rs:16-18, 352, 372, 390`**

`pat.as_bytes()[0] as char` breaks for multibyte UTF-8. Cache never
cleared, grows without bound.

### M9. `String.prototype` registered twice

**`builtins/string.rs:94-119` and `builtins/date.rs:85-105`**

`string::register_string` creates `String.prototype`, then
`date::register_string_converter` creates another and overwrites.

### M10. Global functions registered twice

**`builtins/date.rs:20-25` and `builtins/uri.rs`**

`parseInt`, `parseFloat`, `isNaN`, `isFinite`, `encodeURIComponent`,
`decodeURIComponent` registered in both `date::register_global_functions`
and `uri::register_uri`.

### M11. `has_explicit_return` — doesn't handle `Statement::Labeled`

**`ast.rs:132-161`**

If `return` is inside a labeled block, reports `false` — incorrectly
says the function has no explicit return.

---

## Linter / style violations

### L1. Functions over 40 lines

- `context/helpers.rs:26-150` — `eval_impl` (~124 lines)
- `context/helpers.rs:300-413` — `register_eval_function` (~113 lines)

### L2. `println!`/`eprintln!` debug output in tests

- `parser.rs:321-396` — 14 `println!` calls in 3 tests
- `context/tests/basic.rs:129-284` — 6 `println!` calls
- `context/tests/accessor_symbol.rs:108-133` — 14 `eprintln!` calls

### L3. Double `#[cfg(test)]` attributes

- `value/convert.rs:22-23`
- `builtins/regex/string_methods.rs:360-361`

### L4. `Default for Context` panics

**`context/mod.rs:293-297`**

`Default` impl should not panic. Callers should use `Context::new()`
explicitly.

---

## Simplification opportunities (LOC reduction)

### S1. `NativeFunction` constructor consolidation

**`value/function/native_function.rs:44-148`**

Extract shared init into a private constructor or `..Default::default()`.
~40 LOC saved.

### S2. Duplicate `same_value_zero` → hoist to `eval/ops.rs`

**3 files → 1 location.** ~10 LOC saved.

### S3. Duplicate `make_array` → hoist to `array/methods/mod.rs`

**4 files → 1 location.** ~20 LOC saved.

### S4. Duplicate `lower_prop_name_key` → unify

**3 files → 1 location.** ~40 LOC saved. Fix BigInt `n` suffix bug in the
process (only `declarations.rs` version is correct).

### S5. Duplicate class member lowering → unify with generic error mode

**6 functions × 2 copies → 6 functions.** ~80 LOC saved.

### S6. Duplicate export/import lowering → unify

**10 functions × 2 copies → 10 functions.** ~150 LOC saved. Fix
`exports.rs` bug in the process.

### S7. Collapse `eval`/`eval_es_module`/`eval_typescript` try-eval-cleanup pattern

**`context/mod.rs:74-184`**

Three methods with identical structure. Extract a private helper.
~50 LOC saved.

### S8. `parse_script`/`parse_es_module`/`parse_jsx` → extract `parse_with(source, SourceType)`

**`parser.rs:15-72`**

~40 LOC saved.

### S9. Remove `#[allow(unused_imports)]` in `val.rs`

**`value/val.rs:89-92`**

The re-exports are already in `mod.rs`. `to_primitive`, `to_bool`,
`to_number`, `strict_eq`, `loose_eq` are mostly unused here. Only
`to_js_string` is used in `Display` impl.

### S10. Intl `toStringTag` → `Symbol.toStringTag`

**`builtins/intl.rs:44, 61, 78`**

Use `"Symbol.toStringTag"` instead of `"toStringTag"` so
`Object.prototype.toString.call()` works correctly.

### S11. `promise_resolve_impl_static` — reuse thenable unwrapping

Route through `settle_resolve` from `callbacks.rs` instead of
duplicating the Promise check.

---

## Summary

| Category | Count | Test262 impact |
|----------|-------|----------------|
| Spec-violation bugs | 19 | Direct — block stages |
| Unsoundness / panics | 5 | Crashes |
| Dead code | 41+ `#[allow]` + 7 blocks + 3 `#[allow(complexity)]` | Maintenance drag |
| Duplicate code | 13 patterns | Maintenance drag; some carry bugs |
| Inconsistent error handling | 7 | Silent failures |
| Missing edge cases | 11 | Future stage failures |
| Linter violations | 4 | Block PRs |
| Simplifications | 11 | ~480 LOC saved |

**Top 5 highest-impact bugs for test262 progress:**

1. **B1** — `Array.prototype.reduce` accumulator (stage ~40)
2. **B2** — `await` discarded (stages ~80+)
3. **B8** — `Array.prototype.groupBy` wrong this (stage ~40)
4. **B14** — Double evaluation of assignment RHS (language stages)
5. **B10** — Object rest captures full object (stage ~25)