# V8-v7 board on v2-gc with the MemoryFirst young minimum at 2^12

Binary `qb8`: v2-gc `4829abf4b9`, static. It contains trunk's 12:07 sync, the HTTP weak-root
fix and the 2^12 young minimum. It is bolted with a BOLT profile retrained on itself
(`train8.sh`, the same corpus as `bolt7`; 2,226,630 hot bytes).

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. MemoryFirst, binaries evicted before each
run. QuickJS-ng 0.10.1.

- `board-3-runs-all-engines.json`: the full board against QuickJS, Node 22.22.0 and Bun 1.4.2,
  3 runs. Its first Quench sample (Richards, 6860 KiB) ran with a cold compile cache for
  this new binary; every other sample ran warm.
- `board-5-runs-qjs.json`: Quench and QuickJS only, 5 runs, compile cache warm.
- `gaps.txt`, computed from the 5-run board: each row's worst and median sample over
  0.8 × QuickJS's best. The `old` column is the morning board
  (`tod137-board-after-trunk-merge-…`, worst of 3). Splay's bar is Bun no-JIT's 61.4 MB,
  so its factor there is 0.64, as in `board-3-runs-all-engines.json`.

| Fixture | No JIT, worst (median) | `--jit`, worst (median) | Morning board |
| -- | --: | --: | -- |
| Richards | 1.11 (1.03) | 1.08 (1.08) | 1.18 / 1.21 |
| DeltaBlue | 1.02 (**0.99**) | 1.01 (1.01) | 1.10 / 1.06 |
| Crypto | 1.16 (1.16) | 1.39 (1.38) | 1.25 / 1.34 |
| RayTrace | 1.24 (1.14) | 1.15 (1.14) | 1.35 / 1.30 |
| RegExp | 1.12 (1.11) | 1.15 (1.15) | 1.19 / 1.16 |
| NavierStokes | 1.14 (**1.00**) | 1.82 (1.80) | 1.21 / 1.86 |
| EarleyBoyer | met | met | met |
| Splay | met | met | met |

Every interpreted row improved, and four of six `--jit` rows did. DeltaBlue and
NavierStokes (no JIT) reach the target in their median run. Crypto `--jit` is within this
board's ±200 KiB worst-sample band of the morning's 1.34.
