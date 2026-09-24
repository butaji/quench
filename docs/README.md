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

The complete Test262 gate is deterministic and uncapped:

```sh
TEST262_TEST_TIMEOUT_MS=30000 \
TEST262_REPORT=target/test262-report.json \
  cargo run --release -p quench-test262 --bin run-all
```

The rewrite gate must use the v2-derived runtime explicitly. The current
conformance phase proceeds through the requested sequential `run-stages-next`
prefixes (0, 0–1, 0–2, 0–3), without a whole-inventory run; see task 20. Every
case has its own mandatory timeout. `run-stages-next` reports and executes
deterministic batches capped at 100 cases.

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

For long runs, execute deterministic file batches sequentially:

```sh
for i in $(seq 0 53); do
  TEST262_TEST_TIMEOUT_MS=30000 TEST262_BATCH_SIZE=1000 TEST262_BATCH_INDEX="$i" \
    TEST262_REPORT="target/test262-batches/file-${i}.json" \
    target/release/run-all || exit $?
done
```
