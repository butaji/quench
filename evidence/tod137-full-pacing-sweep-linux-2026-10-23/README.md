# MemoryFirst full-collection pacing: old growth and allocation cap — both kept as they are

Host: Linux 6.18 x86_64 VM, 4 logical CPUs. Plain production `quench-node` with
`quench-runtime/gc-tuning` (v2-gc `ded7bd5b90`). MemoryFirst, compile cache warm, evicted binary,
5 alternating runs per setting. Each cell reads median max RSS (KiB) / Score.

Richards' timeline showed old records and the property arena growing between full collections
(`tod137-trailing-release-rejected-…`). Two knobs decide when a full collection runs: old growth
(how far the old generation may grow) and the allocation cap (how much may be allocated since the
last full collection, as a fraction of the live set).

## Old growth 1/1 (current) → 1/2, 1/4, 1/8 (`old-growth-sweep.txt`)

No fixture moved beyond noise in a consistent direction. Richards went 3976 → 3924–3976, and
Crypto `--jit` rose at 1/8 (4840 → 5084). The old generation's growth does not set these peaks.

## Allocation cap 1/1 (current) → 1/2, 1/4, 1/8 (`allocation-cap-sweep.txt`)

| Cap | RegExp | EarleyBoyer | Richards / Crypto |
| -- | -- | -- | -- |
| 1/4 | −260 KiB without JIT, −104 with | −176 / −288 KiB, Score −11% | Richards +128, Crypto `--jit` +140 KiB |
| 1/8 | | −912 / −904 KiB, Score −23 / −22% | |
| 1/2 | −196 / −40 KiB | Score −5% | others within ±80 KiB |

EarleyBoyer already meets the target at 0.50, so its RSS gain is worth nothing, while Richards and
Crypto are unmet. Not adopted.

The live-set-proportional cap stays at one live set, matching the "Weigh payload bytes more toward
collections" result in the ledger.
