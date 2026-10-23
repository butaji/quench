# MemoryFirst young minimum 2^12 instead of 2^13 — kept

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB.

Binary: v2-gc `27bf0218cf`, static, built with `quench-runtime/gc-tuning` and bolted with
the `bolt7` profile. The profile was trained on another binary, so absolute numbers sit
above the board's. Every arm runs this same binary. Only `QUENCH_GC_MIN_YOUNG` changes, so
code layout is not a confound.

Driver: `ysweep.py BINARY RUNS`. It runs under `QUENCH_MEMORY_POLICY=memory-first`, evicts the
binary before each run, interleaves the arms in alternating order, and uses the stock
V8-v7 runner over `base.js`.

## Sweep: 2^13, 2^12, 2^11, 2^10, 5 runs (`sweep-4-values-5-runs.txt`)

2^12 lowered max RSS on most rows. 2^11 and 2^10 cost much more Score for little more:
RayTrace ×0.65 and ×0.49.

## Confirmation: 2^13 against 2^12, 9 runs (`confirm-8192-4096-9-runs.txt`)

Medians, max RSS in KiB:

| Fixture | 2^13 | 2^12 | Score 2^12 / 2^13 |
| -- | --: | --: | --: |
| Richards | 3280 | 2864 | 1.09 |
| Richards `--jit` | 3408 | 3116 | 1.01 |
| DeltaBlue | 3316 | 3164 | 0.86 |
| DeltaBlue `--jit` | 3548 | 3292 | 0.88 |
| Crypto | 3688 | 3456 | 0.97 |
| Crypto `--jit` | 3920 | 3844 | 0.98 |
| RayTrace | 3448 | 3228 | 0.86 |
| RayTrace `--jit` | 3500 | 3260 | 0.90 |
| RegExp | 6516 | 6212 | 0.86 |
| RegExp `--jit` | 6596 | 6260 | 0.88 |
| EarleyBoyer | 6860 | 6668 | 0.91 |
| EarleyBoyer `--jit` | 6928 | 6768 | 0.92 |
| Splay | 31552 | 31560 | 1.13 |
| Splay `--jit` | 31588 | 31596 | 1.04 |
| NavierStokes | 4524 | 4188 | 0.97 |
| NavierStokes `--jit` | 7468 | 7460 | 1.01 |

Median max RSS falls on 14 of 16 rows, by 76–416 KiB; Splay is flat (+8 KiB) and
NavierStokes `--jit` moves 8 KiB. The worst run is lower on every row. MemoryFirst's table weighs RSS alone, and the policy is opt-in
(Throughput is the default), so the Score cost is accepted.

At 2^13, the TOD-167 sweep (2026-10-22) had measured 2^12 at Score ×0.89 and ×0.94 of
Throughput's. Since then, the first-call release and the lazy memo tables have lowered the
fixed RSS, so the young generation is a larger share of each peak.
