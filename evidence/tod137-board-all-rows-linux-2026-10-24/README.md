# V8-v7 max RSS board: every row at or below 0.8 × the best other engine

**Build:**
- **Source:** v2-gc `7f46508374` as a static production build.
- **Profiles:** BOLT profiles retrained on that binary (callgrind on the deegen curriculum,
  micros and test262 order corpus).
- **Layout:** `llvm-bolt -reorder-blocks=ext-tsp -split-functions -split-all-cold
  -reorder-functions=cdsort`, the recipe in `docs/README.md`; the pipeline script is
  `pipeline.sh`.

**Board:** `quench-bench/v8-v7-rss.cjs`, MemoryFirst, 5 runs per engine. QuickJS, Node
(with and without `--jitless`) and Bun (with and without JIT) are run as shipped.

**Host:** 4-vCPU Intel Xeon @ 2.80GHz x86_64 VM, 16 GB.

Ratio is Quench's max RSS over 0.8 × the best other engine's best run. The target is ≤ 1.00
on the worst run.

## Board 1 (`board-1.txt`)

| Fixture | no-JIT worst (median) | --jit worst (median) |
| -- | --: | --: |
| Richards | 0.88 (0.81) | 0.85 (0.81) |
| DeltaBlue | 0.84 (0.84) | 0.90 (0.87) |
| Crypto | 0.98 (0.96) | 0.99 (0.95) |
| RayTrace | 0.98 (0.94) | 0.97 (0.94) |
| EarleyBoyer | 0.44 (0.43) | 0.44 (0.44) |
| RegExp | 0.99 (0.95) | 0.95 (0.95) |
| Splay | 0.64 (0.64) | 0.65 (0.64) |
| NavierStokes | 0.93 (0.93) | 0.95 (0.94) |

All 16 rows meet the target on the worst run. The best other engine is QuickJS on every
fixture except Splay, where it is Bun without JIT.

What closed the last rows, in order:
1. The compiled-RegExp compaction (`tod137-regexp-compact-patterns-…`).
2. The documented call-graph layout, on profiles trained for the measured binary
   (`tod137-board-cdsort-layout-…`). A binary bolted with a stale profile measured
   120–200 KiB higher on several rows.
