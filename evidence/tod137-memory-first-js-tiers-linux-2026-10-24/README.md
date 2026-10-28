# MemoryFirst compiles no JavaScript function tiers

Question: under MemoryFirst `--jit`, every V8-v7 row except RayTrace peaked above its no-JIT
run on the last board (`tod137-board-general-dispatch-gated-…`). What do the JavaScript
function tiers still buy there?

Probe: one static unbolted binary of v2-gc `916d83d686`, MemoryFirst, 5 alternating runs
(`njt.py`). A temporary switch, `QX_NOJSTIER`, not committed, makes `js_tier_runs` false.
Arms:
- `--jit`;
- `--jit` with the JavaScript tiers off;
- no JIT.

| Row | `--jit` KiB / Score | tiers off | no JIT |
| -- | -- | -- | -- |
| Richards | 2912 / 1488 | 2700 / 1381 | 2636 / 1211 |
| DeltaBlue | 3144 / 1086 | 2956 / 984 | 2952 / 1013 |
| Crypto | 3372 / 2265 | 3288 / 2237 | 3304 / 2204 |
| RayTrace | 3200 / 1868 | 2976 / 2082 | 2992 / 1791 |
| RegExp | 6300 / 2171 | 6140 / 2026 | 6088 / 2143 |
| NavierStokes | 4472 / 2484 | 4092 / 2679 | 4196 / 2654 |
| EarleyBoyer | 6736 / 1462 | 6588 / 3288 | 6524 / 3226 |
| Splay | 31580 / 3645 | 31532 / 4207 | 31524 / 3993 |

Reading:
- **Memory:** with the tiers off, every `--jit` row peaks 48–380 KiB lower, at its no-JIT level.
- **Score:** it moves both ways.
  - Up: EarleyBoyer ×2.25, Splay +15%, RayTrace +11%, NavierStokes +8%.
  - Down: DeltaBlue −9%, Richards −7%, RegExp −7%, Crypto −1%.

## Change

`JS_FUNCTION_TIERS = [true, false]` (`MemoryPolicy` order). `js_tier_runs`, the one gate of
the per-function tier, of the general-dispatch tier and of the loop tier's site analysis,
reads it. It replaces the general-dispatch-only table, and `note_js_general_dispatch_entry`
reads the same table. This follows the owner's TOD-123 direction for MemoryFirst `--jit`.
Throughput is unchanged.

Tests:
- `js_function_tiers_follow_the_memory_policy` pins the table.
- The OSR test now expects loop-site analysis only where the tiers run.
- Runtime 929/929, on top of the 12:07 sync.
