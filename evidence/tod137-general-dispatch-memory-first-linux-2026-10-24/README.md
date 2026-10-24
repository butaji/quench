# Board after the 06:07 sync; MemoryFirst compiles no general-dispatch functions

## Board

The bolted board ran on v2-gc `a8ed7d97ba`. That tree includes:
- the 06:07 sync, which brought the jit lane's general-dispatch tier (`882889c3ca`);
- the cold-entry change.

5 runs against QuickJS-ng, Node and Bun (`board-5-runs.*`). Each factor is the worst run,
median in brackets, over 0.8 × the lowest other engine:

| Fixture | No JIT | `--jit` |
| -- | --: | --: |
| Richards | 1.00 (0.97) | 1.07 (1.05) |
| DeltaBlue | 0.99 (0.94) | 1.03 (1.03) |
| Crypto | 1.09 (1.07) | **1.86 (1.85)** |
| RayTrace | 1.13 (1.05) | 1.14 (1.14) |
| RegExp | 1.13 (1.10) | 1.13 (1.09) |
| NavierStokes | 1.00 (0.99) | 1.05 (1.03) |
| EarleyBoyer | 0.47 | 0.50 |
| Splay | 0.64 | 0.65 |

- **The cold-entry change works:** the first run is no longer any row's outlier.
- **Crypto `--jit` regressed:** 3220 → 5496 KiB. Its peak is now a burst of about 2.3 MB at the
  end of the run. `strace -k` places it in regalloc2 under `compile_js_general_dispatch`.
- **Some no-JIT medians rose** against the previous board (Crypto 2996 → 3172, RegExp 5892 →
  6208). These rows do not run the tier, so the cause lies elsewhere in the sync's tree;
  not yet attributed.

## Change: `GENERAL_DISPATCH_TIER` = compile / don't by `MemoryPolicy`

The general-dispatch tier is gated by memory policy, as the loop tier is
(`LOOP_TIER_OPT_LEVELS`). Under MemoryFirst it compiles no function; this follows the owner's
TOD-123 direction that MemoryFirst may trade Score for memory under `--jit`.

The A/B is one static binary, unbolted, with a temporary switch (`QX_GD_ON`, not committed)
that turns the tier back on. Five alternating runs, MemoryFirst `--jit`, median KiB:

| Row | Gated | Tier on | Score change with tier on |
| -- | --: | --: | --: |
| Richards | 2848 | +132 | +2.2% |
| DeltaBlue | 3128 | +360 | −7.1% |
| Crypto | 3348 | **+2492** | +7.1% |
| RayTrace | 3052 | +980 | −2.3% |
| RegExp | 6240 | −148 | −1.2% |
| NavierStokes | 4452 | 0 | −2.1% |
| EarleyBoyer | 6668 | +452 | −6.6% |
| Splay | 31596 | +32 | −6.3% |

Throughput is unaffected.

Noted for the jit lane: MemoryFirst `--jit` scores here are below no-JIT; Richards is 782
against about 1500.
