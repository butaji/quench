# Splay common-cell trace fast path

Date: 2026-10-11. Branch: `v2-cloud`. Source base: `faf31d099`.
Rust: `1.99.0 (b940084d7 2026-09-28)`. The candidate adds early `Cell::Object`
and `Cell::Array` branches in `Heap::children` to avoid the typed-array check
and general cell-kind dispatch for Splay's common cells. The compressed source
patch is `candidate.patch.gz`.

Control binary SHA-256:
`64c7e972a923b7cec339f4d5f9b7f3165f3860b953fbffdf9a0c728ab33ebe7f`.
Candidate binary SHA-256:
`52c292de4c646a286c1a72901d46b67b6cc878fa50f42700c5e7fdc862d09ca0`.

## Result

The fixed-work Splay report was valid and output-equal in 11/11 rounds, but
0/11 rounds were clean under the contention detector. Two samples for each
engine failed the involuntary-context-switch check. The result cannot qualify
timing; its paired direction also gives no reason to retain this common-kind
branch:

| Metric | Control | Candidate | Paired candidate minus control |
| --- | ---: | ---: | ---: |
| Work max RSS median | 114,626,560 B | 114,634,752 B | +12,288 B setup-adjusted |
| Marginal wall median | 870.99 ms | 985.59 ms | +77.91 ms |

Ten of eleven paired marginal times were slower with the candidate. This
collector source change was reverted; no stock Score run was justified.

The candidate passed `RUSTFLAGS='-D warnings' cargo +1.99.0 check --locked
--offline -p quench-runtime` and the optimized production build. Build output is
in `production-build.log`. Compressed raw report and runner log are
`fixed-11.json.gz` and `fixed-11.log`.

## Reproduction

From the repository root:

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' cargo +1.99.0 build --locked --offline \
  --profile production -p quench-node --bin quench-node
cp target/production/quench-node work/common-trace-candidate
export PATH="$PWD/target/stageb-tools/bun/bun-linux-x64:$PWD/target/stageb-tools/qjs/quickjs-2026-06-04:$PWD/target/stageb-tools/node/node-v26.10.0-linux-x64/bin:$PATH"
target/production/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench work/task94-v2-cloud-slotgen-free-bitmap-candidate \
  --quench-peer work/common-trace-candidate \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 --out work/common-trace-fixed-11.json
```

Current stock Splay bars remain open: the retained bitmap candidate is Score
2,736 vs Node `--jitless` at 5,433 and max RSS 114,819,072 B vs Bun no-JIT at
62,672,896 B.
