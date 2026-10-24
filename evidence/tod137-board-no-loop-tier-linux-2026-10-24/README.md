# V8-v7 board on v2-gc with MemoryFirst compiling no loop regions

Binary `qb9` (rebuilt): v2-gc `d87c06f3e7`, static. That tree includes:
- c2's MemoryFirst "no loop regions" table (`1d4f1953f`);
- the closure-trace cost bound (`17e0bdfa19`).

It is bolted with a BOLT profile retrained on itself (`train9.sh`; 2,325,821 hot bytes).

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. MemoryFirst, binaries evicted before each
run, one warm-up board first. The board ran 5 times against QuickJS-ng 0.10.1, Node 22.22.0
and Bun 1.4.2.

Each factor is a row's worst run, with its median in brackets, over 0.8 × the lowest other
engine's best run.

| Fixture | No JIT | `--jit` | Previous board (`tod137-board-loop-tier-opt-level-…`) |
| -- | --: | --: | -- |
| Richards | 1.06 (**0.99**) | **0.99** (0.99) | 1.03 / 0.97 |
| DeltaBlue | **0.99** (0.92) | 1.00 (0.98) | 0.97 / 0.99 |
| Crypto | 1.10 (**0.99**) | 1.07 (1.06) | 1.13 / 1.26 |
| RayTrace | 1.07 (**0.97**) | 1.13 (1.06) | 1.08 / 1.07 |
| RegExp | 1.07 (1.05) | 1.09 (1.07) | 1.09 / 1.12 |
| NavierStokes | 1.05 (**0.94**) | 1.01 (**1.00**) | 1.06 / 1.71 |
| EarleyBoyer | **0.49** | **0.47** | met |
| Splay | **0.64** | **0.64** | met |

- **Medians:** they meet the target on 11 of 16 rows. They miss only on Crypto `--jit` (1.06),
  RayTrace `--jit` (1.06), RegExp in both modes (1.05 and 1.07), and DeltaBlue `--jit` (exactly
  1.00).
- **NavierStokes `--jit`:** 1.71 → 1.01.
- **Crypto `--jit`:** 1.26 → 1.07.

**Why worst and median differ.** The harness writes each fixture to a fresh temporary
directory per invocation. The compile cache is keyed by path, so the first of the five runs
compiles the fixture cold, even after the warm-up board. That run is the row's worst on
DeltaBlue, Crypto, RayTrace, EarleyBoyer and NavierStokes without JIT.

A matched cold/warm probe (`coldwarm.py`, 5 pairs, same path) prices a cold compile at:
- no JIT: +136 to +492 KiB;
- `--jit`: +40 to +244 KiB.
