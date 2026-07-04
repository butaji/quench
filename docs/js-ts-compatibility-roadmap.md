> Roadmap to full ECMA-262 + TypeScript language compatibility for `.ts/.tsx/.js/.jsx` running natively in Quench. No bytecode, no JIT.

# JS/TS Compatibility Roadmap

## Current baseline

Measured against the current test subsets:

| Suite | Total | Passed | Failed | Skipped | Pass rate (total) | Pass rate (non-skipped) |
|---|---|---|---|---|---|---|
| test262 subset | 431 | 73 | 60 | 298 | **16.9%** | **54.9%** |
| TypeScript expressions subset | 100 | 45 | 55 | 0 | **45.0%** | **45.0%** |
| TypeScript full conformance subset | 100 | 14 | 47 | 39 | **14.0%** | **22.9%** |

Most skipped test262 tests are blocked by stubbed harness helpers (`$262`, `assert.compareArray`, etc.) or unsupported language features (modules, classes, async, generators). Most TypeScript failures are missing runtime syntax/builtins and missing baselines/module resolution.

## Strategy

1. **Unblock measurement first.** Load the real test262 harness, implement the missing assert helpers, and remove the most aggressive skip reasons so we know what actually passes.
2. **Quick syntax/builtin wins.** Fix the small expression/statement gaps and constructor registration that are causing hundreds of TS expression failures.
3. **Core semantics.** Hoisting, TDZ, strict mode, `arguments`, `typeof` undeclared, arrow `this`, global object.
4. **Big features.** Trampoline interpreter, ES modules, classes, Promises/async/generators.
5. **Object model.** Property descriptors, shapes, prototype chains, real built-in prototypes.

## P0 — Blockers / broadest wins

| # | Task | Gap | Why it matters |
|---|------|-----|----------------|
| 1 | 253 / harness | Real test262 harness helpers (`$262`, `assert.*`) | 298 skipped tests are hidden failures; cannot measure real progress without them. |
| 2 | 91 | Audit and shrink test262 skip list | Many skips are outdated and hide real failures. |
| 3 | 289 | Register `Array`, `Error`, `Date` as constructors | `new Array(3)`, `new Error("x")` fail; blocks thousands of built-in tests. |
| 4 | 290 | Complete expression syntax gaps | Template literals, computed keys, spread, `??`, `?.`, `delete`, unary `+`, `for-of` cause 100+ TS expression failures. |
| 5 | 85 | Trampoline interpreter | Recursive AST walker exhausts native stack; required to run whole test262 suites. |
| 6 | 241 | Native ES module loading | `import`/`export` are stripped; blocks module tests and TS moduleResolution baselines. |
| 7 | 182 / 183 | Classes with `super` / `extends` / static fields | No class support blocks class/private-field suites. |
| 8 | 251 | Real Promise + microtasks | Async functions, generators, `await`, and many modern APIs depend on this. |

## P1 — High-impact correctness

| # | Task | Gap | Why it matters |
|---|------|-----|----------------|
| 9 | 291 | `typeof` undeclared returns `"undefined"` | Many `typeof` tests and TS `typeofOperator` failures. |
| 10 | 292 | `var` hoisting + `let`/`const` TDZ | Large swath of variable-scope tests and "x is not defined" failures. |
| 11 | 293 | `arguments` object in functions | Legacy function-argument tests and TS function-call failures. |
| 12 | 141 | Strict mode semantics | Currently skipped `onlyStrict` tests; also changes `this` binding and globals. |
| 13 | 294 | Property descriptors + `defineProperty` | Object semantics, getters/setters, built-in attribute checks. |
| 14 | 295 | Global object as environment record | `var x = 1` ↔ `globalThis.x` parity. |
| 15 | 250 | Preserve thrown values as objects | Negative tests need real error types, not stringified messages. |
| 16 | 283 | Install `String`/`Number`/`Boolean` prototypes once | Primitive method access and many built-in tests. |

## P2 — Medium-impact missing features

| # | Task | Gap |
|---|------|-----|
| 17 | existing | Array `length` accessor, numeric sort, sparse arrays |
| 18 | existing | `String.prototype` UTF-16 methods (`padStart`, `replaceAll`, etc.) |
| 19 | existing | `Date` constructor overloads and formatting |
| 20 | existing | `RegExp`, `BigInt`, `Symbol` real values |
| 21 | existing | `do...while`, `for-in`, `for-of`, `with`, `switch`, labeled break/continue |
| 22 | existing | Destructuring defaults (nested + params) |

## P3 — Host / polish

| # | Task | Gap |
|---|------|-----|
| 23 | existing | Host bridge JSON serialization and timer ID parsing |
| 24 | existing | Hot reload does not replace running context |
| 25 | existing | Parse caching and parallel conformance runner |

## Order of attack (low effort / high impact first)

1. **Measurement:** Tasks 253, 91, 250, 97.
2. **Syntax quick wins:** Task 290 (template literals, computed keys, spread, `??`, `?.`, `delete`, unary `+`, `for-of`).
3. **Built-in constructors:** Task 289 (`Array`, `Error`, `Date` as constructors).
4. **Core semantics quick wins:** Tasks 291 (`typeof`), 292 (hoisting/TDZ), 293 (`arguments`), 141 (strict mode), 283 (primitive prototypes).
5. **Big architecture:** Tasks 85 (trampoline), 241 (modules), 182/183 (classes), 251 (Promise).
6. **Object model:** Task 294 (descriptors), 88/264 (HIR/shapes).

## Testing requirement

Every item above must land with:
- A focused Rust unit test.
- A JS/TS scenario test in `crates/quench-runtime/tests/scenarios/` where applicable.
- A before/after conformance run showing the delta.
