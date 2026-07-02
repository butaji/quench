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
TypeScript expressions (376 cases):  124 passed, 252 failed, 0 skipped (33%)
TypeScript 100-case sanity:           12 passed,  49 failed, 39 skipped (12%)
test262: only tiny subsets run; no recent full-subset report
```

## Why whole-suite runs are not yet possible

1. **Recursive interpreter stack overflow.** Running more than a few hundred test files on the same thread exhausts the native Rust stack.
2. **Global recursion counter.** `CURRENT_DEPTH` in `interpreter/control.rs` is a global `AtomicUsize`, so thread-per-case isolation in the TypeScript harness is undermined.
3. **Large unsupported feature surface.** test262 skips 84 feature categories (Promise, async, generators, classes, BigInt, Symbol, Proxy, RegExp, modules, spread, destructuring, template literals, etc.).
4. **Baseline/harness gaps.** Many TypeScript cases fail because no baseline is found or the baseline extractor mis-parses the file. test262 include files are stubbed, not loaded from the submodule.
5. **Runtime bugs.** Top failure signatures are `ReferenceError`, `Invalid computed property`, `is not a function`, and `Parse error`.

See `tasks/82-whole-suite-conformance-analysis.md` for the full analysis and phased plan.

## Reports

Each run writes two artifacts to `target/`:

- `conformance_expressions_report.json` / `conformance_report.json` — full per-case JSON.
- `conformance_expressions_report.md` / `conformance_report.md` — human-readable Markdown with pass rate, top failure signatures, and per-category pass rates.

For test262:

- `test262_report.json`
- `test262_report.md`

The printed summary also shows the overall pass rate, the top failure signatures, and the categories with the most failures so the next bug to fix is obvious.

## Adding regression tests

When a harness case exposes a runtime bug:

1. Reproduce it as a focused Rust unit test in `crates/quench-runtime/tests/`.
2. Fix the runtime code.
3. Run `cargo test -p quench-runtime`.

Do not modify files in `examples/`, `tests/test262/`, or `tests/typescript/`.
