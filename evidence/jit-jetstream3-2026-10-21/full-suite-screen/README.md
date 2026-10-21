# Full JetStream 3 JIT screen

Command:

```sh
PATH=/tmp/quench-bun/bin:/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin:/tmp/quench-cargo/bin:$PATH \
  target/production/quench-bench --all --suite jetstream3 --jit --runs 1 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --node node --bun bun \
  --checkpoint /tmp/quench-jetstream3-full-jit-v2.checkpoint.json \
  --out /tmp/quench-jetstream3-full-jit-v2.json
```

This was a one-round screening run, not qualification. Thirty three of 34 fixtures produced valid matching output. `argon2-wasm` exceeded the 600,000 ms per-engine timeout for Quench. On the 33 valid fixtures the per-fixture geometric mean Quench score was 0.0846 of Node and 0.0581 of Bun. The score gap is broad: the lowest valid Quench/Node ratios were `sqlite3-wasm` (0.012), `espree-wtb` (0.017), `typescript-lib` (0.026), and `async-fs` (0.027).

One-round memory remains highly workload dependent. Quench RSS is lower than both peers on some fixtures and much higher on allocation-heavy cases: `doxbee-promise` peaked at 1,011,830,784 bytes, `doxbee-async` at 1,052,549,120 bytes, and `async-fs` at 934,285,312 bytes. Its RSS also exceeds both peers for `sqlite3-wasm` and `typescript-lib`. These values are screening observations, not qualification claims.

A focused one-round diagnostics run for `typescript-lib` measured 28,945,747 function entries and 3,925,687 native entries across 2,348 reported functions. There were 89 promotions, 43 successful native compilations, 35.7 ms compile time, and 176,128 generated code bytes. High-cost functions were dominated by closure/call-heavy operations and functions above the 256-instruction cap (`forEach`, `checkSourceElement`, `findSourceFileWorker`, `bindChildren`, `scan`, and checker/type-relation functions). The census counts sites but does not infer redundant guards. This points to missing execution coverage and object/call costs rather than a demonstrated Boolean-guard optimization opportunity.

Raw reports are kept here. The all-suite report includes the timed-out `argon2-wasm` fixture and therefore is incomplete.
