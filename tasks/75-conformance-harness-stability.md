# Task 73: Make conformance harnesses stable for full-suite runs

## Goal

Both the test262 and TypeScript conformance harnesses currently crash with a native stack overflow when run over larger subsets. Fix the runtime so the harnesses can process the full intended suites without aborting.

## Current behavior

- `./scripts/run_tests.sh test-test262` crashes on `test262_builtins_array` with stack overflow.
- `cargo test -p quench-runtime --test conformance test_typescript_conformance_sanity -- --nocapture` crashes with stack overflow while walking all conformance cases.
- Individual small subsets work:
  - `test262_expressions`: total=435, passed=31, failed=31, skipped=373
  - `test_typescript_conformance_expressions`: total=376, passed=131, failed=208, skipped=37

## Likely causes

1. The interpreter has no recursion depth limit, so a test with infinite or very deep recursion exhausts the native stack before the harness can record a failure.
2. `Array.prototype` global storage previously used `Mutex<Arc<...>>` and was changed to thread-local; there may still be prototype-chain cycles or unbounded recursion in property lookup.
3. The test262 runner reuses one `Context` across all tests; leaked state from one test may affect the next.

## Steps

1. **Add a recursion budget to the interpreter.**
   - Track call depth in `Context` or `Env`.
   - When depth exceeds a configurable limit (e.g. 1000 for harness, higher for production), return `JsError("Maximum call stack size exceeded")` instead of recursing.
   - Reset the budget between top-level `ctx.eval` calls.

2. **Run each test262/TypeScript case in a fresh `Context`.**
   - This prevents state leakage and makes crashes isolated.
   - Keep harness helper registration cheap or cache a pre-configured template context.

3. **Identify and skip/document the crashing cases.**
   - If a particular test262/TypeScript case still overflows after the recursion limit, add it to an expected-failures/skip list with a link to a bug task.

4. **Verify full-suite runs.**
   - `./scripts/run_tests.sh test-test262` completes and writes `target/test262_report.json`.
   - `./scripts/run_tests.sh test-conformance` completes and writes `target/conformance_report.json`.

## Boundaries

- Changes are limited to `crates/quench-runtime/src/interpreter.rs`, `crates/quench-runtime/src/test262/runner.rs`, `crates/quench-runtime/src/conformance/typescript/mod.rs`, and related files.
- Do not modify `examples/` or `tests/typescript/`.

## Verification

```bash
./scripts/run_tests.sh test-test262
./scripts/run_tests.sh test-conformance
cargo test -p quench-runtime
```

All commands must run with timeouts.
