# Task 18: Pass TypeScript function and class conformance tests

## Goal

Make the runtime pass all runtime-relevant function and class conformance cases.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Files

- `crates/quench-runtime/src/lower/decl.rs`
- `crates/quench-runtime/src/lower/patterns.rs`
- `crates/quench-runtime/src/lower/expr.rs`
- `crates/quench-runtime/src/interpreter/call.rs`
- `crates/quench-runtime/src/interpreter/eval_expr/main.rs`
- `crates/quench-runtime/src/interpreter/eval_stmt/mod.rs`
- `crates/quench-runtime/src/value/function.rs`

## Steps

1. From the Task 16 audit, pick the function and class failures.
2. Implement or fix the missing features, which are likely to include:
   - default parameters
   - destructuring parameters (array and object patterns in function/arrow signatures)
   - rest parameters in arrow functions
   - `arguments` object in ordinary JS-to-JS calls
   - `Function.prototype.bind`
   - class declarations and expressions
   - `constructor`, `super()`, `super.method()`
   - static members
   - getters/setters on classes (not just object literals)
3. Add unit tests for each fixed feature.
4. Re-run the function/class conformance subset.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- Do not modify `tests/typescript/` or `examples/`.

## Acceptance criteria

- All function/class conformance cases that are in scope pass.
- `class Foo extends Bar { ... }` and `new Foo()` work.
- `function f({x}, ...rest) {}` binds patterns and rest correctly.

## Verification

```bash
cargo test -p quench-runtime --test conformance -- functions classes
```
