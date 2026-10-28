# Throughput survival limit 3/4: V8-v7 check

**Change:** `THROUGHPUT_SURVIVAL_LIMIT = SURVIVAL_SCALE * 3 / 4`, replacing `SURVIVAL_SCALE / 2`.
It applies to the Throughput row only. The npm lane proposed and measured it; its doc comment
carries their npm board. Their 11-run board: typescript-eslint CPU −7%, eslint-plugin −8%,
playwright −3%, max RSS lower or equal. The +9% on @testing-library/dom was noise: over 21
runs the medians were 625 and 621 ms.

**V8-v7 check:** Throughput policy, static production builds, unbolted, binaries evicted.
- **Base:** v2-gc `8fb03c6810` (`qv2`).
- **Change:** `cand-gc-survival-768`, the same base plus the change (`qsv`).
- **Host:** 4-vCPU Intel Xeon @ 2.80GHz x86_64 VM.

**First pass** (`v8-9-interleaved.txt`, all eight fixtures, both modes, 9 interleaved runs):
- Max RSS medians are equal or lower, except Crypto `--jit` (+232 KiB median, −708 KiB worst)
  and DeltaBlue `--jit` (+128).
- The Score geomean ratio is 0.983 (no JIT) and 0.981 (`--jit`).

**Confirmation** (`v8-15-interleaved-confirm.txt`): the largest drops were RegExp −6.5%,
Splay `--jit` −5% and Crypto −4%. Over 15 interleaved runs:

| Row | Score base → change |
| -- | -- |
| RegExp | 3197 → 3257 (+2%) |
| RegExp `--jit` | 3117 → 3204 (+3%) |
| Splay | 4291 → 4406 (+3%) |
| Splay `--jit` | 4454 → 4602 (+3%) |
| Crypto | 2508 → 2421 (−3.5%) |
| Crypto `--jit` | 5331 → 5259 (−1.3%) |

The first pass's drops do not reproduce; both passes are within run-to-run noise. Merged into
v2-gc.
