# F1 (TOD-197): one binary, both memory policies, Score and max RSS together

**Binary:** trunk `20b251514b` (v2-gc at the same commit) as a static production build,
BOLTed with `-reorder-functions=cdsort` on profiles retrained for this binary. The recipe is
in `docs/README.md`.

**Board:** `quench-bench/v8-v7-rss.cjs`, 5 runs per engine per board. The boards alternated
MemoryFirst, Throughput, MemoryFirst, Throughput, so each policy has 10 runs per fixture.
QuickJS, Node (with and without `--jitless`) and Bun (with and without JIT) are run as
shipped.

**Host:** 4-vCPU Intel Xeon @ 2.80GHz x86_64 VM, 16 GB.

**Table columns** (`table.txt`, by `poltable.py`; no-JIT Quench only, the F1 mode):
- Score: median of Quench's per-round paired ratio against Node `--jitless` and against Bun
  with JIT off.
- RSS: worst-run max RSS over 0.8 × the best other engine's best run.

| Fixture | MF ×Node jl | MF ×Bun nj | MF RSS | TP ×Node jl | TP ×Bun nj | TP RSS |
| -- | --: | --: | --: | --: | --: | --: |
| Richards | 1.15 | 1.14 | 0.98 | 1.14 | 1.17 | 1.74 |
| DeltaBlue | 0.89 | 1.34 | 0.90 | 1.09 | 1.70 | 2.16 |
| Crypto | 2.13 | 1.45 | 1.00 | 2.15 | 1.44 | 2.65 |
| RayTrace | 0.69 | 1.37 | 1.08 | 0.94 | 1.82 | 2.44 |
| EarleyBoyer | 0.73 | 1.88 | 0.44 | 1.08 | 2.88 | 0.80 |
| RegExp | 0.90 | 4.44 | 0.92 | 1.17 | 6.00 | 1.89 |
| Splay | 1.25 | 1.46 | 0.64 | 1.26 | 1.37 | 0.67 |
| NavierStokes | 1.75 | 1.12 | 0.93 | 1.73 | 1.10 | 1.08 |

**Reading:**
- **Neither policy qualifies on this binary.**
  - **MemoryFirst:** RSS met on 6 of 8 fixtures (RayTrace's worst run 1.08, Crypto's 1.00).
    Score ≥ 1.20 against both rivals on 1 of 8 (Splay).
  - **Throughput:** RSS met on 2 of 8 (Splay, EarleyBoyer). Score ≥ 1.20 against both on 1
    of 8 (Splay).
- **Score is short under both policies** against Node `--jitless`: DeltaBlue, RayTrace,
  EarleyBoyer and RegExp under Throughput too. It is also short against Bun no-JIT on Richards
  and NavierStokes. That part of F1 is the speed lanes'.
- **MemoryFirst's speed cost** against Throughput: DeltaBlue −17%, RayTrace −24%,
  EarleyBoyer −30%, RegExp −27%; Richards, Crypto, Splay and NavierStokes are within ±3%.
- **Throughput's memory cost** against MemoryFirst: 1.7–2.6× worst max RSS on six fixtures.

A qualifying single policy therefore needs both:
1. The speed lanes raising the no-JIT Score base.
2. A policy between the two tables that keeps MemoryFirst's RSS on the six memory-bound
   fixtures, while recovering its loss on the four allocation-heavy ones.
