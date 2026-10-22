# Splay serial mark-bit screen

Date: 2026-10-11. Branch: `v2-cloud`. Source base: `9f1abb146`.
Rust: `1.99.0 (b940084d7 2026-09-28)`. The candidate uses
`AtomicU64::get_mut()` to read and update mark words in the exclusive serial
stop-the-world marker. The parallel marker still uses atomic operations. The
source patch is compressed in `candidate.patch.gz`.

Control binary SHA-256:
`64c7e972a923b7cec339f4d5f9b7f3165f3860b953fbffdf9a0c728ab33ebe7f`.
Candidate binary SHA-256:
`fa642ab8b93cb519759904238309da64974c5cbb2c92c9f4f71e714fab57b660`.

## Result

Both paired fixed-work Splay runs were valid and output-equal 11/11, but each
had 0/11 clean rounds under the Linux runner's qualification checks. The first
paired marginal-wall median was +16.0 ms, the repeat was −64.7 ms, and the
combined 22-pair median was −34.4 ms (13/22 candidate wins). A 50,000-resample
bootstrap 95% interval was −123.6 to +51.9 ms, crossing zero. Paired
setup-adjusted RSS was −368,640 B in the first screen and +172,032 B in the
repeat; the combined median was +90,112 B, with 8/22 lower-RSS pairs.

The timing signal is inconclusive and the RSS did not improve consistently, so
the source candidate was rejected. It was not stock-scored.

## Correctness and build

`RUSTFLAGS='-D warnings' cargo +1.99.0 check --locked --offline
-p quench-runtime` passed. The focused heap suite passed 30/30 tests, including
weak references, ephemerons, minor tracing, and the parallel mark verifier.
The optimized production build passed. Logs are `production-build.log` and the
two fixed-work `.log` files; compressed raw reports are `fixed-11.json.gz`
and `repeat-11.json.gz`.

## Reproduction

From the repository root:

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' cargo +1.99.0 build --locked --offline \
  --profile production -p quench-node --bin quench-node
cp target/production/quench-node work/serial-mark-bit-candidate
export PATH="$PWD/target/stageb-tools/bun/bun-linux-x64:$PWD/target/stageb-tools/qjs/quickjs-2026-06-04:$PWD/target/stageb-tools/node/node-v26.10.0-linux-x64/bin:$PATH"
target/production/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench work/task94-v2-cloud-slotgen-free-bitmap-candidate \
  --quench-peer work/serial-mark-bit-candidate \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 --out work/serial-mark-bit-fixed-11.json
```

Run the command again with a different output path for the repeat. The retained
stock bitmap candidate remains below the Score and RSS bars: 2,736 vs Node
`--jitless` at 5,433 and 114,819,072 B vs Bun no-JIT at 62,672,896 B.
