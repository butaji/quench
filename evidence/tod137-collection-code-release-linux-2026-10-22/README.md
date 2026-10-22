# What MemoryFirst's collection-time release costs, and a power-of-two backoff

Host: Linux x86_64 VM, 4 vCPUs, kernel 6.18. Every binary is a static + BOLT `quench-node`
built from v2-gc `7ad78bf43f`, plus the change named in each section. Ratios are against
Throughput on the same binary. Fixture order in the summaries: richards, deltablue, crypto,
raytrace, earley-boyer, regexp, splay, navier-stokes.

## Where MemoryFirst's speed goes (`faults.txt`, `ru.py`)

EarleyBoyer, medians of 3. Columns are Score, user time, system time and minor faults:

| MemoryFirst releases at each collection | Score | User | Sys | Minor faults |
| -- | --: | --: | --: | --: |
| code and heap pages (trunk) | 2705 | 5.72 s | 0.40 s | 196,637 |
| code pages only | 3036 | 4.68 s | 0.36 s | 178,449 |
| heap pages only (`malloc_trim`) | 3348 | 4.94 s | 0.13 s | 37,828 |
| nothing | 3648 | 4.94 s | 0.10 s | 39,035 |

Throughput scores 4340 with 15,753 faults.
- **Refaulting the binary's code** after each code release is the cost. The run collects
  thousands of times and refaults its own hot code after each release.
- **Collector instructions** are not the cost: callgrind puts collection at 5.2% of
  instructions under Throughput and 7.2% under MemoryFirst. TOD-164 step 3's root skipping
  was within noise for the same reason.

## What each release returns (`sweep-rel2*.json`, 5 runs without JIT and 3 with)

| Release | Score, no JIT | Max RSS, no JIT | Score, `--jit` | Max RSS, `--jit` |
| -- | --: | --: | --: | --: |
| code and heap (trunk) | 0.913 | 0.621 | 1.000 | 0.641 |
| heap only | 0.967 | 0.699 | 1.038 | 0.716 |
| code only | 0.907 | 0.626 | 1.004 | 0.639 |
| nothing | 0.980 | 0.697 | 1.018 | 0.722 |

Code release returns 560–800 KiB of each fixture's max RSS, about the whole remaining gap to
the 20%-below-QuickJS target.

## Power-of-two backoff (`cand-gc-code-release-backoff`; `sweep-backoff*`, `sweep-bnp*`)

The candidate releases code only at collection boundaries whose count is a power of two.
EarleyBoyer under MemoryFirst then scores 3845 with 41,555 faults (Throughput 4268).

| Code release | Score, no JIT | Max RSS, no JIT | Score, `--jit` | Max RSS, `--jit` |
| -- | --: | --: | --: | --: |
| every boundary (trunk, rel2 binary) | 0.913 | 0.621 | 1.000 | 0.641 |
| power of two (candidate) | 0.966 / 0.962 | 0.648 / 0.655 | 1.006 / 0.980 | 0.656 / 0.659 |
| power of two, plus whenever the heap's largest rise could reach the peak | 0.873 | 0.625 | 0.916 | 0.640 |

The two candidate figures come from two separate binaries.

- **Backoff** recovers most of the Score MemoryFirst lost. Max RSS is 100–300 KiB higher per
  fixture than releasing at every boundary, and 1 MB higher on Splay.
- **The near-peak rule** brings max RSS back but costs more Score than trunk. In steady state
  the resident set sits near the peak, so it fires at almost every collection.
- **Trade-off for the owner:** about +5% Score against about +4% max RSS under MemoryFirst.
  The code that the per-boundary release keeps out is the part each cycle touches only
  once. A layout that packs the collector's code, so it occupies fewer pages, would shrink
  it without refaults; that is RSS-lane work (link order).
