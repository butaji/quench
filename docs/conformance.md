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
| TypeScript expressions | 376 | 153 | 223 | 0 | **40.7%** | **40.7%** |
| test262 subset | 431 | 47 | 86 | 298 | **10.9%** | **35.3%** |

The TypeScript harness runs baseline JS extracted from compiler output, not source TS. The test262 harness now loads real harness includes but still skips tests that require unsupported features or includes that are not yet implemented.

Do not edit `tests/test262/`, `tests/typescript/`, or `examples/`.
