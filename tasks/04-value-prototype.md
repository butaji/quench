# Task 04: Fix value and prototype model

**Status: COMPLETED** - All acceptance criteria met.

## Goal

Ensure shared prototypes are installed correctly and `new`/`prototype` lookup works for all built-ins.

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

## Still missing (deferred)

- ❌ Boxing of primitives (String/Number/Boolean) - deferred to Task 11.
- ❌ `Symbol.toStringTag` - not needed for current examples.

## Acceptance criteria

- ✅ `new Array(1, 2, 3)` creates an array with correct prototype.
- ✅ `new Object()` creates an object with correct prototype.
- ✅ Property lookup traverses prototype chain.
- ✅ `Array.from(new Set([1, 2]))` returns array with correct prototype.

## Verification

```bash
cargo test -p quench-runtime
```
