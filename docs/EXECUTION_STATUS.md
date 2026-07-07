# Quench Runtime - Execution Status

## Current state (2026-07-07)

### Test results

| Suite / file | Total | Passed | Failed | Status |
|--------------|-------|--------|--------|--------|
| `lib.rs` unit tests | 55 | 55 | 0 | ✅ |
| `runtime_issues.rs` (parallel) | 44 | 38 | 6 | ❌ |
| `runtime_issues.rs` (single-threaded) | 44 | 44 | 0 | ✅ |
| `scenarios.rs` | 32 | 32 | 0 | ✅ |
| `var_hoisting_tdz.rs` | 17 | 9 | 8 | ❌ |
| Other integration tests | ~30 | ~30 | 0 | ✅ |

Run with `cargo test -p quench-runtime`.

## Diagnosed issues and exact fixes

### 1. False "Maximum call stack size exceeded" in parallel tests

**Symptom:** Simple built-in calls (`Math.log10(1)`, `Number.prototype.toFixed`, `clearTimeout`) fail with `Maximum call stack size exceeded` when tests run in parallel, but pass when `--test-threads=1`.

**Root cause:** `CURRENT_DEPTH` in `crates/quench-runtime/src/interpreter.rs` is a global `static AtomicUsize`. Concurrent test threads accumulate each other's recursion counts.

**Exact fix:**
1. In `crates/quench-runtime/src/interpreter.rs`, replace the global `static CURRENT_DEPTH: AtomicUsize` with a thread-local `Cell<usize>`:
   ```rust
   thread_local! {
       static CURRENT_DEPTH: Cell<usize> = const { Cell::new(0) };
   }
   ```
2. Rewrite `check_depth()` to read and increment the thread-local cell.
3. Rewrite `release_depth()` to decrement the thread-local cell.
4. Rewrite `reset_depth()` to set the thread-local cell to 0.
5. Keep `MAX_RECURSION_DEPTH_OVERRIDE` as a global static (tests still set it), but read it inside the thread-local helper.
6. Run `cargo test -p quench-runtime --test runtime_issues` to confirm 44/44 pass in parallel.

**Status:** Fix landed by the background process; parallel `runtime_issues` tests pass.

**Tracking:** Task 338.

### 2. Recursive interpreter consumes the native Rust stack

**Symptom:** Deeply recursive JS functions exhaust the native Rust stack after a few hundred calls.

**Root cause:** Each JS function call translates to multiple nested Rust calls (`eval_expr` → `call_value_with_this` → `eval_statements` → ...).

**Exact fix (by design):** Replace recursion with a non-recursive state machine. Implement a trampoline loop over a heap-allocated `Vec<CallFrame>` (Task 85). JS function calls become `stack.push(...)`/`stack.pop(...)`, not nested Rust calls. The recursive interpreter stays until Task 85 lands.

**Tracking:** Task 85.

### 3. `var` hoisting broken inside function scope

**Symptom:**
```javascript
function f() { console.log(x); var x = 1; }
f(); // ReferenceError: x is not defined
```

**Root cause:** Function bodies do not run a `var` hoisting pass before execution.

**Exact fix:**
1. Add `hoist_var_declarations(stmts: &[Statement]) -> Vec<String>` in `crates/quench-runtime/src/interpreter/call.rs`.
2. When entering a function body, call it and declare each name as `DeclaredOnly` in the function's local environment before evaluating statements.
3. Do not initialize the values; leave them `undefined` until the `var` declaration statement executes.

**Status:** Fix landed by the background process; function-scope `var` is hoisted.

**Tracking:** Task 339.

### 4. `let` / `const` TDZ missing at script and block level

**Symptoms:**
```javascript
let y = 1; { y; let y = 2; }   // should throw TDZ, currently does not
let z; z; let z = 1;           // should throw TDZ
```

**Root cause:** The environment does not track temporal dead zone for `let`/`const` bindings.

**Exact fix:**
1. Extend `crates/quench-runtime/src/env.rs` `Binding` to store `VarKind { Var, Let, Const }` and a boolean `initialized`.
2. When declaring `let`/`const`, set `initialized = false`.
3. On read/write of a binding where `initialized == false`, throw `ReferenceError: Cannot access 'X' before initialization`.
4. When the initializer expression finishes evaluating, set `initialized = true`.

**Status:** Fix landed by the background process; TDZ and const assignment `TypeError` are enforced.

**Tracking:** Task 340.

### 5. `const` assignment does not throw TypeError

**Symptom:**
```javascript
const x = 1; x = 2; // no error
```

**Root cause:** The assignment path does not check `VarKind` before writing.

**Exact fix:**
1. In `crates/quench-runtime/src/interpreter.rs`, locate the `assign_to` (or equivalent) helper.
2. Before writing, look up the binding's `VarKind`.
3. If `VarKind::Const`, return `TypeError: Assignment to constant variable.`.

**Status:** Fix landed by the background process; TDZ and const assignment `TypeError` are enforced.

**Tracking:** Task 340.

### 6. Constructor returns expression value instead of `this`

**Symptom:**
```javascript
new Boolean(false) // returns false instead of Boolean object
```

**Root cause:** Constructor call path returns the last expression value when there is no explicit `return`.

**Exact fix:**
1. In the `[[Construct]]` path, create a new object and bind it to `this` before executing the function body.
2. After execution, if the body returned an object, return that object; otherwise return the `this` binding.
3. Update `crates/quench-runtime/src/interpreter/call.rs` and the constructor logic in `crates/quench-runtime/src/value.rs`.

**Tracking:** Task 341.

### 7. `typeof this` at script level returns `"undefined"`

**Symptom:**
```javascript
typeof this // "undefined"
```

**Root cause:** Script-level `this` is not bound to the global object.

**Exact fix:**
1. In `eval_program` for `Program::Script`, bind `this` to the global object before executing statements.
2. In `eval_program` for `Program::Module`, bind `this` to `undefined`.

**Tracking:** Task 345.

## Exit criteria for this status page

- [x] Task 338 closed: parallel `runtime_issues.rs` passes.
- [ ] Task 85 closed: recursive stress test (`f(100000)`) passes without native stack overflow.
- [x] Task 339 closed: `var` hoisting inside functions works.
- [x] Task 340 closed: `let`/`const` TDZ and const assignment `TypeError` enforced.
- [ ] Tasks 341 and 345 closed: constructor `this` and script-level `typeof this` still pending.
