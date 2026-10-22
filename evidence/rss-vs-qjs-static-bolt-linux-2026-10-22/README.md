# V8-v7 max RSS against QuickJS, Node and Bun: static + BOLT, both run tiers trained

Host: Linux x86_64 VM, 4 logical CPUs, 15 GB.

Engines: Node 22.22.0 (`--jitless` and JIT), Bun 1.4.2 (no-JIT and JIT), QuickJS-ng 0.10.1. See
`engines.txt`.

Board: `quench-bench/v8-v7-rss.cjs`, 3 runs, every binary evicted from the page cache before
each run. The ratio is Quench's worst sample in both modes over the best sample of any other
engine. The owner's target is ≤ 0.80 on every fixture.

## Builds

| Board file | Build |
| -- | -- |
| `board-production-dynamic.json` | `--profile production`, dynamic glibc (v2-gc `8024bed7a`) |
| `board-static.json` | qualification static build (`docs/README.md`: crt-static, position-dependent, `x86_64-linux-static.order`) |
| `board-static-bolt-old-order.json` | the static build bolted (BOLT 20) with profiles of the interpreter only |
| `board-static-bolt-new-order.json` | static build relinked with the retrained order, then bolted with both tiers' profiles |

The retrained order (`crates/quench-node/link/x86_64-linux-static.order`, commit on
`cand-rss-static-order`) and the BOLT profile come from callgrind runs of TOD-171's generic
corpus, never a benchmark fixture: deegen curriculum, micros, a 300-file test262 sample and an
empty script (`train.sh`).

- Each run is split at `quench_entry_compiled` (`--dump-before`).
- `symbol-order` takes start-up profiles, then `--run` interpreter profiles, then `--run` JIT
  profiles, with `--dense 13dispatch_fast --foreign-first`.
- `mkbolt.sh` relinks and bolts.
- The previous order and BOLT training had no JIT-tier profiles, so the JIT compiler sat in
  cold, scattered pages.

## Result (`summary.txt`)

| Fixture | Dynamic | Static | Static + BOLT, old order | Static + BOLT, new order |
| -- | --: | --: | --: | --: |
| richards | 1.16 | 0.97 | 0.79 | **0.745** |
| deltablue | 1.20 | 0.96 | 0.94 | 0.856 |
| crypto | 1.43 | 1.16 | 1.10 | 1.067 |
| raytrace | 1.20 | 1.01 | 0.88 | **0.738** |
| earley-boyer | 0.44 | 0.40 | 0.38 | **0.362** |
| regexp | 1.07 | 0.98 | 0.92 | 0.944 |
| splay | 0.53 | 0.53 | 0.52 | **0.516** |
| navier-stokes | 1.06 | 0.92 | 0.84 | **0.781** |

Five of eight fixtures meet the target with the new order.

The new order against the old, both bolted, 10 pairs on V8-v7: speed ×1.003, RSS ×0.976
(`scorecard-v8-old-vs-new-order-10.txt`).

## What remains

Node and Bun are 5–100× above Quench on every fixture. QuickJS is the only bar left.

- **The peak is mostly code pages** (`rsssplit2.py`: RssAnon against RssFile over time).
  - The host releases read-only pages at compile phases and collections, and a run refaults
    about 1.1 MB of code between releases.
- **Crypto `--jit`:** every compile attempt fails (`unsupported-lowering-shape-or-codegen-failure`).
  - Yet each attempt runs the JIT compiler, and its code pages put the JIT peak 1.1 MB above
    no-JIT.
  - A release before each JIT compilation did not help (measured and rejected).
  - The fix belongs to the JIT lane: skip attempts that cannot succeed.
- **Regexp** is heap: 5.2 MB anonymous against QuickJS's 4.1 MB.
- **Speed is separate.** Node's JIT is 10–30× faster than Quench `--jit` on V8-v7, and
  Node `--jitless` leads Quench's interpreter on raytrace, earley-boyer, regexp, deltablue
  and splay. These are compiler and interpreter lane targets.
