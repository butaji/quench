# Current-head JetStream hash-map screen

Host: Linux x86_64, 4 vCPU, 16 GiB memory cgroup. Source: `v2-jit` at
`072a338621ebfe8d9caf4da4c3ad44df9528c450`. Production binaries were built with
`CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo build --locked --profile production -p quench-node -p quench-bench`.
JetStream corpus: `06785cf861ac44855f168cbbe829278c2802e6de`.

These are one-round fixture screens, not qualification evidence. The production
no-JIT and clean JIT runs both validated output. Exact reports are preserved in
this directory. Comparison binaries were Node v26.10.0 and Bun 1.4.2.

| Mode | Quench Score | Bun Score | Node Score | Quench/Bun | Quench/Node | Quench max RSS |
| -- | --: | --: | --: | --: | --: | --: |
| no-JIT | 12.27 | 12.69 | 10.36 (`--jitless`) | 0.967x | 1.184x | 36.6 MiB |
| JIT | 7.43 | 203.76 | 185.45 | 0.036x | 0.040x | 40.3 MiB |

The JIT RSS was below Bun (125.7 MiB) and Node (192.0 MiB); no-JIT RSS was
below Bun (45.7 MiB) and Node (189.6 MiB). The JIT score is below Quench's
no-JIT score on this fixture. One round is not sufficient to infer a stable
effect or claim the 1.2x target.

The separate diagnostic run added `--jit-diagnostics`, so its score is not
used for comparison. It reports `_findKeyEntry` taking the numeric-find helper
for 53,999,968 of 54 million entries and `equals` with 53,999,969 native
entries. The hot `runIteration` wrapper did not compile; its five instructions
include `LoadNameCall` and `Call`. The diagnostic motivates general
effect-aware call lowering with exact VM continuation/fallback, but does not
predict its performance.

The iteration-profile diagnostic from this commit timed out after 600 seconds
and was discarded. The production diagnostic is valid for coverage counts and
JetStream output, but only the non-diagnostic production report is treated as
the clean JIT screen above.

## Reproduction

```sh
target/production/quench-bench --fixtures hash-map --suite jetstream3 --runs 1 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out target/current-hash-map-nojit.json

target/production/quench-bench --fixtures hash-map --suite jetstream3 --jit --runs 1 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out target/current-hash-map-jit-clean.json

target/production/quench-bench --fixtures hash-map --suite jetstream3 --jit \
  --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out target/current-hash-map-jit-production-diagnostic.json
```
