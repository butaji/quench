> Roadmap to **100% ECMA-262 + TypeScript language compatibility** for `.ts/.tsx/.js/.jsx` running natively in Quench. No bytecode, no JIT. **Minimum code, maximum performance.**

# JS/TS Compatibility Roadmap

## North star

- **100% spec compatibility.** Every observable JS/TS/TSX/JSX behavior works.
- **Minimum code.** The smallest Rust implementation that matches the spec. No speculative layers.
- **Maximum performance.** Hot paths use slots, shapes, interned strings, and explicit state. Cold paths stay simple.
- **Complete fast test coverage.** Every spec behavior has a focused Rust unit test or fixture; regressions are caught in seconds.

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

## Minimum custom code

The detailed strategy is in `docs/minimum-custom-code-strategy.md`. The short version:

- **Reuse, don't rewrite:** keep `swc_ecma_parser` for parsing, adopt `test262-harness` for the runner, use `indexmap`, `lasso`, `num-bigint`, `chrono`, `regex`, and `serde_json` for built-ins.
- **One object representation:** collapse `Function` / `NativeFunction` / `NativeConstructor` / `Array` into `Value::Object` with `[[Call]]` / `[[Construct]]` slots.
- **AST → interpreter only:** no HIR, no bytecode, no JIT until 100% conformance is reached.
- **Conformance suites are the backlog:** fix buckets reported by test262/TypeScript harnesses rather than writing spec tests from scratch.

New tracking tasks: 334, 335, 336.

## Batched milestones

Work is grouped into measurable batches. Each batch has a theme, a primary test-suite focus, and an exit criterion: every task in the batch must make its `target_subset` pass at 100% with zero spec skips. The registry in `tasks/index.json` carries the authoritative `suite`, `category`, `batch`, `target_subset`, `blocked_by`, and `exit_criteria` for every task; regenerate it with `python3 scripts/target_tasks.py`.

| Batch | Theme | Suite focus | Tasks (sample) | Exit signal |
|-------|-------|-------------|----------------|-------------|
| 0 | Truthful measurement | test262 / both | 91, 97, 250, 253, 344 | Harness helpers loaded; reports reflect real pass/fail/skip counts. |
| 1 | Quick syntax / builtin wins | both | 289, 290, 320–324 | Expression, object, and constructor subsets reach 100%. |
| 2 | Functions / core statements | both | 322, 281, 141, 291–293 | Function, statement, and basic semantic subsets reach 100%. |
| 3 | Big architecture | both | 85, 182, 183, 187, 241, 251, 297, 298 | Trampoline, modules, classes, promises, and test harness unblock whole suite areas. |
| 4 | P1 correctness | both | 141, 294, 295 | Property descriptors, global object, and strict-mode subsets reach 100%. |
| 5 | Granular language coverage | test262 / both | 105, 112, 117, 119, 124, 132, 147, 191, 239, 290a–g, 309–319 | Per-area coverage milestones (expressions, statements, functions, objects, arrays, classes, modules, errors, async, TypeScript, JSX) reach 100%. |
| 6 | Full suites / host polish | both / runtime | 82, 88, 256, 264, 296 | Entire test262 + TypeScript conformance suites pass; runtime/tooling guardrails prevent regression. |

## Order of attack (low effort / high impact first)

1. **Measurement:** Tasks 253, 91, 250, 97, 344.
2. **Syntax quick wins:** Task 290 (template literals, computed keys, spread, `??`, `?.`, `delete`, unary `+`, `for-of`) and sub-tasks 290a–g.
3. **Built-in constructors:** Task 289 (`Array`, `Error`, `Date` as constructors) and sub-tasks 289a–c.
4. **Core semantics quick wins:** Tasks 291 (`typeof`), 292 (hoisting/TDZ), 293 (`arguments`), 141 (strict mode), 283 (primitive prototypes).
5. **Big architecture:** Tasks 85 (trampoline), 241 (modules), 182/183/187 (classes), 251 (Promise).
6. **Object model:** Task 294 (descriptors), 88/264 (HIR/shapes).

## Testing requirement

Every item above must land with:
- A focused Rust unit test.
- A spec fixture in `crates/quench-runtime/tests/spec_fixtures/` (see `docs/spec-test-fixtures.md`) that exercises the real JS/TS snippet.
- A JS/TS scenario test in `crates/quench-runtime/tests/scenarios/` where applicable.
- A before/after conformance run showing the delta.
