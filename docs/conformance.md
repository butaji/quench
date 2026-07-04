> Running the test262 and TypeScript conformance harnesses.

# Conformance

External submodules:

```bash
git submodule update --init tests/test262 tests/typescript
```

Run:

```bash
cargo test -p quench-runtime --test test262 -- --ignored --nocapture
cargo test -p quench-runtime --test conformance -- --test-threads=1
```

## Latest results

Run against the current test subsets on `engine` branch:

| Suite | Total | Passed | Failed | Skipped | Pass rate (of total) | Pass rate (of non-skipped) |
|---|---|---|---|---|---|---|
| TypeScript full conformance subset | 100 | 14 | 47 | 39 | **14.0%** | **22.9%** |
| TypeScript expressions subset | 100 | 45 | 55 | 0 | **45.0%** | **45.0%** |
| test262 subset | 431 | 73 | 60 | 298 | **16.9%** | **54.9%** |

The TypeScript harness runs baseline JS extracted from compiler output, not source TS. The test262 harness currently stubs helpers and skips tests that require includes.

Do not edit `tests/test262/`, `tests/typescript/`, or `examples/`.
