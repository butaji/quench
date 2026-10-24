# V8-v7 board with MemoryFirst compiling no general-dispatch functions

Binary `qb9` (rebuilt): v2-gc `8e5cc4fb83`, static, bolted with a BOLT profile retrained on
itself. It contains the general-dispatch gate (`3ed4c736fe`), the cold-entry change and the
06:07 sync.

Host: Linux 6.18 x86_64 VM, 4 CPUs. MemoryFirst, evicted, one warm-up board, then 5 runs
against QuickJS-ng, Node and Bun. Each factor is the worst run, median in brackets, over
0.8 × the lowest other engine:

| Fixture | No JIT | `--jit` | Previous (`tod137-general-dispatch-memory-first-…`) |
| -- | --: | --: | -- |
| Richards | **0.99** (0.95) | 1.06 (1.02) | 1.00 / 1.07 |
| DeltaBlue | **0.97** (0.94) | 1.07 (1.02) | 0.99 / 1.03 |
| Crypto | 1.09 (1.06) | 1.17 (1.16) | 1.09 / **1.86** |
| RayTrace | 1.12 (1.03) | 1.10 (1.10) | 1.13 / 1.14 |
| RegExp | 1.12 (1.08) | 1.12 (1.10) | 1.13 / 1.13 |
| NavierStokes | **0.98** (0.97) | 1.05 (1.04) | 1.00 / 1.05 |
| EarleyBoyer | **0.46** | **0.47** | met |
| Splay | **0.64** | **0.64** | met |

- **Met in every run:** five no-JIT rows (Richards, DeltaBlue, NavierStokes, EarleyBoyer, Splay)
  and two `--jit` rows (EarleyBoyer, Splay).
- **Crypto `--jit`:** 5496 → 3388 KiB median.
- **`--jit` against no-JIT:** under MemoryFirst, every `--jit` row except RayTrace now peaks
  above the same row without JIT. The per-function tier's code and compile memory are what
  remain; the loop and general-dispatch tiers compile nothing under MemoryFirst.
