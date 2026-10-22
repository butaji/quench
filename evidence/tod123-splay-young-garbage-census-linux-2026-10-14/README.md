# Record-heap young-garbage census

This is a diagnostic run on Linux x86_64 at baseline `aea2c5445`, the commit shared by `v2-gc` and `v2-merged` when the measurement started. It ran the pinned V8-v7 Splay fixed-work script for 300 calls after setup and observed 16 full collections.

The profile-only memory counter compares each freed record's allocation clock with the clock at the preceding collection. Managed bytes include the in-space `Object`, `ArrayObject`, or legacy `Cell` footprint plus the payload bytes already recorded by the profiler. The completed run found 827,210 of 1,990,134 freed records (41.57%) were allocated after the preceding collection, accounting for 41,933,436 of 101,823,724 freed managed bytes (41.18%). The measurement exceeds the 20% gate for testing G-09.

The experiment does not establish a speed or RSS win and does not change the production policy. G-09 is tracked in Linear issue [TOD-124](https://linear.app/todoxapp/issue/TOD-124/g-09-record-heap-young-collection-with-rss-guard), with acceptance requiring lower maximum RSS at equal-or-better wall time, correctness checks, and the all-eight instruction guard.

## Reproduction

From repository root, using Rust 1.99.0:

```sh
cargo build --release -p quench-node --features profile-memory
QUENCH_MEMORY=1 target/release/quench-node evidence/tod123-splay-young-garbage-census-linux-2026-10-14/splay-fixed-300.js
```

The binary hash, fixture hash, host quota, aggregate result, stdout, and full profile output are retained alongside this file. The profile output contains bootstrap, run, and completion snapshots; the `complete` allocation census and memory composition report are the aggregate input to `census.json`.
