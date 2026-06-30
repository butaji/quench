# Task 01: Fix parser and lowering bugs in quench-runtime

**Status: IN_PROGRESS** - Core lowering works; many JS/TS constructs still missing (see notes).

## Goal

Make the swc-based parser/lowering pipeline robust enough to ingest `src/runtime.js` and TSX/JSX source directly, without any pre-compilation step, and produce a clean HIR that is suitable for both interpretation and future AOT compilation.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/src/swc_parse.rs`
- `crates/quench-runtime/src/lower/` (now split into submodules)
- `crates/quench-runtime/src/ast.rs`

## ✅ Completed

- ✅ Computed member property access (`obj[expr]`) lowers correctly.
- ✅ Template literal expressions are interleaved into binary `+` trees.
- ✅ `for...of` and `for...in` loops (including destructuring loop heads) are lowered.
- ✅ `??`, `in`, and `instanceof` binary operators are lowered.
- ✅ Getter/setter properties (`get prop() {}`, `set prop(v) {}`) are lowered correctly.
- ✅ Object and array spread (`{...obj}`, `[...arr]`) is lowered.
- ✅ **Module/script fallback**: `parse_swc` now tries module syntax first if `import`/`export` is present.
- ✅ **lower.rs split into submodules**: `lower/mod.rs`, `lower/decl.rs`, `lower/expr.rs`, `lower/stmt.rs`, `lower/helpers.rs`, `lower/patterns.rs`
- ✅ **Optional chaining** (`obj?.prop`, `obj?.[expr]`, `obj?.()`) is lowered to conditional expressions.
- ✅ **Destructuring assignment** (`[a,b] = arr`, `({x} = obj)`) is lowered.
- ✅ **Rest parameters in arrow functions** are captured and bound correctly.

## Still missing / deferred

- ❌ **Class expressions/statements** - not needed for current Ink examples.
- ❌ **`delete` operator** - not needed for current Ink examples.
- ❌ **Unary `+`** - not needed for current Ink examples.
- ❌ **`yield` / generators** - deferred to Task 19.
- ❌ **`async`/`await`** - deferred to Task 19.

## Acceptance criteria

- ✅ `cargo check -p quench-runtime` and `cargo test -p quench-runtime` pass.
- ✅ `ctx.eval(include_str!("../../../src/runtime.js"))` parses without lowering errors.
- ✅ A snippet using spread, getters/setters, and ES module syntax parses and lowers to HIR.
- ✅ Optional chaining works: `obj?.prop` returns `undefined` if `obj` is `null`/`undefined`.
- ✅ Destructuring assignment works: `[a, b] = arr` binds correctly.

## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
cargo test
cargo run -- --bundle examples/simple.js
```
