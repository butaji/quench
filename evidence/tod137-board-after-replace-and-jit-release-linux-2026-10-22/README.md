# V8-v7 board after TOD-184 and TOD-185

Host: Linux x86_64 VM, 4 vCPUs, 15 GB, kernel 6.18.

- **Binary:** static + BOLT `quench-node` (`qb5`) from v2-gc `a391e63280`: trunk 9f8b3ab7f, the
  2^13 MemoryFirst young minimum, the regexp-replace fix (TOD-184) and the JIT compile release
  (TOD-185). The BOLT profile predates the trunk merge.
- **Board:** `quench-bench/v8-v7-rss.cjs`, 3 runs, evicted binaries, run by Node 22 so the
  runner does not share the measured binary's pages. QuickJS-ng 0.10.1, Node 22.22.0 and Bun
  1.4.2, each in both its modes.

## Against the target: ≥20% better than every other runtime (`gaps.txt`)

The RSS factor is Quench's worst sample over 0.8 × the lowest other engine's best; at or below
1.0 the cell is met. MemoryFirst:

| Fixture | Score needed, no JIT | RSS factor, no JIT | RSS factor, `--jit` | Before today (worst of both modes) |
| -- | --: | --: | --: | --: |
| richards | ×1.33 | 1.16 | 1.13 | 1.11 |
| deltablue | ×1.29 | 1.13 | 1.08 | 1.33 |
| crypto | met | 1.28 | 1.41 | 1.77 |
| raytrace | ×1.46 | 1.32 | 1.11 | 1.17 |
| earley-boyer | ×1.79 | met (0.50) | met | met |
| regexp | ×1.48 | 1.22 | 1.19 | 1.31 |
| splay | ×1.12 | met (0.65) | met | met |
| navier-stokes | ×1.07 | 1.15 | 1.84 | 2.23 |

"Before today" is the `tod167-young-minimum-board` evidence. This board's QuickJS samples are
a little lower (Crypto 3.66 MB against 3.80), so some factors rose while Quench's own bytes
fell.

- **JIT mode improved most:** DeltaBlue, Crypto and NavierStokes fell by 0.9–1.7 MB.
- **RayTrace without JIT** rose by 0.2–0.4 MB against the earlier board, and the plain-build
  A/B shows no change from today's fixes. The rise came with the trunk merge, and the BOLT
  link order predates it; a retrained link order is the RSS lane's job (TOD-171).
- **Score with JIT** still needs ×1.8–63 against V8 and JavaScriptCore (TOD-110).
