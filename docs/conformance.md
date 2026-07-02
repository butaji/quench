> **Running the test262 and TypeScript conformance harnesses.**

# Conformance Harnesses

Quench runs two Rust-only conformance harnesses against external submodules:

- **test262** — official ECMAScript suite (`tests/test262`)
- **TypeScript** — TypeScript compiler conformance cases (`tests/typescript`)

## Setup

```bash
git submodule update --init tests/test262 tests/typescript
```

## Run

```bash
./scripts/run_tests.sh test-test262      # test262 subsets
./scripts/run_tests.sh test-conformance  # TypeScript expressions subset
```

Or directly:

```bash
cargo test -p quench-runtime --test test262 -- --ignored --nocapture
cargo test -p quench-runtime --test conformance -- --ignored --nocapture
```

## Latest results

```text
TypeScript expressions (376 cases):      149 passed, 227 failed, 0 skipped (39.6%)
TypeScript 100-case sanity:               12 passed,  49 failed, 39 skipped (12%)
test262 expressions subset (431 cases):   45 passed,  88 failed, 298 skipped (10.4%)
```

## Reports

Each run writes JSON + Markdown reports to `target/`:

- `conformance_expressions_report.json` / `.md`
- `conformance_report.json` / `.md`
- `test262_report.json` / `.md`

The Markdown report shows the pass rate, top failure signatures, and per-category pass rates so the next bug is obvious.

## Fixing bugs

1. Run a subset and open the Markdown report.
2. Pick a high-count failure signature.
3. Reduce the example path to a minimal reproduction.
4. Write a regression test in `crates/quench-runtime/tests/`.
5. Fix the smallest runtime change that makes it pass.
6. Re-run the subset and confirm the bucket shrank.

Do not edit `examples/`, `tests/test262/`, or `tests/typescript/`.

## Priorities

Pick the highest-impact, lowest-effort bucket from the Markdown report, fix it with a regression test, and re-run. Active tasks are in `tasks/index.json`:

- **Task 82** — whole-suite run blockers.
- **Task 85** — trampoline interpreter to eliminate stack overflow.
