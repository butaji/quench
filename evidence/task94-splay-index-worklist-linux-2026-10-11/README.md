# Splay collector screens: index worklist and policy sweeps

## Decision

No candidate in this record is retained. Splay remains active. The compact
index worklist is a small work/RSS improvement in one comparison, but it did
not establish a qualified speed gain. Policy changes either traded too much
speed for RSS or grew RSS without meeting the stock Score bar. Linux fixed-work
results are screening data: all eleven rounds were output-equal, but no round
had clean hardware-counter timing.

The Task 61 M4 targets remain open: Splay Score above Node --jitless and median
maximum RSS below Bun no-JIT at 57,950,208 B. These Linux stock runs are local
comparisons, not M4 qualification.

## Build and inputs

- Branch: v2-cloud at a1f987b96339dea84eb5d4c898353d8ca8f26093.
- Scratch build base: 9824fc284 with the Task 61 40 B compact-cell changes
  applied. These screens changed collector code/policy in the scratch worktree;
  none of that source is retained by this evidence commit.
- Rust: 1.99.0 (b940084d7 2026-09-28).
- Host: Linux x86_64, kernel 6.18.44, 4 logical CPUs, 4 CPU quota, 16 GiB
  memory limit. Process metrics use Linux wait4 rusage; perf IPC counters were
  unavailable, so clean timing rounds: 0/11.
- Fixture SHA-256:
  1f875cec579d03af40d256ceaeb6823f3ef16c883652650ca51d183472a8185d
- Fixed-work runner SHA-256:
  3e6004cf479a3546cab110112a325c8daf18f500c536b045463263786cc7ed94
- Node 26.10.0 SHA-256:
  ab9c8eecf9f82d6693cdc3accced17034065c8d96213b0aa76a7e803d20ae1da
- Bun 1.4.2 SHA-256:
  a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c
- QuickJS 2026-06-04 SHA-256:
  c798e32aac14495856a4709be67a11b0562a6e2d07fa89ad4f28efe46c7c7955
- Bun used BUN_JSC_useJIT=0 and its JIT-disabled probe succeeded. Node used
  --jitless. Quench ran the shared VM interpreter.

Quench candidate executable hashes:

- Direct slot worklist, 1/4 headroom and 1/4 full-growth:
  0d972188013d9f32fc546319c5bfe04bc27368ca85cfe12dc6707b237e51cefa
- Index worklist, 1/4 and 1/4:
  879f2b73256e9a72fe7d65bbdf7a1e41ca797f92d4a458a757155a87446c54f3
- Index worklist, 1/8 and 1/8:
  979bbfa25b39c8df425928ee3ff8647c27603ce22c98f847984744031d05d2d1
- Index worklist, 1/8 headroom and 1/4 full-growth:
  f70c2e48316be80e22cdfcfecb5049fb112d516864383422a1c0f041d61aeb17
- Index worklist, 1/8 headroom and 1.0 full-growth:
  5c88582b927fd979766e7a32b5f36192f65dce85981184bca190666bfe56a2e4
- Reusable index worklist, 1/4 and 1/4:
  397f6e33f0d70821cf031b63bbe811d96c5d601b784b37f95c363fcc2d1c9278
- Slab-trim experiment, 1/8 headroom and 1/4 full-growth (reverted after
  weak-generation aliasing was found):
  f1cd6b260a21d41fff76ad3c593e6bc343c32f7e94b6a52630b0010f850ef8b5

## Results

All fixed-work comparisons below ran 11 paired rounds with equal output in
11/11. Raw setup-adjusted wall deltas are directional only because 0/11 rounds
had clean IPC counters. RSS is maximum process RSS.

| Screen | Raw wall median | Work RSS change | Setup RSS change | Decision |
| --- | ---: | ---: | ---: | --- |
| Index worklist, 1/4 and 1/4 vs direct-slot control | -1.24% (7/11 wins; -16.07% to +2.61%) | -159,744 B | -172,032 B | Too noisy/small to retain |
| Index worklist, reusable vs non-reusable, 1/4 and 1/4 | -2.14% (8/11 wins; -8.45% to +19.34%) | -45,056 B | -16,384 B | Too small to retain |
| 1/8 headroom and 1/8 full-growth vs 1/4 policy | +26.45% slower | -4,956,160 B | +2,310,144 B | Reject on speed and stock gate |
| 1/8 headroom and 1/4 full-growth vs 1/4 policy | +1.68% slower (4/11 wins; -9.45% to +53.98%) | -315,392 B | +147,456 B | No qualified win |
| 1/8 headroom and 1.0 full-growth vs 1/4 policy | -11.76% (9/11 wins; -49.19% to +28.20%) | +28,717,056 B | -413,696 B | Reject on RSS and stock gate |
| Slab trim vs untrimmed split policy | -15.38% (-50.27% to +65.46%) | -61,440 B | -53,248 B | Revert: weak-generation aliasing |

Fresh stock runs were valid and output-equal 11/11:

| Policy | Quench Score | Node --jitless Score | Quench max RSS | Bun no-JIT max RSS | Decision |
| --- | ---: | ---: | ---: | ---: | --- |
| 1/8 headroom and 1/8 full-growth | 1,448 | 5,419 | 64,258,048 B | 61,751,296 B | Fails both local bars |
| 1/8 headroom and 1.0 full-growth | 1,379 | 2,296 | 95,940,608 B | 62,013,440 B | Fails both local bars |

The 1/8 + 1/8 profile completed 55 full and zero minor collections; marking
took 1.6626 s and sweeping 0.2698 s in the instrumented run. The split 1/8 +
1/4 profile completed 33 full and 23 minor collections; marking took 0.7973 s
(0.7223 s full, 0.0750 s minor), sweeping 0.1701 s, with 65,579 remembered
scans and 1,571,022 young cells swept. The 1/8 + 1.0 raw fixed-work wall
improvement did not transfer to stock Score and came with a 28.7 MB work-RSS
increase.

The slab-trim code truncated generation metadata together with trailing empty
slot slabs. Runtime checking exposed weak-handle generation aliasing after
metadata truncation. That implementation was discarded; its timing is not a
candidate result.

## Reproduction

Fixed-work paired screen, replacing the two Quench paths and output path for
each row in the table:

    work/task94-gc-40b-quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js --fixed-work --quench work/task94-40b-gc-full-025-early-bitcheck-quench-node --quench-peer work/task94-gc-index-worklist-quench-node --runs 11 --timeout-ms 300000 --out work/task94-gc-index-worklist-vs-control-fixed-11.json

Stock check, replacing the candidate and output path for each policy with a
stock report:

    work/task94-gc-40b-quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js --quench work/task94-gc-index-worklist-0125-quench-node --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs --bun target/stageb-tools/bun/bun-linux-x64/bun --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node --runs 11 --timeout-ms 300000 --out work/task94-gc-index-worklist-stock-11.json

Compressed raw reports in this directory:

- fixed-work-index-025-11.json.gz
- fixed-work-index-0125-0125-11.json.gz
- stock-index-0125-0125-11.json.gz
- profile-index-0125-0125-1.json.gz
- fixed-work-index-0125-025-11.json.gz
- profile-index-0125-025-1.json.gz
- fixed-work-slab-trim-0125-025-11.json.gz
- fixed-work-index-0125-1x-11.json.gz
- stock-index-0125-1x-11.json.gz
- fixed-work-worklist-reuse-025-11.json.gz

The pinned Splay M4 bars and broader qualification protocol are in
tasks/61.md on the v2-lanes source: Node --jitless marginal cycles 4,590,844
per run and Bun no-JIT maximum RSS 57,950,208 B. These Linux screens do not
close either M4 gate.
