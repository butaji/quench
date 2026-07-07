# Runtime Optimization Roadmap

This document captures the findings from the architecture / code / tasks review. The goal is to move toward the **best possible runtime with less code and better performance**, aligned with the typed HIR design in `docs/hir-design.md`.

## Review summary

The current runtime is a recursive AST walker backed by `HashMap<String, Value>` for scopes and object properties. That matches the original prototype but is directly at odds with the HIR plan (resolved slots, shapes, trampoline, arena allocation, unified object model). The good news: many of the highest-impact improvements are small, incremental changes that can be made before the HIR rewrite lands.

## Optimization principles

1. **Less hot-path allocation.** Avoid cloning function bodies, array elements, and strings on every access.
2. **No HashMap in the hot path.** Scopes and object properties should be index-based (slots / shapes).
3. **Explicit state.** Replace thread-local control flow and `this` with explicit arguments and return values.
4. **Unified value model.** Functions, native functions, constructors, and objects should share one representation.
5. **One correctness fix, one focused test.** Every change below must have a regression test.

## Ranked recommendations

### P0 — Critical correctness / performance blockers

| # | Issue | Files | Fix | Effort | Impact |
|---|-------|-------|-----|--------|--------|
| 1 | `while` loops hard-capped at 10 iterations | `interpreter.rs` | Remove the arbitrary 10-iteration cap in the `while` evaluator. Do not add an instruction budget unless a later task requires it. | trivial | very high |
| 2 | Function bodies cloned on every `Value` clone | `value.rs`, `interpreter.rs` | Store bodies as `Rc<[Statement]>` | low | high |
| 3 | Recursive interpreter + global atomic depth counter | `interpreter.rs` | Migrate to trampoline/`CallFrame` via Task 85. Make the recursion depth thread-local via Task 338. | high | very high |
| 4 | Variable lookup walks `HashMap<String, Value>` scopes | `env.rs`, `interpreter.rs` | Intern identifiers; use `Vec`-based locals / upvalues (HIR §9) | medium | very high |
| 5 | Object properties stored in `HashMap<String, Value>` | `value.rs` | Intern property keys with `lasso`. Implement shapes + slot arrays only after Task 85 and value-model unification are closed. | medium | very high |

### P1 — Significant performance / design issues

| # | Issue | Files | Fix | Effort | Impact |
|---|-------|-------|-----|--------|--------|
| 6 | String primitives allocate a closure on every property access | `interpreter.rs` | Install `String.prototype` once with shared `NativeFunction`s | low | high |
| 7 | Array builtins clone the entire `elements` vector | `builtins/array.rs` | Operate on `&mut Vec<Value>` directly | low | high |
| 8 | Closures capture whole environment chain | `value.rs`, `interpreter.rs` | Compute captured variables and store `Rc<[UpvalueRef]>` array | medium | high |
| 9 | Value model split into `Function`/`NativeFunction`/`NativeConstructor` | `value.rs`, builtins | Collapse into `Value::Object` with `[[Call]]`/`[[Construct]]` slots | medium | high |
| 10 | Control flow and `this` use thread-local hidden state | `interpreter.rs` | Return `ControlFlow` enum; pass `this` explicitly | medium | medium-high |
| 11 | `Context` maintains redundant `globals` `HashMap` | `lib.rs` | Use top-level `Environment` as global object | low | medium |
| 12 | Thrown values converted to strings | `interpreter.rs` | Make `JsError` carry the original `Value` (Task 250) | low | medium |
| 16 | Host bridge serializes every call to/from JSON | `src/main.rs`, `src/bridge/ffi.rs` | Use `__ink_call_fast`/method IDs for hot paths | medium | medium-high |
| 17 | Timer IDs parsed from JSON string | `src/event_loop.rs` | Return `Vec<u32>` directly from timer bridge | low | medium |
| 18 | Hot reload creates a new context but does not replace the running one | `src/event_loop.rs` | Assign `*ctx = new_ctx` in place | low | medium |
| 19 | Relational ops skip `ToPrimitive` | `interpreter.rs` | Reuse `to_primitive` in comparisons | medium | medium |

### P2 — Complexity hotspots

| # | Issue | Files | Fix | Effort | Impact |
|---|-------|-------|-----|--------|--------|
| 13 | Method-call binding duplicated | `interpreter.rs` | Single `get_property(obj, key) -> (value, this_binding)` helper | medium | medium |
| 14 | Getters detected by function-name prefix | `interpreter.rs` | Implement real property descriptors with `get`/`set` slots in `Object`; remove prefix detection. | medium | medium |
| 15 | `lower.rs` expands destructuring into bloated AST | `lower.rs` | Interpret destructuring patterns directly in `interpreter.rs`; move to HIR ops only after Task 85 lands. | medium | medium |

### P3 — Minor / cleanup

| # | Issue | Files | Fix | Effort | Impact |
|---|-------|-------|-----|--------|--------|
| 20 | Source re-parsed on every `Context::eval` | `lib.rs`, `swc_parse.rs` | Cache parsed `Program`s in a `HashMap<String, Rc<Program>>` keyed by source text. | medium | medium |
| 21 | test262 runner is single-threaded | `test262/runner.rs` | Parallelize with `rayon` | low | medium |
| 22 | Array indices also stored as string keys | `value.rs` | Treat arrays as dense `elements` only | low | medium |
| 23 | Dead/duplicated conversion helpers | `src/main.rs`, `builtins/object.rs` | Re-export `value::to_js_string`/`to_number`; fix formatting | trivial | low |

## Immediate quick wins (do first)

These are the smallest patches with the biggest payoff. They should be completed before larger HIR work.

1. **Task 281** — Remove the `while` loop 10-iteration cap.
2. **Task 282** — Share function bodies via `Rc<[Statement]>`.
3. **Task 283** — Install `String.prototype` once instead of per-access closures.
4. **Task 284** — Avoid cloning array elements in builtin methods.
5. **Task 285** — Make control flow and `this` explicit (remove thread-locals).
6. **Task 286** — Remove redundant `Context::globals` map.

## Alignment with HIR

Every P0/P1 recommendation above moves the current runtime closer to the HIR design:

- Resolved bindings and slots → HIR §5, §9
- Shapes and slot arrays → HIR §11.1–11.2
- Unified function object → HIR §11.3
- Trampoline / explicit frames → HIR §10
- Explicit control flow / exceptions → HIR §8
- No hot-path `HashMap` → HIR §15 efficiency notes

## Tracking

See `tasks/index.json` for the optimization tasks. Each task includes the required regression tests and JS/TS scenario tests where applicable.
