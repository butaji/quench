# Task 03: Add missing language features to the interpreter

**Status: COMPLETED** - All targeted language features implemented.

## Goal

Implement the JavaScript language features needed for Ink and runtime.js.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


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
- ✅ `break`/`continue` handling in loops (with distinct JsError markers)
- ✅ `==` loose equality implements abstract equality comparison
- ✅ `instanceof` correctly walks prototype chain

## Deferred features

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
