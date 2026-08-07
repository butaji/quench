# quench-test262 harness diagnostics

Diagnostics belong to the conformance runner boundary. They may report
engine outcomes and timings, but must not couple the runner to runtime
implementation details.

**Status:** implemented in transitional runner; extraction pending
**Goal:** Keep diagnostics in `quench-test262` while preserving a narrow
engine host boundary and accelerating the path to 100% test262.

## Tools Implemented

### Runner Env-Vars (`src/test262/runner.rs`)

| Env Var | Purpose | Status |
|---------|---------|--------|
| `TEST262_SHOW_SCRIPT=1` | When a test fails, dump the full generated JS script | done |
| `TEST262_DUMP_FAILURES=<path>` | Save failure list to JSON file | done |
| `TEST262_RERUN_FAILURES=<path>` | Re-run only tests from failure file | done |
| `TEST262_FIRST_N=<N>` | Only run first N tests in digest | done |

### Tools (`tools/`)

| Tool | Purpose | Status |
|------|---------|--------|
| `run-test` (enhanced) | Single-test runner with --strict/--module/--async/--stack/--show-script | done |
| `inspect-test` (new) | Show parsed test metadata without running | done |

### Documentation

| Doc | Status |
|-----|--------|
| `docs/tools.md` | done - comprehensive reference with all env-vars + workflows |

## Usage in Workflow

1. `TEST262_SHOW_SCRIPT=1` when debugging a cryptic failure
2. `TEST262_DUMP_FAILURES=/tmp/fails.txt + TEST262_RERUN_FAILURES=/tmp/fails.txt` for fast iteration
3. `TEST262_FIRST_N=10` for quick smoke test
4. `cargo run --bin inspect-test -- path/to/test.js` to understand expectations
5. `cargo run --bin run-test -- --stack --show-script path/to/test.js` for single-test debugging with full visibility

## Verification
- `cargo clippy -p quench-runtime --all-targets` — zero new warnings (3 pre-existing only)
- `cargo test -p quench-runtime --lib` — 1899 passed, 0 failed
- `cargo build -p run-test -p inspect-test` — clean
- `cargo run --bin inspect-test -- <test.js>` — works
- `cargo run --bin run-test -- <test.js>` — works

## Boundary follow-up

- [ ] Move runner diagnostics behind the standalone `quench-test262` crate.
- [ ] Keep engine metrics exposed as stable result data, not runtime-internal
  types.
- [ ] Re-run Stage 44 after extraction and record wall time, RSS, and outcome
  counts in `tasks/stage44-performance-progress.md`.
