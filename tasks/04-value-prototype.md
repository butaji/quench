# Task 04: Fix value and prototype model

**Status: COMPLETED** - Shared prototypes, instanceof, ==, and prototype lookup all working.

## Goal

Ensure shared prototypes are installed correctly and `new`/`prototype` lookup works for all built-ins.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/src/value/mod.rs`
- `crates/quench-runtime/src/builtins/array.rs`
- `crates/quench-runtime/src/builtins/object.rs`
- `crates/quench-runtime/src/builtins/map.rs`
- `crates/quench-runtime/src/builtins/promise.rs`
- `crates/quench-runtime/src/builtins/function.rs`
- `crates/quench-runtime/src/interpreter/eval_expr/helpers.rs`

## ✅ Completed

- ✅ Shared `Object.prototype` installed and used by all objects
- ✅ `__Object_prototype__` global for interpreter use
- ✅ `Array.prototype` inherits from `Object.prototype`
- ✅ `Map.prototype` and `Set.prototype` inherit from `Object.prototype`
- ✅ `Promise.prototype` inherits from `Object.prototype`
- ✅ `Function.prototype` shared for all function value property lookups
- ✅ `new Array()` and `new Object()` work via `__call` handlers
- ✅ Prototype chain lookup in `Object::get`/`has`
- ✅ `constructor` property on prototypes pointing back to constructors
- ✅ `instanceof` correctly walks prototype chain
- ✅ `==` loose equality implements abstract equality comparison
- ✅ Number primitive member access returns `Number.prototype` methods
- ✅ Array methods return arrays with proper `Array.prototype` for chaining

## Deferred features

- ❌ Boxing constructors (new String/Number/Boolean) - partially implemented for Array/Object, deferred for primitives.
- ❌ `Symbol.toStringTag` - not needed for current examples.

## Acceptance criteria

- ✅ `new Array(1, 2, 3)` creates an array with correct prototype.
- ✅ `new Object()` creates an object with correct prototype.
- ✅ Property lookup traverses prototype chain.
- ✅ `Array.from(new Set([1, 2]))` returns array with correct prototype.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime
```
