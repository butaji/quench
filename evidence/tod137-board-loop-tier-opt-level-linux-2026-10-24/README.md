# V8-v7 board on v2-gc with the MemoryFirst loop-tier optimization level

Binary `qb9` (rebuilt): v2-gc `0a6d6cbc06`, static, bolted with a BOLT profile retrained on
itself (`train9.sh`; 2,324,035 hot bytes). It is the previous board's tree plus
`LOOP_TIER_OPT_LEVELS`.

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. MemoryFirst, binaries evicted before each
run. One warm-up board ran first, so the compile cache was warm. The board ran 5 times against
QuickJS-ng 0.10.1, Node 22.22.0 and Bun 1.4.2.

`board-5-runs.txt` holds min / median / max KiB per engine. The harness could not write its
JSON: a file from the previous board was already at the output path. Each factor is a row's
worst run, with its median in brackets, over 0.8 × the lowest other engine's best run.

| Fixture | No JIT | `--jit` | Previous board (`tod137-board-closure-retention-…`) |
| -- | --: | --: | -- |
| Richards | 1.03 (**0.97**) | **0.97** (0.96) | 1.02 / 0.98 |
| DeltaBlue | **0.97** (0.92) | **0.99** (0.97) | 0.97 / 1.00 |
| Crypto | 1.13 (1.04) | 1.26 (1.22) | 1.09 / 1.30 |
| RayTrace | 1.08 (1.02) | 1.07 (1.06) | 1.09 / 1.13 |
| RegExp | 1.09 (1.07) | 1.12 (1.08) | 1.10 / 1.09 |
| NavierStokes | 1.06 (**0.94**) | 1.71 (1.69) | 1.04 / 1.80 |
| EarleyBoyer | **0.48** | **0.46** | met |
| Splay | **0.65** | **0.64** | met |

Absolute Quench medians against the previous board:
- Crypto `--jit`: 3852 → 3588 KiB.
- NavierStokes `--jit`: 7492 → 7056 KiB.
- RayTrace `--jit`: 3064 → 2964 KiB.

Every no-JIT median moved by 104 KiB or less, the bolt-to-bolt layout noise; the no-JIT rows
do not use the loop tier.

Met in every run: EarleyBoyer and Splay in both modes, Richards `--jit`, DeltaBlue in both
modes. Met in the median run: Richards and NavierStokes without JIT.
