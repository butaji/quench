# Per-operation benchmark

Run with:

```sh
node tools/bench-ops.cjs
```

Optional environment variables are `BENCH_ITERATIONS` (default 1,000,000),
`MAX_WALL_MS` (default 30,000 per execution), `MAX_TOTAL_MS` (default 55,000
for the harness), and `BENCH_REPEATS` (default 3, capped at 5). Use
`node --expose-gc` to enable explicit GC sampling. The harness never runs a
full suite; each VM execution has a timeout and the process has a total-time
budget.

The JSON `results` array contains up to five bounded micro-workloads. Existing
fields remain available: `wall_ns` and `per_op_ns` now report the median sample,
`rss_delta_bytes` is the average process RSS change, and `allocs_proxy` is a
best-effort average allocation proxy (post-GC positive heap growth divided by
64 bytes). A timed-out workload is reported with `timed_out: true`.

Additional machine-oriented fields are `repeat_count`, `wall_ns_median`,
`wall_ns_p95`, `per_op_ns_median`, `per_op_ns_p95`, and `timed_out_repeats`.
Percentiles use nearest-rank selection, so p95 is deterministic for the small
bounded sample set. `representative_rss_delta_bytes` identifies the RSS delta
from the sample whose wall time is the median.
