# Task 03: Add missing language features to the interpreter

## Goal

Add the JS language features required by `runtime.js` and compiled TSX that the current interpreter does not yet support.

## Files

- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`

## Required features

1. **`for...of` loops** — iterate over arrays, Map, Set, and other iterables.
2. **`for...in` loops** — iterate over enumerable property keys of an object.
3. **Nullish coalescing (`??`)** — return left operand if it is not `null`/`undefined`, otherwise right.
4. **Optional chaining (`?.`)** — lowered to a null-check + access; the interpreter may not need direct support if lowering handles it.
5. **Rest parameters** — `function(cb, ...args)` binds `args` to the trailing arguments.
6. **Spread in function calls** — `cb(...args)` expands an iterable into positional arguments.
7. **Template literal expressions** — `` `Count: ${count}` `` evaluates to the concatenated string.
8. **Getters and setters** — property access invokes getters; assignment invokes setters.
9. **`arguments` object** — every non-arrow function has a local `arguments` array-like object (used heavily by the console polyfill and `Fragment`).
10. **`typeof` for undeclared identifiers** — currently throws `ReferenceError`; `typeof foo` should return `"undefined"` when `foo` is undeclared.

## Steps

1. Add AST nodes for `ForOf` and `ForIn` and lower them from swc.
2. Add `BinaryOp::NullishCoalescing` and implement it in `eval_binary_op`.
3. Ensure optional chaining is lowered in Task 01; here, verify the generated conditional evaluates correctly.
4. In function calls, bind a rest parameter to the remaining arguments and support spread calls.
5. In `eval_program`/`eval_statements`, for every function call, declare an `arguments` local that is an array-like object containing the actual arguments.
6. Implement getter/setter invocation in member access and assignment.
7. Make `typeof` handle undeclared identifiers gracefully.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable.

## Acceptance criteria

- `for (const x of [1,2,3])` sums to `6`.
- `for (const k in {a:1,b:2})` collects keys `a` and `b`.
- `null ?? 'fallback'` returns `'fallback'`; `0 ?? 'fallback'` returns `0`.
- `function f(a, ...rest) { return rest; } f(1,2,3)` returns `[2,3]`.
- `function g() { return arguments.length; } g(1,2)` returns `2`.
- `({ get x() { return 42; } }).x` returns `42`.
- `typeof notDeclared` returns `"undefined"` without throwing.

## Verification

```bash
cargo test -p quench-runtime interpreter
```
