# Task 03: Add missing language features to the interpreter

## Goal

Add the JS/TS language features required by `runtime.js` and TSX source that the current interpreter does not yet support.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Files

- `crates/quench-runtime/src/interpreter/`
- `crates/quench-runtime/src/lower/`
- `crates/quench-runtime/src/ast.rs`

## Done ✓

- `for...of` and `for...in` loops (with identifier and destructuring bindings).
- Nullish coalescing (`??`).
- Computed property access (`obj[expr]`).
- Template literal expressions (`` `a ${b}` ``).
- `in` and `instanceof` binary operators.
- **Spread in function calls** (`cb(...args)`).
- **Spread in array/object literals** (`[...arr]`, `{...obj}`).
- **Getter/setter invocation** on property access and assignment.
- **`typeof` on undeclared identifiers** returns `"undefined"` instead of throwing.

## Still missing / caveats

- **Optional chaining** depends on Task 01; it is currently rejected by the lowerer.
- **`arguments` object is not injected by the interpreter's normal JS-to-JS call path** — it is only created by `Context::call_function`.
- **Rest parameters in arrow functions** are ignored by the lowerer.
- **Destructuring function/arrow parameters** are not bound correctly; params are renamed to `"arg"`.

## Acceptance criteria

- `cb(...[1,2,3])` calls `cb` with three arguments.
- `[...[1,2], 3]` evaluates to `[1,2,3]`.
- `({ get x() { return 42; } }).x` returns `42`.
- `({ set x(v) { this._x = v; } }).x = 5` stores `5`.
- `typeof notDeclared` returns `"undefined"` without throwing.

## Verification

```bash
cargo test -p quench-runtime interpreter
```
