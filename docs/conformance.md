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
./scripts/run_tests.sh test-test262      # test262 full suite, no skips
./scripts/run_tests.sh test-conformance  # TypeScript expressions subset
```

Or directly:

```bash
cargo test -p quench-runtime --test test262 -- --ignored --nocapture
cargo test -p quench-runtime --test conformance -- --ignored --nocapture
```

## No-skip policy

The test262 harness must execute **every** `.js` file under `tests/test262/test`. Tests are never pre-filtered or skipped because of an unsupported feature, a missing built-in, or an expected failure. Instead, each test runs in an isolated thread with a fresh context, and failures are bucketed by root cause. This gives a true compatibility percentage and makes the next high-impact fix obvious.

## Latest results

```text
TypeScript expressions (376 cases):      149 passed, 227 failed, 0 skipped (39.6%)
TypeScript 100-case sanity:               12 passed,  49 failed, 39 skipped (12%)
test262 full suite (~53,683 cases):      TBD passed, TBD failed, 0 skipped (TBD%)
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
- **Task 88** — Rust-specific runtime model (slot-indexed storage, isolate threads, allocator/interning strategy).
