# Unified Rust Conformance Harness for test262 and TypeScript Submodules

## Goal

Run both external JavaScript/TypeScript conformance suites — `tests/test262` and `tests/typescript` — entirely from Rust. Replace the current Node-based TypeScript runner (`scripts/run_typescript_runner.sh`) and avoid injecting any JS helper strings. All harness logic, helper functions, reports, and regression tests are Rust code.

## Submodules

| Submodule | Path | Content | First-phase scope |
|-----------|------|---------|-------------------|
| `test262` | `tests/test262` | Official ECMAScript conformance tests | Non-strict, synchronous, non-module JS tests without unsupported features. |
| `TypeScript` | `tests/typescript` | TypeScript compiler test cases | Baseline-JS mode first; source-direct TS after `eval_ts` works. |

## Architecture

```
tests/test262/                              # git submodule
tests/typescript/                           # git submodule
crates/quench-runtime/src/conformance/
  mod.rs                                    # shared types and reporting
  test262/
    mod.rs                                  # runner
    metadata.rs                             # frontmatter parser
    helpers.rs                              # native harness helpers (Rust)
  typescript/
    mod.rs                                  # runner
    directives.rs                           // @target, @module, @filename parsing
    baseline.rs                             # baseline .js lookup
    helpers.rs                              # TS emit helpers as native functions
    skip.rs                                 # skip rules
crates/quench-runtime/tests/
  conformance.rs                            # entry point for both suites
```

All helpers are Rust native functions registered through `Context::set_global` / `register_native`. The only JS files executed are the actual test cases from the submodules.

## Public API additions

Add to `crates/quench-runtime/src/lib.rs`:

```rust
impl Context {
    /// Evaluate JS/TSX source that may contain TypeScript syntax.
    /// Uses swc to parse TypeScript, strip types with swc transforms, then lower and execute.
    pub fn eval_ts(&mut self, source: &str) -> Result<Value, JsError> { ... }
}
```

Implementation:

1. Parse with `swc_ecma_parser` using `Syntax::Typescript(...)`.
2. If the source is a module, parse with `parse_module`; otherwise `parse_script`.
3. Run `swc_ecma_transforms_typescript::strip` to remove type annotations.
4. Lower the resulting JS AST with the existing lowerer.
5. Execute via the interpreter.

## Test262 runner

Same as `docs/superpowers/specs/2026-06-28-test262-conformance-design.md`, but helpers are native Rust functions:

- `Test262Error`, `$DONOTEVALUATE`, `assert`, `assert.sameValue`, `assert.notSameValue`, `assert.throws`, `$DONE`, `print`.
- Skip unsupported flags/features.
- Report to `target/test262_report.json`.

## TypeScript runner

### Baseline-JS mode (phase 1)

For each `.ts`/`.tsx` conformance case:

1. Parse directives (`// @target:`, `// @module:`, `// @jsx:`, `// @filename:`, `// @noEmit`, `// @emitDeclarationOnly`, etc.).
2. Skip per the rules in `docs/conformance.md`.
3. Locate the corresponding baseline `.js` file in `tests/typescript/tests/baselines/reference/`:
   - Handle configuration-specific names (`case.es2015.commonjs.js`).
   - Handle multi-file baselines (`case.1.js`, `case.2.js`).
4. Preload TS emit helpers as native functions: `__extends`, `__assign`, `__awaiter`, `__decorate`, `__importStar`, `__importDefault`.
5. Evaluate the baseline JS with `ctx.eval()`.
6. Record pass/fail/skip.

### Source-direct TS mode (phase 2)

Once `eval_ts` is implemented:

1. For each case, try `ctx.eval_ts(&ts_source)`.
2. If it fails, optionally fall back to baseline JS (hybrid mode).
3. Record whether source-direct passes.

### Report

`target/conformance_report.json`:

```json
{
  "timestamp": "...",
  "total": 200,
  "passed": 112,
  "failed": 46,
  "skipped": 42,
  "pass_rate": 70.9,
  "mode": "baseline" | "source" | "hybrid",
  "cases": [
    { "name": "es6/for-of1.ts", "category": "es6", "status": "pass", "message": null }
  ]
}
```

## Shared reporting

Both runners use a common `Report` trait:

```rust
pub trait ConformanceReport {
    fn total(&self) -> usize;
    fn passed(&self) -> usize;
    fn failed(&self) -> usize;
    fn skipped(&self) -> usize;
    fn write_json(&self, path: &Path) -> Result<(), std::io::Error>;
}
```

## Skip / unsupported lists

### test262

- Flags: `module`, `async`, `CanBlockIsFalse`, `CanBlockIsTrue`.
- Features: `Promise`, `Symbol`, `generators`, `async-functions`, `class`, `BigInt`, `Proxy`, `Reflect`, `WeakMap`, `WeakSet`, `TypedArray`, `RegExp`, `default-parameters`, `destructuring-binding`, `spread`, `template-literals`, `optional-chaining`, `private-fields`, etc.

### TypeScript

- Directives: `@noEmit`, `@emitDeclarationOnly`, unsupported `@module` systems, `@jsx: react`.
- Directories: `types/`, `interfaces/`, `symbols/`, `namespaces/`, etc.
- Files ending `.errors.txt`.

## Local pass-rate gate

Both harnesses read `MIN_PASS_RATE` from the environment. If the pass rate is below the threshold, the integration test fails. This keeps the gate local (no CI).

## Regression testing

Every bug surfaced by either harness must be reproduced as a focused Rust unit test in `crates/quench-runtime/tests/` before the runtime fix is committed.

## Verification

```bash
git submodule update --init tests/test262 tests/typescript
cargo test -p quench-runtime --test conformance -- --ignored --nocapture
```

All commands run with the timeout wrapper in `scripts/run_tests.sh`.
