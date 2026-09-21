# Documentation

- [Repository rules and Lisp mindset](../AGENTS.md)
- [Active rewrite queue](../tasks/index.json)
- [Task execution contract](../tasks/README.md)
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
TEST262_REPORT=target/test262-report.json \
  cargo run --release -p quench-test262 --bin run-all
```

For long runs, execute deterministic file batches sequentially:

```sh
for i in $(seq 0 53); do
  TEST262_BATCH_SIZE=1000 TEST262_BATCH_INDEX="$i" \
    TEST262_REPORT="target/test262-batches/file-${i}.json" \
    target/release/run-all || exit $?
done
```
