# Task 03: Add missing language features to the interpreter

**Status: COMPLETED** - All acceptance criteria met.

## Goal

Implement the JavaScript language features needed for Ink and runtime.js.

## Files

- `crates/quench-runtime/src/interpreter/eval_expr/main.rs`
- `crates/quench-runtime/src/interpreter/eval_expr/helpers.rs`
- `crates/quench-runtime/src/interpreter/eval_stmt/mod.rs`
- `crates/quench-runtime/src/interpreter/call.rs`
- `crates/quench-runtime/src/interpreter/binary_ops.rs`

## ✅ Completed

- ✅ Spread operator in function calls and array literals
- ✅ Getter/setter invocation in member access
- ✅ `typeof` on undeclared identifiers returns `undefined` (not error)
- ✅ `for...of` loops over arrays
- ✅ `for...in` loops
- ✅ Nullish coalescing (`??`)
- ✅ Template literals with expressions
- ✅ `arguments` object in JS-to-JS function calls
- ✅ Arrow function rest parameters bound correctly
- ✅ Optional chaining via lowering (produces conditional expression)
- ✅ Destructuring parameters via lowering

## Still missing (deferred)

- ❌ `yield` / generators - deferred to Task 19.
- ❌ `async`/`await` - deferred to Task 19.
- ❌ `class` syntax - deferred to Task 18.
- ❌ `super` keyword - deferred to Task 18.

## Acceptance criteria

- ✅ `runtime.js` createElement works (reads `arguments`)
- ✅ `config.platform?.os` evaluates safely
- ✅ `Object.entries(config).map(([k, v]) => ...)` works
- ✅ Arrow rest parameters work: `(...args) => args`

## Verification

```bash
cargo test -p quench-runtime
cargo test
```
