# V8-v7 no-JIT board (Linux x86-64, 2026-10-24)

Host: 4-vCPU Intel Xeon (Sapphire Rapids) cloud VM, 15 GiB RAM, no PMU counters.

Engines:
- Quench: v2-cloud-c at 0189c87305, built with `tools/pgo-build.sh profiling`. The profiling profile is release codegen with symbols, so `tools/check-lane-tail-calls.sh` can read the binary. It lists only frame transitions, slow forms and Wasm handlers.
- References: `node --jitless` and `BUN_JSC_useJIT=0 bun`.

Measurement: concurrent pairs, each run pinned to CPU 1 or CPU 2 (swapped every round), 9 rounds. A ratio is the median of Quench Score / reference Score. The raw output is in `board.txt`.

| Fixture | vs Node --jitless | vs Bun JIT off |
|---|---:|---:|
| Richards | 1.41 | 1.23 |
| DeltaBlue | 1.25 | 1.34 |
| Crypto | 2.42 | 1.74 |
| RayTrace | 1.40 | 1.75 |
| EarleyBoyer | 1.19 (15-pair rerun: 1.36, 15/15) | 1.62 |
| RegExp | 1.33 | 5.70 |
| Splay | 2.18 | 2.84 |
| NavierStokes | 1.71 | 1.24 |

Quench won every pair. EarleyBoyer against Node read 1.19 in the 9-pair board. A 15-pair rerun of the same binaries gave 1.36 with all 15 won, and the same binary against the previous head won 8 of 9 pairs at 1.086.

## After the trunk merge (df7e05eea9)

This build is from `df7e05eea9`: `origin/v2-merged` f98921070 merged in and the
lane's slot layout re-applied. It was built with `tools/pgo-build.sh profiling`
at `CARGO_BUILD_JOBS=2`, because the default job count hit the shell's memory
cgroup limit. The same protocol gives these results (`board-df7e05eea.txt`):

| Fixture | vs Node --jitless | vs Bun JIT off |
|---|---:|---:|
| Richards | 1.47 | 1.22 (8/9) |
| DeltaBlue | 1.29 | 1.49 |
| Crypto | 2.34 | 1.81 |
| RayTrace | 1.34 | 1.70 |
| EarleyBoyer | 1.31 | 1.63 |
| RegExp | 1.32 | 5.44 |
| Splay | 1.91 | 2.87 |
| NavierStokes | 1.68 | 1.27 |

Compared with the 0189c87305 build in 5 concurrent pairs
(`merge-df7e05eea-vs-0189c87305.txt`):
- DeltaBlue is 15% faster, Splay 14% and EarleyBoyer 5%.
- The other fixtures are within ±3%.

All 136 Node oracle scripts match. The runtime lib tests pass, 916 of 916.

`tools/check-lane-tail-calls.sh` now also lists three trunk handlers:
`lane_frame_construct`, `lane_push_frame_call` and `lane_tier_prologue`.
