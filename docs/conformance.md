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
# test262 expressions subset
./scripts/run_tests.sh test-test262

# TypeScript expressions subset
./scripts/run_tests.sh test-conformance

# Direct cargo invocations
cargo test -p quench-runtime --test test262 -- test262_expressions --ignored --nocapture
cargo test -p quench-runtime --test conformance -- test_typescript_conformance_expressions --ignored --nocapture
```

## Latest results

```text
test262_expressions:  total=431  passed=44  failed=89  skipped=298
TypeScript expressions: total=376  passed=131 failed=208 skipped=37
```

Full-suite runs can still hit native stack overflow on crashers. The pending work is tracked in `tasks/index.json` (Task 75).

## Adding regression tests

When a harness case exposes a runtime bug:

1. Reproduce it as a focused Rust unit test in `crates/quench-runtime/tests/`.
2. Fix the runtime code.
3. Run `cargo test -p quench-runtime`.

Do not modify files in `examples/`, `tests/test262/`, or `tests/typescript/`.
