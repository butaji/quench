# TOD-151: charge text made on demand to the collector's budget

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB, no cgroup CPU quota; rustc 1.99.1.
Release builds; base `c150` = v2-gc e088a870 (TOD-150), candidate `c151e` (this change).

## Result

| Measure | Base | Candidate |
| -- | -- | -- |
| npm top 300 geomean RSS (`npm-top300-rss.txt`) | — | ×0.896, none >5% worse |
| npm median package | 20.8 MB | 18.8 MB |
| typescript-eslint / playwright | 193 / 144 MB | 147 / 95 MB |
| typescript-lib peak RSS | 215.1 MB | 186.8 MB |
| babel-wtb peak RSS | 99.2 MB | 94.7 MB |
| JetStream Score, 5 pairs (`jetstream-pairs-5.txt`) | — | within noise (json-parse ×1.06, json-stringify ×1.00, typescript-lib ×1.00, babel ×0.92 but same collection counts and wall time in a traced rerun) |
| V8-v7 Score geomean (`v8-pairs-3.txt`) | — | ×1.028 |

Run-to-run noise on this host is ±15% for single JetStream tests (doxbee-async 19.6–25.9
on one binary). babel's lower median is not GC work: QUENCH_GC_TRACE gives 4,371 vs 4,383
collections, 88 full each, wall 23.06 vs 23.04 s.

## Variants

- A: should_collect read the pending bytes at every safepoint: web-ssr RSS 68.7 MB but 32%
  more collections there and json-stringify ×0.90 (0/5): rejected.
- B: fold only at generic allocation: web-ssr RSS +4.8 MB: superseded.
- C/E (kept): fold at generic allocation, native and host call return, fast-lane string
  methods (`pure_string_method_call`, where web-ssr's 30 flattenings of ~350 KB come from),
  and each collection; allocation weight only.

web-ssr stays on its young-budget resonance (1/8: 84–89 MB both); at 1/7, 1/9 and 1/16 the
candidate is 3 MB lower (71–72 vs 74–75 MB). Runtime lib 747/747.
