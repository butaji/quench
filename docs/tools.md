# Development Tools

The test262 digest output is the sole source of conformance/progress status.
`tasks/index.json` is descriptive configuration only. This document contains
commands, not copied status or milestones.

## Normal workflow

There is one canonical conformance command: the filtered nextest invocation
below runs the single staged harness test, which owns per-file execution and
the digest. The shell tools only select a stage, capture output, or format
that result; they do not maintain a second coverage counter. `cargo run
--bin run-test` is reserved for one-test diagnosis, and `run-each.sh` is the
crash-isolated fallback.

```bash
# Fast unit-test suite (requires cargo-nextest)
cargo nextest run -p quench-runtime

# Test262 harness (the staged test is one harness test; the harness runs files)
cargo nextest run -p quench-runtime --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all

# Run one test
cargo run --bin run-test -- tests/test262/path/to/test.js

# Run the configured stage
cargo nextest run -p quench-runtime --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture

# Run a selected stage
TEST262_STAGE=N cargo nextest run -p quench-runtime --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture

# Collect all failures and group them by error
TEST262_STAGE=N TEST262_DIGEST=1 cargo nextest run -p quench-runtime --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture

# Fast digest invocation; output remains the Test262 SSOT
TEST262_STAGE=N TEST262_DIGEST=1 cargo nextest run -p quench-runtime --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all

# Crash-isolated fallback for a stage (slower, useful after a runtime crash)
TEST262_STAGE=N bash tools/run-each.sh

# Run every stage in order
ALL_STAGES=1 cargo nextest run -p quench-runtime --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture
```

The runner may use `tasks/index.json` to determine the configured stage. Do not infer
completion from an old command log or from this document.

## Verification

```bash
cargo nextest run -p quench-runtime
cargo fmt -p quench-runtime -- --check
cargo clippy -p quench-runtime --all-targets
```

## Diagnostics

```bash
# Metadata and generated source
cargo run --bin inspect-test -- tests/test262/path/to/test.js
cargo run --bin run-test -- --show-script tests/test262/path/to/test.js

# Persisted failure report, when supported by the local runner
bash tools/digest-all.sh
```

The helper scripts under `tools/` are implementation details of the runner.
Use them directly only when their command-line behavior is needed; do not
add their output or progress summaries to documentation.
