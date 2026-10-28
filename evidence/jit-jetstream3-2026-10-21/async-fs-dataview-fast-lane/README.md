# Async-fs Float64 DataView lane fast path

`File.swapByteOrder` in `generators/async-file-system.js` repeatedly invokes
`DataView.getFloat64` and `setFloat64`. The opt-in diagnostic run recorded
8,204,080 successful fast gets and 8,204,080 successful fast sets. It passed
JetStream output validation.

The path accepts only primitive numeric offsets and values on fixed, ordinary,
non-shared DataViews and ArrayBuffers. It leaves coercions, error cases,
detached/resizable/shared buffers, and other backing stores on the existing
generic native implementation. `QUENCH_JS_JIT_DATAVIEW_FLOAT64=0` disables the
path for same-binary control runs. Diagnostic counters are emitted only when
`QUENCH_JS_JIT_STATS=1` is enabled.

## Three-round screen

| Mode | Quench score | Quench max RSS | Bun 1.4.3 score / RSS | Node 24.19 score / RSS |
| --- | ---: | ---: | ---: | ---: |
| Off | 2.97 | 87.93 MiB | 110.07 / 66.63 MiB | 50.42 / 206.71 MiB |
| On | 3.32 | 87.92 MiB | 110.07 / 66.63 MiB | 50.42 / 206.71 MiB |

The candidate median is 11.8% higher with effectively unchanged RSS. Because
single fixture scores varied widely, five additional interleaved one-round
pairs were collected. The candidate won four of five raw pairs; the median
paired score ratio was 1.329. Dividing each Quench pair ratio by the concurrent
Node pair ratio to account for host variation gives a median ratio of 1.095.
Median Quench score across those five control/candidate samples was 3.04 / 4.04;
RSS stayed between 87.88 and 87.98 MiB. This supports retaining the local fast
path, though Quench remains well behind Bun and Node on async-fs.

The five interleaved pairs were:

| Pair | Off score / RSS | On score / RSS | Quench pair ratio | Node score off / on |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 3.84 / 87.91 MiB | 2.96 / 87.89 MiB | 0.771 | 67.86 / 67.91 |
| 2 | 4.14 / 87.93 MiB | 4.26 / 87.98 MiB | 1.029 | 67.43 / 68.15 |
| 3 | 2.97 / 87.95 MiB | 4.29 / 87.90 MiB | 1.444 | 63.51 / 64.82 |
| 4 | 2.34 / 87.91 MiB | 3.70 / 87.90 MiB | 1.581 | 44.32 / 63.98 |
| 5 | 3.04 / 87.88 MiB | 4.04 / 87.89 MiB | 1.329 | 55.54 / 51.67 |

All reports use the pinned JetStream 3 corpus and passed output validation.
The off wrapper executes the same production binary with only the environment
switch set to zero; the on runs execute that binary directly.

The finalized production build, including diagnostic-counter gating, was
checked once more after the paired screen. `finalized-build-1round.json`
reports Quench 3.72 points / 87.89 MiB, Bun 139.44 / 66 MiB, and Node 56.95 /
207.26 MiB. This confirms valid execution on the final binary; one round under
the shared Linux host quota is not a qualification result.

The fast path is restricted to `ExecutionMode::Compiled`, preserving the
interpreter path. After adding that guard, both
`final-jit-guarded-1round.json` and `final-interpreted-1round.json` passed
JetStream validation. The JIT run measured 2.74 / 88.03 MiB; the interpreted
run measured 3.86 / 87.16 MiB. Peer scores also swung substantially between
these runs (Bun 136.45 vs 39.82), so these single-round reports verify both
modes execute correctly but do not compare performance.

## Reproduction

```sh
cargo build --locked --profile production -p quench-node -p quench-bench

target/production/quench-bench async-fs --suite jetstream3 --jit --jit-diagnostics --runs 1 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --node /opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node \
  --bun /tmp/quench-bun/bin/bun --out diagnostic-1round.json

target/production/quench-bench async-fs --suite jetstream3 --jit --runs 3 \
  --timeout-ms 600000 --quench /tmp/quench-node-no-dataview \
  --node /opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node \
  --bun /tmp/quench-bun/bin/bun --out control-3round.json

target/production/quench-bench async-fs --suite jetstream3 --jit --runs 3 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --node /opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node \
  --bun /tmp/quench-bun/bin/bun --out candidate-3round.json
```

`/tmp/quench-node-no-dataview` is a shell wrapper that runs the measured
`target/production/quench-node` with
`QUENCH_JS_JIT_DATAVIEW_FLOAT64=0`. The interleaved `pair-N-{off,on}.json`
reports in this directory use the same runner and settings.
