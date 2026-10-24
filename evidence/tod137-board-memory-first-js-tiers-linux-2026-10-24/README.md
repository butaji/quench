# V8-v7 board with MemoryFirst compiling no JavaScript native code

Binary `qb9` (rebuilt): v2-gc `d7f732f66d`, static, bolted with a BOLT profile retrained on
itself. It includes the 12:07 sync and `JS_FUNCTION_TIERS`. Under MemoryFirst no loop region,
general-dispatch function or per-function tier compiles.

Host: Linux 6.18 x86_64 VM, 4 CPUs. MemoryFirst, evicted, one warm-up board, then 5 runs
against QuickJS-ng 0.10.1, Node 22.22.0 and Bun 1.4.2. Each factor is the worst run, median
in brackets, over 0.8 × the lowest other engine:

| Fixture | No JIT | `--jit` | Previous (`tod137-board-general-dispatch-gated-…`) |
| -- | --: | --: | -- |
| Richards | **0.97** (0.85) | **0.90** (0.90) | 0.99 / 1.06 |
| DeltaBlue | **0.89** (0.89) | **0.93** (0.89) | 0.97 / 1.07 |
| Crypto | 1.03 (**0.97**) | 1.02 (1.01) | 1.09 / 1.17 |
| RayTrace | 1.05 (**0.98**) | 1.01 (**0.98**) | 1.12 / 1.10 |
| RegExp | 1.04 (1.03) | 1.08 (1.05) | 1.12 / 1.12 |
| NavierStokes | **0.93** (0.93) | **0.93** (0.93) | 0.98 / 1.05 |
| EarleyBoyer | **0.45** | **0.46** | met |
| Splay | **0.64** | **0.64** | met |

- **Met in every run:** 11 of 16 rows, among them every `--jit` row but Crypto, RayTrace and
  RegExp.
- **Met in the median run:** 13 of 16. Only Crypto `--jit` (1.01) and RegExp in both modes
  (1.03 and 1.05) miss there.
- **Still missing on the worst run, by 1–8%:** Crypto (1.03 / 1.02), RayTrace (1.05 / 1.01)
  and RegExp (1.04 / 1.08).
- **No-JIT rows also improved** against the previous board. Part of that is the 12:07 sync;
  part is variance between BOLT retrains, as `tod137-board-general-dispatch-gated-…` showed.
