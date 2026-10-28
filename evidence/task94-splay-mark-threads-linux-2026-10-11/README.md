# Splay mark-thread count screen

Date: 2026-10-11. Branch: `v2-cloud`. Source revision: `e2e48259d`.
The same retained bitmap binary was tested with the serial configuration
(`QUENCH_GC_MARK_THREADS=1`, default on this four-vCPU host) and a one-worker
parallel pool (`QUENCH_GC_MARK_THREADS=2`). Binary SHA-256:
`64c7e972a923b7cec339f4d5f9b7f3165f3860b953fbffdf9a0c728ab33ebe7f`.
Wrapper hashes are recorded in `threads-1.sh` and `threads-2.sh`.

## Result

All 11 fixed-work rounds were valid and output-equal. The harness marked 0/11
rounds clean; 14 samples from the two-thread candidate failed the involuntary
context-switch check. The result cannot qualify timing, but the directional
screen is clearly unfavorable:

| Metric | Serial control | Two-thread candidate | Paired candidate minus control |
| --- | ---: | ---: | ---: |
| Median work max RSS | 114,606,080 B | 117,018,624 B | +2,187,264 B, setup-adjusted |
| Median marginal wall | 1,135.63 ms | 1,373.14 ms | +188.33 ms |

The paired wall median is +16.6% of the serial paired marginal median. The
parallel pool also adds memory. The 2-thread setting is rejected, with no
source change and no stock Score run.

## Reproduction

From the repository root with the stage B tools installed:

```sh
source /workspace/quench-build-env.sh
export PATH="$PWD/target/stageb-tools/bun/bun-linux-x64:$PWD/target/stageb-tools/qjs/quickjs-2026-06-04:$PWD/target/stageb-tools/node/node-v26.10.0-linux-x64/bin:$PATH"
target/production/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench evidence/task94-splay-mark-threads-linux-2026-10-11/threads-1.sh \
  --quench-peer evidence/task94-splay-mark-threads-linux-2026-10-11/threads-2.sh \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 --out work/mark-threads-fixed-11.json
```

The report is compressed in `fixed-11.json.gz`; the runner log is
`fixed-11.log`.
