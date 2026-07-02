> **Canonical usage docs for both conformance harnesses.**

# Conformance Harnesses

Quench has two Rust-only conformance harnesses that run against external test submodules:

- **test262** — official ECMAScript suite (`tests/test262`)
- **TypeScript** — TypeScript compiler conformance cases (`tests/typescript`)

Both harnesses live in `crates/quench-runtime/tests/` and register their helpers as Rust native functions. No JS helper strings are injected.

## Setup

```bash
git submodule update --init tests/test262 tests/typescript
```

## Running the harnesses

```bash
# test262 tiny subsets
./scripts/run_tests.sh test-test262

# TypeScript expressions subset
./scripts/run_tests.sh test-conformance

# Direct cargo invocations
cargo test -p quench-runtime --test test262 -- --ignored --nocapture
cargo test -p quench-runtime --test conformance -- --ignored --nocapture
```

## Latest results

```text
TypeScript expressions (376 cases):      149 passed, 227 failed, 0 skipped (39.6%)
TypeScript 100-case sanity:               12 passed,  49 failed, 39 skipped (12%)
test262 expressions subset (431 cases):   45 passed,  88 failed, 298 skipped (10.4%)
```

## Why whole-suite runs are not yet possible

1. **Recursive interpreter stack overflow.** Running more than a few hundred test files on the same thread exhausts the native Rust stack.
2. **Global recursion counter.** `CURRENT_DEPTH` in `interpreter/control.rs` is a global `AtomicUsize`, so thread-per-case isolation in the TypeScript harness is undermined.
3. **Large unsupported feature surface.** test262 skips 84 feature categories (Promise, async, generators, classes, BigInt, Symbol, Proxy, RegExp, modules, spread, destructuring, template literals, etc.).
4. **Baseline/harness gaps.** Many TypeScript cases fail because no baseline is found or the baseline extractor mis-parses the file. test262 include files are stubbed, not loaded from the submodule.
5. **Runtime bugs.** Top failure signatures are `ReferenceError`, `Invalid computed property`, `is not a function`, and `Parse error`.

See `tasks/82-whole-suite-conformance-analysis.md` for the full analysis and phased plan.

## Current priorities

Ranked by low effort / high impact:

1. **Finish missing operators (Task 81)** — `??`, `?.`, unary `+`, `delete`, `||=`/`&&=`/`??=` are behind many top failures.
2. **Skip "No baseline found" cases** — trivial; removes noise so reports show real runtime bugs.
3. **Thread-local `CURRENT_DEPTH` + `MAX_JS_STACK` guard** — trivial/low effort; improves harness stability.
4. **Fix the `ReferenceError: A is not defined` root cause** — likely a single class/namespace hoisting issue that removes dozens of failures.
5. **Load test262 include files from the submodule** — medium effort; makes test262 results trustworthy.
6. **Trampoline interpreter (Task 85)** — high effort; the proper long-term fix for stack overflow.

See `tasks/86-implementation-priorities.md` for the full ranked rationale.

## Reports

Each run writes two artifacts to `target/`:

- `conformance_expressions_report.json` / `conformance_report.json` — full per-case JSON.
- `conformance_expressions_report.md` / `conformance_report.md` — human-readable Markdown with pass rate, top failure signatures, and per-category pass rates.

For test262:

- `test262_report.json`
- `test262_report.md`

The printed summary also shows the overall pass rate, the top failure signatures, and the categories with the most failures so the next bug to fix is obvious.

## Fixing bugs from the harness

The harness is meant to drive development, not just measure it. Use it to find the next bug, fix it with a regression test, and verify the bucket shrinks.

1. Run a harness subset and open the Markdown report.
2. Pick the highest-impact failure signature (count × clarity of root cause).
3. Reduce the example path from the report to a minimal reproduction.
4. Write a focused regression test in `crates/quench-runtime/tests/` that fails before the fix.
5. Fix the smallest piece of runtime code that makes the test pass.
6. Run `cargo test -p quench-runtime`.
7. Re-run the harness subset and confirm the error signature count dropped.
8. Move to the next bucket.

See `tasks/84-conformance-driven-bug-fixing.md` for the full workflow.

## Boundaries

Do not modify files in `examples/`, `tests/test262/`, or `tests/typescript/`.
