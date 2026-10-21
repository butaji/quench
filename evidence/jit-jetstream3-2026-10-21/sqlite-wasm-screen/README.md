# SQLite Wasm JIT diagnostic

Command:

```sh
PATH=/tmp/quench-bun/bin:/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin:/tmp/quench-cargo/bin:$PATH \
  target/production/quench-bench --fixtures sqlite3-wasm --suite jetstream3 \
  --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node --node node --bun bun \
  --out /tmp/quench-sqlite3-wasm-diagnostic.json
```

One-round result: valid output; Quench scored 0.25 / 162.4 MiB RSS, Node 25.96 / 67.6 MiB, and Bun 12.26 / 89.4 MiB. The earlier full-suite screen scored 0.27, so this confirms a large gap but is not a paired comparison.

The diagnostic reports 540 Wasm JIT records for the workload. Across module instances it records 23,033 compiled functions out of 1,367,820 functions, 28.0 MB emitted code, and 69.7 s cumulative compilation. The largest one-shot module compiled 2,517 of 2,533 functions, emitting 8.04 MB in 24.7 s. Execution recorded 26.0 million native entry requests and 2.23 million compiled-to-VM reference callouts, with no compiled import callouts. This profile does not isolate cause, but the workload already compiles most functions in its main module; the main remaining leads are generated-code throughput, repeated module compilation, and reference-call boundaries rather than unsupported interpreter functions.

The raw instrumented report is retained here. Instrumented timings are diagnostic only.
