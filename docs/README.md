# Documentation

- [Repository rules and Lisp mindset](../AGENTS.md)
- [Active rewrite queue](../tasks/index.json)
- [Task execution contract](../tasks/README.md)
- [Pinned v2 design decisions](v2/architecture.md) (copied from `../v2` at the indexed reference commit; binding for the next core)
- [Wasm boundary](spec.md)
- [Test262 stages](STAGES.md) and [Node stages](../STAGES.md)
- [V8-v7 commands](v8_v7.md)
- [Benchmark micros](../quench-bench/micros/README.md)

Documentation records stable contracts and reproducible commands. Current work
and status live only in `tasks/index.json` and the numbered task files. Build
and measurement artifacts belong under ignored `target/` directories and must
record their source revisions and commands.

The rewrite target is an interpreter-only runtime derived from the pinned
`../v2` snapshot. Node host behavior remains in `quench-node`; JavaScript
semantics remain in `quench-runtime`; OXC owns syntax. The final conformance
gates are the pinned Test262, Wasm, and tracked Node-compat inventories named
in the queue.

The rewrite gate must use the v2-derived runtime explicitly. Test262 progress
is stage-ordered with `run-stages-next`; never replace that ratchet with a
whole-inventory run. Every case has its own mandatory timeout, and the runner
reports and executes deterministic batches capped at 100 cases. Task 20 owns
the current stage and cumulative coverage record.

The default `dev` profile is incremental and uses no optimization for the
shortest Rust edit/compile cycle. For conformance iterations, use the
incremental `iteration` profile: it uses level-one optimization, parallel code
generation, and no LTO so the runner remains reasonably fast without making
every edit pay release-build costs. It is not the production/performance-
evidence profile. Use `--release` for the final release gate and matched
performance measurements.

```sh
TEST262_TEST_TIMEOUT_MS=900000 TEST262_JOBS=4 \
  cargo run --profile iteration -p quench-test262 --bin run-stages-next -- 10 10
```

```sh
TEST262_TEST_TIMEOUT_MS=30000 TEST262_JOBS=10 \
  cargo run --release -p quench-test262 --bin run-all-next
```

```sh
TEST262_TEST_TIMEOUT_MS=30000 \
  cargo run --release -p quench-test262 --bin run-stages-next -- 0 0
```

The existing `run-stages`/`run-all` binaries retain the legacy host for
reference comparisons and are not evidence for the next-runtime gate.
Every Test262 runner/tool (`run-test`, both stage/all variants, `triage`, and
`compare-runs`) refuses to start without a positive `TEST262_TEST_TIMEOUT_MS`;
the batch wrapper has the same requirement. `run-all-next` and
`run-stages-next` run case processes concurrently (configurable with
`TEST262_JOBS`, defaulting to available parallelism), preserve discovery-order
reports, and keep stage batches capped at 100. The fixture comparison scripts
`tools/diff-next.mjs` and `tools/run-all-next.mjs` require positive
`DIFF_TIMEOUT_MS` values.

`run-all-next` writes its full per-test report to
`target/test262-next-report.json` and compares a full-inventory run against
`target/test262-next-ratchet.json`. A first complete all-pass run freezes the
baseline; a later lost pass is reported as a regression and fails the command.
Set `TEST262_REPORT` or `TEST262_RATCHET` to select other paths. A
`TEST262_BATCH_SIZE` subset still writes its report but does not update or
compare the full-inventory ratchet.

For an explicitly authorized all-pass assumption, freeze expectations without
executing any cases:

```sh
TEST262_TEST_TIMEOUT_MS=30000 \
  cargo run --profile iteration -p quench-test262 --bin run-all-next -- \
  --freeze-expected-pass-set
```

This refuses to overwrite an existing baseline and records
`basis: user_assumption` and the suite revision, without generating observed
outcomes. A later complete observed all-pass run replaces that basis with
`observed`. `run-stages-next` compares only baseline expectations owned by its
selected stages and path filter, including previously expected paths removed
from discovery. Focused runs never update the baseline. `TEST262_RATCHET`
selects the same baseline in both runners; an explicitly selected missing or
invalid baseline is an error. A stage selection discovering no cases is an
error.

For long runs, advance one stage at a time with `run-stages-next`; it divides
large stages into deterministic batches of at most 100 cases and preserves
discovery order. Do not use the legacy `run-all` batch wrapper for the current
next-runtime ratchet.
