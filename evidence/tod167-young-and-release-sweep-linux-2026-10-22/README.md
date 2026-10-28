# TOD-167: young minimum and collection-time page release, swept on one binary

Host: Linux x86_64 VM, 4 logical CPUs, 15 GB.

Binary: one static + BOLT `quench-node` built from `meas-gc-sweep`, which is never merged. It is
v2-gc `6db7d383d`, plus TOD-164 step 3's candidates and their switches
(`QUENCH_MEASURE_TRACE_*`), a `gc-tuning` build (`QUENCH_GC_MIN_YOUNG`) and a switch for what
MemoryFirst releases at collections (`QUENCH_MEASURE_COLLECTION_RELEASE`).

`sweep.py` gives every configuration the same 8 V8-v7 fixtures, binary evicted before each run.
Score and max RSS are geomeans relative to Throughput.

## Young minimum under MemoryFirst

No JIT, 5 runs (`sweep-confirm.json`):

| Young minimum | Score | Max RSS |
| -- | --: | --: |
| 768 (today's MemoryFirst) | 0.780 | 0.629 |
| 4096 | 0.890 | 0.630 |
| 8192 | 0.953 | 0.640 |

`--jit`, 3 runs (`sweep-confirm-jit.json`):

| Young minimum | Score | Max RSS |
| -- | --: | --: |
| 768 | 0.727 | 0.607 |
| 4096 | 0.839 | 0.632 |
| 8192 | 0.891 | 0.648 |

The RSS cost of 8192 over 768 falls on a few fixtures:
- No JIT: regexp +684 KiB, earley-boyer +180 KiB.
- JIT: deltablue +550 KiB, navier-stokes +1.6 MB.

## What MemoryFirst releases at each collection (2 runs, `sweep-release.json`)

| Young minimum | Released at each collection | Score | Max RSS |
| -- | -- | --: | --: |
| 768 | code and heap pages (today) | 0.730 | 0.642 |
| 768 | heap pages only | 0.925 | 0.684 |
| 768 | nothing | 0.957 | 0.687 |
| 8192 | code and heap pages | 0.951 | 0.651 |

At 768, releasing code pages at each collection costs about 22% of the score: the run refaults
its code after every one of thousands of collections. At 8192 there are 10× fewer collections
and the release costs almost nothing.

## TOD-164 step 3 (root skipping), 2 runs (`sweep-young.json`)

Within noise, between −3% and +2% per configuration. Root scanning is not where the
policies' speed difference lies.
