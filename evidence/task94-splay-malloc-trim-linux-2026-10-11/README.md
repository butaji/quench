# Splay glibc trim screen

Date: 2026-10-11. Branch: `v2-cloud`. Base commit: `9d6b9e59e`.

## Candidate

Call glibc `malloc_trim(0)` after each full collection on Linux GNU. The build
was Rust `1.99.0 (b940084d7 2026-09-28)`. The candidate binary SHA-256 is
`f53196d6192193c7e42aa38e942ef38e096b4eeac573abf8e5699b36f073042f`; its
bitmap-only paired baseline is
`64c7e972a923b7cec339f4d5f9b7f3165f3860b953fbffdf9a0c728ab33ebe7f`.
The source patch is compressed in `candidate.patch.gz`.

## Result

Both 11-pair fixed-work Splay screens were valid and output-equal in all 11
rounds, but both had 0/11 clean rounds under the benchmark contention detector.
The candidate did not pass the timing qualification gate.

| Screen | Bitmap work max RSS median | Trim work max RSS median | Paired setup-adjusted RSS delta | Paired marginal wall delta |
| --- | ---: | ---: | ---: | ---: |
| Initial | 114,565,120 B | 114,589,696 B | -249,856 B | -76.58 ms |
| Retest | 114,622,464 B | 114,524,160 B | -192,512 B | -28.35 ms |

The two comparisons suggest at most about 0.2 MB lower setup-adjusted RSS,
while the prior stock bitmap candidate remains 52,146,176 B above Bun's RSS bar.
The observed raw timing deltas are not qualified. This Linux/glibc-specific
change was rejected; it does not address Task 94's cross-platform Splay gates.
No stock Score run was used to qualify the candidate.

The candidate passed Rust 1.99 checks for `quench-runtime` with and without
`profile-memory`, the 38 focused heap tests, and the optimized production
build. The trim source change was reverted after this screen.

## Reproduction

From the repository root, with the benchmark dependencies already installed:

```sh
source /workspace/quench-build-env.sh
export PATH="$PWD/target/stageb-tools/bun/bun-linux-x64:$PWD/target/stageb-tools/qjs/quickjs-2026-06-04:$PWD/target/stageb-tools/node/node-v26.10.0-linux-x64/bin:$PATH"
target/production/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench work/task94-v2-cloud-slotgen-free-bitmap-candidate \
  --quench-peer work/task94-v2-cloud-malloc-trim-candidate \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 --out work/trim-fixed-11.json
```

Compressed raw reports and logs: `fixed-11-contended.*` and
`fixed-11-retest.*`. The second report retained the same 0/11 clean result.
