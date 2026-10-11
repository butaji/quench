# TOD-109 G-04: Splay mark-queue prefetch (Linux)

## Decision

No prefetch buffer qualified. Sizes 1, 2, 4, 8, 16, and 32 were screened.
Sizes 1, 4-first, 8, 16, and 32 had intervals crossing zero; size 2 and the
size-4 repeat had intervals wholly above zero, showing regressions. The
fixed-work host produced 0/11 clean counter rounds per session. The temporary
FIFO buffer and x86 prefetch code were removed, leaving the `8eb9b4617`
production source unchanged. No stock Score/RSS claim is made for these
temporary binaries.

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
produced a decisive paired wall-time improvement. Shorter windows did not help;
size 2 and the size-4 repeat had significant regressions.

## Provenance and method

- Source base: `4672a4658` for sizes 8/16/32 and `8eb9b4617` for the short-window
  extension sizes 1/2/4. These differ only by intervening documentation commits
  before the same temporary G-04 implementation.
- Rust: `1.99.0 (b940084d7 2026-09-28)`.
- Control: Splay, fixed-work, one marker, buffer size 0, binary SHA-256
  `6aaf73b79e0e71419a1ed9ebfe0e705e8c755c72e6ac0f1a4ea9b68e61e27bbd`.
- Each candidate build prefetched the slot record before scanning it and used a
  FIFO buffer of the stated size ahead of the existing LIFO stack.
- Workload: 300 iterations of Splay per sample, 11 alternating paired rounds
  per size; size 4 received one repeat session. Every report was valid and
  output-equal 11/11; all had 0/11 clean Linux counter rounds. The results are
  paired wall-time screening data, not a qualified speed claim.
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
| 1 | 2,964,227 ns/run | 2,974,773 ns/run | +98,593 ns/run | 4/11 | −387,100 to +380,186 ns | +90,112 B | −90,112 B | `f1f45958ebfdaadf719606ce013686b0aac8198a1dee00b5db0fc0485a278cad` |
| 2 | 2,838,379 ns/run | 3,207,049 ns/run | +260,402 ns/run | 2/11 | +28,230 to +679,212 ns | +12,288 B | 0 B | `5eb88deb7edf582c44c023a47e96cb3c9884d8495c8dd3e22553dd608143dae9` |
| 4 (first) | 3,004,003 ns/run | 3,756,638 ns/run | +752,635 ns/run | 5/11 | −386,603 to +1,516,440 ns | 0 B | +20,480 B | `41864b25fe13e5e254e8098d2068d7c9a5d990a08027f30c3befbaab683e297f` |
| 4 (repeat) | 3,141,734 ns/run | 3,640,677 ns/run | +495,514 ns/run | 2/11 | +144,163 to +1,845,318 ns | −167,936 B | +8,192 B | `41864b25fe13e5e254e8098d2068d7c9a5d990a08027f30c3befbaab683e297f` |
| 8 | 4,573,663 ns/run | 4,585,183 ns/run | −464,648 ns/run | 6/11 | −878,049 to +819,128 ns | 0 B | +4,096 B | `ae9489f5213d6d6a6a7b3c18c4f78bd3e291724b09962eddb39d4c515d1db421` |
| 16 | 2,970,822 ns/run | 3,077,882 ns/run | +209,937 ns/run | 3/11 | −44,446 to +671,359 ns | +499,712 B | −172,032 B | `f90902deeecdc11b56959d00a2e24a9eafa24f90ca00f8b5dc3740c23ff12523` |
| 32 | 3,461,689 ns/run | 3,604,075 ns/run | +690,347 ns/run | 3/11 | −64,768 to +1,355,226 ns | +86,016 B | −180,224 B | `1b2f973df785f404f50a09e16dec0eab4d33bcdd6b34a8b01eefd4eb2482985c` |

Absolute control medians vary substantially across sessions. The paired deltas
are the comparison of interest. The size-2 screen and size-4 repeat are
significant regressions; no other buffer's interval excludes zero. RSS changes
are small and do not show a repeatable reduction.

## Correctness

- The Rust 1.99 warnings-denied `quench-runtime` check passed with the size-1
  implementation.
- Focused heap tests passed 30/30 with both sizes 1 and 8.
- Fixed-work Splay output was equal for 11/11 pairs at every tested size; size
  4's repeat also matched 11/11.
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

Repeat with each size's candidate/report names in `work/task109/`; the size-4
candidate was run twice. The source experiment changed
`MARK_QUEUE_PREFETCH_BUFFER_SIZE` for each production build; the candidate
hashes identify the exact executables used. The ignored reports contain the
per-round work, setup, RSS, host and output-equality records.

## Active Splay gap

The latest repeat stock run on the unchanged production baseline measured
Quench Score 2,776 vs Node 5,484 (50.6%) and max RSS 117,587,968 B vs Bun
62,148,608 B (1.89x); it matched the earlier 2,773/5,142 score and
117,227,520/62,410,752 B RSS result. Both Stage B bars remain open. See the
[stock retake record](../task94-splay-stock-retake-linux-2026-10-11/README.md).
`tasks/61.md` is absent from the current `origin/v2`; compact heap records are
owned by the trunk lane and remain the largest documented RSS dependency. This
collector lane stays on Splay and continues with collector-owned options.
