# Reverted-source package hot-PC screen

One-round JIT instrumentation run on the existing production control binary;
no build was started. Binary SHA-256:
`3010c9f68840ad8c451a8c487754d099de3ba2ff9d82823258027772f9c997ef`.
Its `crates/quench-runtime/src/vm/compiled/osr_loop.rs` is byte-for-byte the
current checkout's file. Host: Linux x86_64, 5 CPUs; Node 24.19.0; Bun 1.4.4.
The workload runner checked observable output parity. Instrumented one-round
timings and sampled RSS are diagnostics only; no performance or qualification
claim is made.

## Findings

| Region | Activity on reverted-source control | Missing prerequisite / result |
| --- | --- | --- |
| Pino `lib/tools.js::_asJson` | 13 lane entries; 5/5 backward sites admitted; 10,476 `Call`, 5,236 `Binary`, 3 `Jump` general-dispatch ops; 81.1 ms inclusive interpreter time | 0 compile attempts and 0 native region runs despite admitted sites |
| Zod `core/memoizer.cjs::isRecursive` | 8 lane entries; 13 backward sites | All 13 OSR analyses reject at `LoadEnvLocal`; 0 native runs |
| Zod `core/util.cjs::mergeDefs` | 6 lane entries; 2 backward sites admitted | 0 compile attempts and 0 native runs |

Inclusive timing is not exclusive CPU and must not be summed. The counters
justify shared-JIT owner review of the admission/compile decision for the
already-admitted Pino and `mergeDefs` sites, plus generic `LoadEnvLocal`
support for `isRecursive`. They do not justify package recognizers or another
package benchmark matrix yet.

## Reproduction

```sh
QUENCH_NODE_BIN=/workspace/quench/target/production/quench-node \
  BUN_BIN=/tmp/quench-bun/bun-linux-x64/bun \
  QUENCH_JS_JIT_STATS=1 \
  node tests/npm-perf/run-workload.mjs pino-json-logger jit 1 \
  tests/npm-perf/reports/reverted-prefix-hotpc-2026-10-25/pino-jit-stats-1.json

QUENCH_NODE_BIN=/workspace/quench/target/production/quench-node \
  BUN_BIN=/tmp/quench-bun/bun-linux-x64/bun \
  QUENCH_JS_JIT_STATS=1 \
  node tests/npm-perf/run-workload.mjs zod-parse jit 1 \
  tests/npm-perf/reports/reverted-prefix-hotpc-2026-10-25/zod-jit-stats-1.json
```

Raw JSON reports are in
[`tests/npm-perf/reports/reverted-prefix-hotpc-2026-10-25/`](../../tests/npm-perf/reports/reverted-prefix-hotpc-2026-10-25/).
The top-level ten-package goal remains open: Linear's current clean-PGO Linux
11-round matrix is 0/22 numerical passes and sampled RSS is diagnostic; no M4
qualification is inferred.
