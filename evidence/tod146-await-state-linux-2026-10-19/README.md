# TOD-146: an await's continuation lives in its own reaction handler cell

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB, no cgroup CPU quota; rustc 1.99.1.
Release builds of `quench-node` (`cargo build --release -p quench-node`).

- base `fin2` = `v2-merged` 125f202a (the TOD-139 publish)
- candidate `c146c` = `v2-gc-tod146` 1e36b797

Binary hashes in `binaries.sha256`. Paired rows alternate base and candidate on
the same test. Row format: `round test binary max_rss_kb wall_s exit score`
(V8 rows: `round binary bench score max_rss_mb`). JetStream runs use the
in-tree driver with `--no-prefetch --test=<name>`.

## Change

`Cell::AwaitState { continuation: Option<Box<Continuation>>, realm, promise,
generator, yielded }` is both handlers of the await's promise reaction;
`run_promise_reaction` resumes it directly. Removed: the `suspended` continuation
table, `ContinuationId`, `async_resume_jobs`, two native reaction functions per
await, and every per-collection pass over them.

## Versions measured

| Version | File | doxbee-async | async-fs |
| -- | -- | -- | -- |
| State cell, two natives with it as env, continuation inline | `first-version-pairs-5.txt` | ×1.15, RSS 76.5 → 90.9 MB | ×0.98 |
| + continuation boxed apart (freed on resume) | `boxed-version-pairs-*.txt` | ×1.13, RSS 76.5 → 73.7 MB | ×0.96, 2/9 |
| + state is the reaction handler (no natives) — kept | `async-pairs-7.txt` | ×1.40 7/7, RSS 76.1 → 62.9 MB | ×1.08 5/7 |

The first version kept the 256-byte state alive until the next sweep after its
resume, and charged it as one cell (RSS +14 MB). The boxed version added a third
cell per await to the two natives: 18% more collections on async-fs.

## Kept version

- `async-pairs-7.txt`: doxbee-promise ×1.02 (4/7), RSS unchanged.
- profile-gc-time (unboxed version): doxbee-async `vm_owner_edges` 2.13 → 0.11 s.
- `jetstream-round.txt`: one round of all 30; peak RSS unchanged except
  doxbee-async 76 → 63 MB. Single-round outliers re-run in
  `outlier-pairs.txt`: espree ×1.01 (5/9), web-ssr ×1.02 (6/9), babylon,
  source-map, lazy-collections ×0.98–1.00 (3 pairs); RSS identical.
- `npm-top300-rss.txt` (`name best_other_mib base_mib cand_mib ok ok`): geomean
  RSS ratio 1.0004, none worse by >5%, lowest of three 277 → 277; all pass except
  `@rollup/rollup-linux-x64-musl` (fails on both; musl addon on glibc).
- `v8-pairs-3.txt`: no await paths; geomean ×1.026, RSS unchanged (layout noise:
  crypto ×0.945, navier-stokes ×1.18).
- Runtime lib 731/731 (debug, minor-collection verifier on).
- Node `test/parallel` inventory (iteration build, `--jobs 3 --timeout-secs 10`,
  inventory a417f98a082acec0): 2246 pass, as before. One pass→timeout,
  test-performance-many-marks.js, passes alone twice; one timeout→pass,
  test-stream2-read-sync-stack.js.
