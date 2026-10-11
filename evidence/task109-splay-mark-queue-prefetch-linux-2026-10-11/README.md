# TOD-109 G-04: Splay mark-queue prefetch (Linux)

## Decision

No prefetch buffer qualified. Sizes 8, 16, and 32 all had paired bootstrap
intervals that crossed zero; sizes 16 and 32 had slower paired medians. The
fixed-work host produced no clean counter rounds. The temporary FIFO buffer and
x86 prefetch code were removed, leaving the `4672a4658` production source
unchanged. No stock Score/RSS claim is made for these temporary binaries.

## Online research and what it predicts

- Hans-J. Boehm, “Reducing Garbage Collector Cache Misses,” *ACM SIGPLAN
  Notices* 36(1), January 2001, pp. 59–64,
  [DOI 10.1145/362426.362438](https://doi.org/10.1145/362426.362438). The
  abstract describes “prefetch-on-grey” as a way to reduce tracing cache
  misses in a non-moving collector, and says the technique remained useful in
  some non-moving generational collector experiments.
- Chen-Yong Cher, Antony L. Hosking, and T. N. Vijaykumar, “Software
  Prefetching for Mark-Sweep Garbage Collection,” *ASPLOS XI*, 2004,
  pp. 199–210,
  [DOI 10.1145/1024393.1024417](https://doi.org/10.1145/1024393.1024417).
  The paper abstract indexed by [OpenAlex](https://openalex.org/W2098220603)
  reports timing sensitivity in Boehm’s prefetch placement, simulation gains
  from a redesigned strategy, and a 6% average live-hardware speedup on
  GC-intensive benchmarks with controlled heap growth.

These papers establish a plausible cache-miss hypothesis, not a transferable
queue depth. Their collector layouts, prefetch placement, hardware, and
workloads differ from Quench. The Splay measurements below challenge the
hypothesis for a FIFO buffer in front of Quench's mark stack: no tested size
produced a decisive paired wall-time improvement.

## Provenance and method

- Source base: `4672a4658`, after the G-06 rejection record.
- Rust: `1.99.0 (b940084d7 2026-09-28)`.
- Control: Splay, fixed-work, one marker, buffer size 0, binary SHA-256
  `6aaf73b79e0e71419a1ed9ebfe0e705e8c755c72e6ac0f1a4ea9b68e61e27bbd`.
- Each candidate build prefetched the slot record before scanning it and used a
  FIFO buffer of the stated size ahead of the existing LIFO stack.
- Workload: 300 iterations of Splay per sample, 11 alternating paired rounds
  per size. Every report was valid and output-equal 11/11; all had 0/11 clean
  Linux counter rounds. The results are paired wall-time screening data, not a
  qualified speed claim.
- Paired marginal wall time is `(work wall − setup-only wall) / 300`; each
  paired delta is candidate minus control. The 95% interval uses 100,000
  bootstrap resamples of the paired median, deterministic xorshift32 seed
  `0x61c88647`, and the 2.5%/97.5% empirical quantiles.
- Raw JSON reports and temporary candidate binaries are ignored artifacts in
  `work/task109/`, not committed.

## Results

| Buffer | Control median | Candidate median | Paired median delta | Candidate wins | Bootstrap 95% interval | Work RSS delta | Setup RSS delta | Candidate SHA-256 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 0 (control) | — | — | — | — | — | — | — | `6aaf73b79e0e71419a1ed9ebfe0e705e8c755c72e6ac0f1a4ea9b68e61e27bbd` |
| 8 | 4,573,663 ns/run | 4,585,183 ns/run | −464,648 ns/run | 6/11 | −878,049 to +819,128 ns | 0 B | +4,096 B | `ae9489f5213d6d6a6a7b3c18c4f78bd3e291724b09962eddb39d4c515d1db421` |
| 16 | 2,970,822 ns/run | 3,077,882 ns/run | +209,937 ns/run | 3/11 | −44,446 to +671,359 ns | +499,712 B | −172,032 B | `f90902deeecdc11b56959d00a2e24a9eafa24f90ca00f8b5dc3740c23ff12523` |
| 32 | 3,461,689 ns/run | 3,604,075 ns/run | +690,347 ns/run | 3/11 | −64,768 to +1,355,226 ns | +86,016 B | −180,224 B | `1b2f973df785f404f50a09e16dec0eab4d33bcdd6b34a8b01eefd4eb2482985c` |

Absolute control medians vary substantially across sessions. The paired deltas
are the comparison of interest; none excludes zero. RSS changes are small and
do not show a repeatable reduction.

## Correctness

- The Rust 1.99 warnings-denied `quench-runtime` check passed with the size-8
  implementation.
- Focused heap tests passed 30/30 with size 8.
- Fixed-work Splay output was equal for 11/11 pairs at each of sizes 8, 16,
  and 32.
- Node semantics were not changed. M4 gates were not run on these rejected
  temporary binaries.

## Reproduction

Use the pinned control and candidate artifacts under `work/task109/` with the
production benchmark runner:

```sh
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work --runs 11 --timeout-ms 300000 \
  --quench work/task109/splay-baseline-a125-node \
  --quench-peer work/task109/splay-g04-prefetch-8-node \
  --out work/task109/splay-g04-prefetch-8-fixed-11.json
```

Repeat with the size-16 and size-32 candidate/report names. The source
experiment changed `MARK_QUEUE_PREFETCH_BUFFER_SIZE` for each production build;
the candidate hashes identify the exact executables used. The ignored reports
contain the per-round work, setup, RSS, host and output-equality records.

## Active Splay gap

The latest stock run on the unchanged production baseline remains Quench Score
2,773 vs Node 5,142 (54.0%) and maximum RSS 117,227,520 B vs Bun 62,410,752 B
(1.88x). Both Stage B bars remain open. `tasks/61.md` is absent from the current
`origin/v2`; compact heap records are owned by the trunk lane and remain the
largest documented RSS dependency. This collector lane stays on Splay and
continues with collector-owned options after checking upstream lane progress.
