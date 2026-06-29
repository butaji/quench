# Task 03: Add missing language features to the interpreter

## Goal

Add the JS language features required by `runtime.js` and compiled TSX that the current interpreter does not yet support.

## Files

- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`

## Done

- `for...of` and `for...in` loops (with identifier and destructuring bindings).
- Nullish coalescing (`??`).
- Optional chaining for member and call (`obj?.prop`, `obj?.[expr]`, `obj?.()`).
- Computed property access (`obj[expr]`).
- Rest parameters (`function(a, ...rest)`).
- Template literal expressions (`` `a ${b}` ``).
- `arguments` object in non-arrow function calls.
- `in` and `instanceof` binary operators.

## Still to do

- Spread in function calls (`cb(...args)`) and array/object literals (`[...arr]`, `{...obj}`).
- Getter/setter invocation on property access and assignment.
- `typeof` on undeclared identifiers (`typeof notDeclared` must return `"undefined"`).

## Steps

1. Implement spread expansion in `eval_call` and array/object literal construction.
2. Implement getter invocation in member access and setter invocation in assignment.
3. Make `typeof` return `"undefined"` for undeclared identifiers instead of throwing `ReferenceError`.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable.

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
