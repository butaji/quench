# test262 Conformance Harness Design

## Goal

Add the official ECMAScript test262 suite as a Git submodule and create a Rust-based harness that runs selected test262 tests against `quench-runtime`. The harness will produce a JSON report, skip unsupported features, and fix the runtime bugs it uncovers with unit-test-regression coverage.

## Scope (first phase)

- **Run only tests that are feasible today.** Skip tests that require features not yet implemented: ES modules, async/Promise, strict-mode enforcement, generators, classes, Symbol, RegExp, BigInt, Proxy, Reflect, WeakMap/WeakSet, TypedArray, destructuring defaults/rest, spread, `for...of`, `do...while`, `delete`, unary `+`, optional chaining, tagged templates, private names, JSX.
- **Focus on `test/language/expressions/` and `test/built-ins/Array/`** initially, expanding outward once the harness and core fixes are stable.
- **Negative tests included only for parse/runtime errors** that the runtime can already surface.

## Out-of-scope for first phase

- `module` flag tests.
- `async` flag / Promise tests.
- `onlyStrict` tests (unless strict mode is already working when this is implemented).
- Tests requiring unimplemented built-ins (RegExp, Symbol, WeakMap, TypedArray, etc.).

## Architecture

```
tests/test262/                         # new git submodule
crates/quench-runtime/tests/
  test262.rs                           # integration test entry
crates/quench-runtime/src/test262/
  mod.rs                               # runner, reporting, skip logic
  metadata.rs                          # frontmatter YAML parser
  harness.rs                           # minimal assert.js / sta.js / $DONE helpers
```

The harness will:

1. Parse each test's `/*--- ... ---*/` frontmatter with `serde_yaml`.
2. Decide whether to run, skip, or expect failure based on `flags`, `features`, and `negative`.
3. Load required harness helpers from `tests/test262/harness/` in declaration order.
4. Prepend `"use strict";` for `onlyStrict` tests (and record as expected failure until strict mode works).
5. Call `Context::eval()` and compare the outcome to the expected outcome.
6. Write `target/test262_report.json` with per-test results.

## Public API used

- `Context::new()` — fresh context with builtins.
- `Context::eval(source)` — run a JS string.
- `Context::set_global(name, Value::NativeFunction(...))` — optional capture helpers.

## Test outcome rules

| Test kind | Expected result | Pass condition |
|-----------|-----------------|----------------|
| Normal (no `negative`) | Runs without throwing | `ctx.eval(test)` is `Ok(_)`. |
| Negative parse | `ctx.parse(test)` returns `Err` whose message contains `negative.type`. | Message contains `SyntaxError`/`ReferenceError`/etc. |
| Negative runtime | `ctx.eval(test)` returns `Err` whose message contains `negative.type`. | Message contains expected type. |
| `onlyStrict` | Same as normal/negative, but source wrapped in `"use strict";`. | Initially recorded as expected failure. |

## Built-in harness helpers (minimal)

```js
function Test262Error(message) { this.message = message || ""; }
function $DONOTEVALUATE() { throw new Test262Error("This statement should not be evaluated."); }
function assert(mustBeTrue, message) { if (mustBeTrue !== true) throw new Test262Error(message); }
assert.sameValue = function (a, b, message) { if (a !== b) throw new Test262Error(message); };
assert.notSameValue = function (a, b, message) { if (a === b) throw new Test262Error(message); };
assert.throws = function (ExpectedError, fn, message) { try { fn(); } catch (e) { if (e instanceof ExpectedError || String(e).includes(ExpectedError.name)) return; throw new Test262Error(message); } throw new Test262Error(message); };
function $DONE(error) { if (error) throw error; }
```

## Skip policy

Skip any test whose frontmatter contains:

- `flags` includes `module`, `async`, `CanBlockIsFalse`, `CanBlockIsTrue`.
- `features` includes unsupported features: `Promise`, `Symbol`, `Symbol.*`, `generators`, `async-functions`, `class`, `BigInt`, `Proxy`, `Reflect`, `WeakMap`, `WeakSet`, `TypedArray`, `RegExp`, `default-parameters`, `destructuring-binding`, `spread`, `template-literals`, `optional-chaining`, `private-fields`, etc.
- Path contains `module/`, `async/`, `class/`, `generators/`, etc.

## Reporting

JSON report (`target/test262_report.json`):

```json
{
  "total": 1234,
  "passed": 900,
  "failed": 200,
  "skipped": 134,
  "results": [
    { "path": "test/language/expressions/addition/S11.6.1_A1.js", "outcome": "pass" },
    { "path": "test/language/expressions/typeof/S11.4.3_A1.js", "outcome": "fail", "error": "..." }
  ]
}
```

## Regression testing

Every runtime bug found by test262 must be reproduced as a focused unit test in `crates/quench-runtime/tests/` before fixing it. These tests run in the normal `cargo test` suite, not just the test262 harness.

## Verification

- `git submodule update --init tests/test262` succeeds.
- `cargo test -p quench-runtime --test test262 -- --ignored` runs and produces a report.
- Existing `cargo test -p quench-runtime` still passes.
