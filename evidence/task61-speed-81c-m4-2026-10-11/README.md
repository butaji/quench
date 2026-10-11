# Speed-lane integration gate on M4

This evidence covers `81c5147270befc131c7d7df28766c772783d916b`, the Splay speed-lane head integrated into the trunk. It is a separate gate from the uncommitted ObjectRecord candidate.

## M4 stock Splay result

The stock V8-v7 harness completed 11 valid rounds with matching output on a Mac16,10 (arm64, Darwin 25.5, 10 logical CPUs, 16 GiB). Quench used the interpreter; references were QuickJS 2026-06-04, Bun 1.4.2 with `BUN_JSC_useJIT=0`, and Node v26.10.0 with `--jitless`.

| Engine | Median Score | Median maximum RSS |
| --- | ---: | ---: |
| Quench | 7,500 | 116,670,464 B (111.27 MiB) |
| Node `--jitless` | 14,913 | 406,781,952 B (387.94 MiB) |
| Bun no-JIT | 9,999 | 62,062,592 B (59.19 MiB) |
| QuickJS | 8,687 | 151,060,480 B (144.06 MiB) |

The best reference stock Score is Node's 14,913, so Quench is **1.99× behind**. The lowest reference RSS is Bun's 59.19 MiB, so Quench is **1.88× above**. Splay has not won either bar. The stock rounds' maximum recorded one-minute host load was 0.419 per logical CPU.

## Fixed-work and correctness gates

- Splay fixed-work, 11 pairs: marginal wall 1,134,287.5 → 1,071,675.7 ns/run (−5.52%); cycles −5.49%; instructions −5.56%; maximum RSS +0.042%. Output matched.
- All-eight fixed-work guard: passed. Every fixture was classified with at least seven clean pairs; Splay had 10 clean of 11 valid pairs. No detected cycle regression; instruction and RSS budgets passed on all eight.
- `quench-runtime` unit tests: 544 passed.
- WAST: 67,124 passed, 0 failed.

The detailed raw reports and resumable guard decision are in this directory. The stock run is the qualification metric; fixed-work is the paired Quench candidate gate.
