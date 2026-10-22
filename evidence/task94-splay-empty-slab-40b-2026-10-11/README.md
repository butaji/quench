# Splay empty-slab release screen on the 40B candidate

Date: 2026-10-11 UTC  
Lane: Task 94 (`v2-cloud`)  
Splay remains the sole active fixture.  
Task 61 source candidate: `9824fc2842c58d5ac4c408c1d2a49ab8d3fd06c9` (`origin/v2-splay-single-space-40b-candidate`).  
Harness source revision: `44bb4a79b76c2b52b42752936691809cddf3512f`.  
Rust: `1.99.0 (b940084d7 2026-09-28)`.

## Method

Both experiments used the exact Task 61 40B source candidate and compared it with an unmodified binary from that source. Production Quench binaries were run on the materialized Splay fixed-work fixture for 11 pairs, 300 `run()` iterations per sample. The Linux host is x86_64, kernel `6.18.44`, 4 logical CPUs (quota 4), with a 16 GiB cgroup memory limit. The runner used Linux `wait4` RSS. It has no usable `perf` counters, so both reports have `qualification_ready: false`, null cycles/instructions, and 0/11 clean samples. These runs diagnose output and peak RSS; they cannot qualify a timing change.

The baseline binary SHA-256 is `963f5689d5da4eb7f362621ea76fcff4729c8a876008076866a01edd0f5c3c43`. The all-empty-slab candidate binary is `9cdabeac5a54884dca93a8b14daa245b56389101fc3922fa4bce70a41e1e2885`. Its source diff is retained as `all-empty-slabs.patch` (SHA-256 `7d84e0fc1a137423756b75da267e8592f402f5f1d8528e4a1b1aa8d0cc72b085`). It returns every slab proven empty after a full sweep while preserving flat indexes and generations, then recreates a slab on free-slot reuse. The implementation and two generation/reuse tests passed the 549-test `quench-runtime --lib` suite before a final cfg-only adjustment; the final optimized production build succeeded.

## Results

| Experiment | Work max RSS, base → candidate | Setup-only max RSS, base → candidate | Valid/output-equal | Decision |
| --- | ---: | ---: | ---: | --- |
| Trim trailing empty slabs | 95,096,832 → 95,096,832 B | 53,927,936 → 53,825,536 B | 11/11 | No Splay peak change |
| Return all empty slabs | 95,223,808 → 95,346,688 B | 53,841,920 → 53,923,840 B | 11/11 | +122,880 B work RSS; reject |

The all-empty-slab run's output matched Node v26.10.0 `--jitless` byte-for-byte. Both outputs hash to `c20ec3581a4413cd22a6c5a4e152c34137941ed5d8818b3955b7b3e77d8ab9f2`. The oracle script and outputs are included here.

Neither approach reduces the measured work high-water mark. This agrees with the trunk owner's Linear finding that returning empty high-index slots can reduce current RSS after collection but cannot lower the earlier peak that the bar measures. The result rejects page return as the active optimization.

## Direction informed by data and online research

The current Linux 40B stock result remains 95,301,632 B vs Bun no-JIT 60,334,080 B and Score 2,316 vs Node `--jitless` 5,036; neither target is met. The trunk owner's Splay profile attributes roughly 87% of the remaining RSS gap to run-phase growth, with about 781K survivors. At 40 B per slot, the observed one-survivor headroom adds about 31 MB; the target increment is about 8 MB (approximately 0.25x survivors). This puts the threshold schedule, not post-sweep page return, on the critical path.

V8's official GC documentation describes minor collection as tracing young objects from roots plus the old-to-young remembered set, with the old generation collected separately. Its Orinoco design likewise describes old-to-young references as young-GC roots and remembered sets as the way to avoid scanning the full old generation. This supports testing the existing young/remembered-set collector on the compact candidate, while measuring barrier cost and verifier overhead rather than assuming the architecture wins: [V8 garbage collection](https://chromium.googlesource.com/v8/v8/%2Bshow/lkgr/docs/heap/garbage-collection.md), [Orinoco young-generation collection](https://v8.dev/blog/orinoco-parallel-scavenger).

Next screen: run the Task 94 minor/full policy on the exact 40B candidate and sweep young allocation headroom toward 0.25x survivors. Record collections, young cells swept, remembered-owner scans, runtime, and peak RSS. Keep the trunk's full-collection policy owned by trunk; retain no source change until the paired Splay speed and RSS gates pass.
