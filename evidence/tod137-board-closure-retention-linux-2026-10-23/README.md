# V8-v7 board on v2-gc after the closure-retention merge

Binary `qb9`: v2-gc `d2a77bf4e7`, static. It contains trunk's 20:07 sync, the 2^12 MemoryFirst
young minimum, the HTTP weak-root fix, the region-reservation fix and the closure-environment
trim. It is bolted with a BOLT profile retrained on itself (`train9.sh`, the same corpus as
before; 2,317,859 hot bytes).

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. MemoryFirst, binaries evicted before
each run. One warm-up board ran first, so the compile cache is warm.

`board-5-runs.json`: 5 runs against QuickJS-ng 0.10.1, Node 22.22.0 and Bun 1.4.2. Each
factor is a row's worst (median) max RSS over 0.8 × the lowest other engine's best run. The
previous board is `tod137-board-young-4096-…`.

| Fixture | No JIT | `--jit` | Previous board |
| -- | --: | --: | -- |
| Richards | 1.02 (**0.98**) | **0.98** (0.98) | 1.11 / 1.08 |
| DeltaBlue | **0.97** (0.92) | 1.00 (0.97) | 1.02 / 1.01 |
| Crypto | 1.09 (1.02) | 1.30 (1.30) | 1.16 / 1.39 |
| RayTrace | 1.09 (1.08) | 1.13 (1.12) | 1.24 / 1.15 |
| RegExp | 1.10 (1.07) | 1.09 (1.09) | 1.12 / 1.15 |
| NavierStokes | 1.04 (**0.93**) | 1.80 (1.78) | 1.14 / 1.82 |
| EarleyBoyer | **0.48** | **0.46** | met |
| Splay | **0.64** | **0.64** | met |

Met in every run: EarleyBoyer and Splay in both modes, Richards `--jit`, DeltaBlue without
JIT, and DeltaBlue `--jit` at exactly 1.00. Met in the median run: Richards and NavierStokes
without JIT. Every row improved on the previous board.

What remains:
- Crypto, RayTrace and RegExp: 1.09–1.30.
- NavierStokes `--jit`: 1.80, the Cranelift compiler's scratch memory (TOD-110).
