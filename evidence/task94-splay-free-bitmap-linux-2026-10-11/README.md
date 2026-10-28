# Splay per-slab free bitmap screen — Linux, 2026-10-11

## Decision

Retain the fixed per-slab free bitmap as an incremental collector memory
reduction. Splay is still unwon: this Linux candidate does not meet either the
Node Score or Bun maximum-RSS bar, and it is not an M4 qualification result.

The bitmap replaces each slab's dynamic `Vec<u16>` free-offset payload with
32 `u64` words, a summary of nonempty words, and a free count. Allocation pops
the highest free offset to preserve the prior LIFO reuse order. Parallel sweep
merges disjoint slab bitmaps by reference, avoiding a 264-byte bitmap copy.

## Splay results

The fixed-work comparison used 300 iterations per sample and was valid and
output-equal in 11/11 pairs. The Linux runner had 0/11 clean hardware-counter
rounds, so timing is diagnostic only.

| Metric | Control | Bitmap candidate | Change |
| --- | ---: | ---: | ---: |
| Work max RSS median | 117,465,088 B | 114,589,696 B | −2,875,392 B |
| Setup-only max RSS median | 63,762,432 B | 63,610,880 B | −151,552 B |
| Setup-adjusted max RSS paired median delta | — | — | −2,637,824 B |
| Marginal wall median | 980.64 ms | 903.29 ms | — |
| Paired marginal wall delta median | — | — | −24.89 ms (−2.54% of control median) |

Marginal wall won 7/11 pairs. Its paired bootstrap 95% interval was −327.81 ms
to +17.87 ms; the interval crosses zero and counters were unavailable, so no
speed claim is made.

Separate 11-round stock sessions were valid and output-equal. The control scored
2,745 vs Node at 5,562 and used 117,489,664 B vs Bun at 62,537,728 B. The bitmap
candidate scored 2,736 vs Node at 5,433 and used 114,819,072 B vs Bun at
62,672,896 B. Because the sessions are separate and Node's median moved, the
9-point Quench score difference is not causal. Candidate RSS is 1.83× Bun and
52,146,176 B over its median. Candidate Score is 50.4% of Node; both bars remain
open by a wide margin.

## Correctness

- Focused heap tests after the no-copy merge adjustment: 38/38 passed.
- Full `quench-runtime --lib`: 553 passed, 1 failed. The only failure is the
  existing German date-format expectation (`2.1.1970` vs `1.1.1970`), unrelated
  to the heap allocator.
- Test262 stage 47 (FinalizationRegistry): 47/47 passed.
- Test262 stages 79–81 (WeakMap, WeakRef, WeakSet): 255/255 passed.
- Rust 1.99 production build passed with repository warnings denied. The final
  fixed-work and stock reports each matched output in 11/11 rounds.
- No JS-visible behavior, write barrier, shared store path, or Wasm path changed.

## Build and measurement details

- Source base: `900927e210f6a399e38418a1819620a9d81395d0`.
- Control binary SHA-256:
  `41864b25fe13e5e254e8098d2068d7c9a5d990a08027f30c3befbaab683e297f`.
- Candidate binary SHA-256:
  `64c7e972a923b7cec339f4d5f9b7f3165f3860b953fbffdf9a0c728ab33ebe7f`.
- Fixture SHA-256:
  `1f875cec579d03af40d256ceaeb6823f3ef16c883652650ca51d183472a8185d`.
- Runner SHA-256:
  `0c553c518dcdd9d57ed0900ad4eb494804eb844675ba0942907cca4cf15c7d20`.
- Rust: `1.99.0 (b940084d7 2026-09-28)`.
- Host: Linux x86_64, kernel `6.18.44`, 4 logical CPUs / CPU quota 4, 16 GiB
  memory limit; RSS via Linux `wait4`.
- Node 26.10.0 SHA-256:
  `ab9c8eecf9f82d6693cdc3accced17034065c8d96213b0aa76a7e803d20ae1da`.
- Bun 1.4.2 SHA-256:
  `a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c`.
- QuickJS 2026-06-04 SHA-256:
  `c798e32aac14495856a4709be67a11b0562a6e2d07fa89ad4f28efe46c7c7955`.

Compressed fixed-work, candidate stock, and control stock reports are included.
Their SHA-256 digests are:

- `candidate.patch.gz`: `5fb68af8514f397183e2baa2daa57e5287e71b03c564b6a07327cf417ab7e0fe`
- `fixed-work-11.json.gz`: `a4c0b1eabf112dbbb16bcdf53e6b603b5139e866116ce58563c385f8ad3460bc`
- `stock-candidate-11.json.gz`: `5f45c58bbaa7fa6095670fa96408004a2d78bd4ba1302c757a61a6c45b9a33b4`
- `stock-control-11.json.gz`: `0b9446c211fa6944e9fe24e81484e5092f67805cac445401e1793281d5750a23`

## Reproduction

From the source base above, build and save the control before applying the
compressed patch, then build the candidate:

```sh
source /workspace/quench-build-env.sh
cargo +1.99.0 build --locked --offline --profile production -p quench-node --bin quench-node
cp target/production/quench-node work/task94-free-bitmap-control
gzip -dc evidence/task94-splay-free-bitmap-linux-2026-10-11/candidate.patch.gz | git apply -
cargo +1.99.0 build --locked --offline --profile production -p quench-node --bin quench-node
cp target/production/quench-node work/task94-v2-cloud-slotgen-free-bitmap-candidate
```

Run the paired fixed-work screen:

```sh
source /workspace/quench-build-env.sh
export PATH="$PWD/target/stageb-tools/bun/bun-linux-x64:$PWD/target/stageb-tools/qjs/quickjs-2026-06-04:$PWD/target/stageb-tools/node/node-v26.10.0-linux-x64/bin:$PATH"
target/production/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench work/task94-free-bitmap-control \
  --quench-peer work/task94-v2-cloud-slotgen-free-bitmap-candidate \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 \
  --out work/task94-free-bitmap-fixed-11.json
```

The two stock reports use the same command without `--fixed-work` and
`--quench-peer`, once with each Quench executable as `--quench`.
