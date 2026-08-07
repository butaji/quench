# Development Tools

Test262 runs are the sole source of conformance results.
`tasks/index.json` is descriptive configuration only. This document contains
commands, not copied status or milestones.

## Conformance-first change gate

Use focused and affected-stage runs to diagnose a minimal conformance fix.
Do not use them to authorize a refactor, migration, architectural expansion,
or other complexity-increasing work while the complete configured corpus is
below 100%. Those changes require a complete zero-failure, zero-skip baseline
and a second complete run proving that they preserved it.

## Normal workflow

There is one canonical conformance command: the filtered nextest invocation
below runs the single staged harness test, which owns per-file execution and
the digest. The shell tools only select a stage, capture output, or format
that result; they do not maintain a second coverage counter or write
conformance status into `docs/` or `tasks/`. `cargo run
--bin run-test` is reserved for one-test diagnosis, and `run-each.sh` is the
crash-isolated fallback.

```bash
# Fast unit-test suite (requires cargo-nextest)
cargo nextest run -p quench-runtime

# Test262 harness (the staged test is one harness test; the harness runs files)
cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all

# Run one test
cargo run --bin run-test -- tests/test262/path/to/test.js

# Run the default stage (stage 0 unless TEST262_STAGE is set)
cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture

# Run a selected stage
TEST262_STAGE=N cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture

# Collect all failures and group them by error
TEST262_STAGE=N TEST262_DIGEST=1 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture

# Fast digest invocation; output remains the Test262 SSOT
TEST262_STAGE=N TEST262_DIGEST=1 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all

# Crash-isolated fallback for a stage (slower, useful after a runtime crash;
# exits non-zero when any test fails or times out)
TEST262_STAGE=N bash tools/run-each.sh

# Run every stage in order
ALL_STAGES=1 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture
```

The runner does not read progress state from `tasks/index.json`. Set
`TEST262_STAGE` explicitly when selecting a stage. Do not infer results from an
old command log or from this document.

## Verification

```bash
cargo nextest run -p quench-runtime
cargo fmt -p quench-runtime -- --check
cargo clippy -p quench-runtime --all-targets

# Reject duplicate JS/Rust builtin ownership
bash tools/check-builtin-ownership.sh
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

The harness owns the per-file loop and produces the digest. Parallelism must be
controlled by the Test262 runner, not by nextest splitting the harness test.
The preferred acceleration path is a bounded persistent worker pool: workers
bootstrap once, execute worker-local mutable contexts, and return structured
results. Process isolation remains available for crash-prone tests.
`run-each.sh` is the diagnostic crash-isolation path and is not a replacement
for the digest command.

## Fast conformance loop

Use `TEST262_QUICK=1` to reduce retained failure groups, then use a complete
digest for evidence. It is not currently a bounded representative sample: the
serial runner continues through passing tests until it observes the configured
number of distinct failure groups.

```bash
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_QUICK=1 \
  cargo nextest run -p quench-test262 --test test262 --profile test262 \
  -E 'test(test262_staged)' --run-ignored all --no-capture

TEST262_STAGE=N TEST262_DIGEST=1 \
  cargo nextest run -p quench-test262 --test test262 --profile test262 \
  -E 'test(test262_staged)' --run-ignored all --no-capture
```

The first command is triage only. The second is conformance evidence.

Benchmark worker count against wall time, tests per second, peak memory,
timeouts, and crashes. Record discovery, metadata, harness, bootstrap, parse,
execution, microtask, cleanup, and worker-startup timings. Cache only
immutable parsed/bootstrap artifacts until context reset hygiene proves that
mutable state can be reused safely.

Independent stages can be batched concurrently only through isolated result
files and serialized merge/advance. A batch is complete only after every
stage reports, including crashes, timeouts, and skips. `tasks/index.json`
remains descriptive configuration, never execution evidence.

Development mode should optimize verified failures cleared per hour: use quick
representatives and stable root-cause groups for triage, then run the complete
affected stage as evidence. Full all-stage runs remain release verification.
