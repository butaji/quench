# Richards: disabled VFS startup experiment, Linux, 2026-10-14

TOD-120, `v2-cloud`. Matched source is `e54557a9d17163dae0280fc6c025f285e69d7b4d`
plus `source.patch`. `manifest.json` records executable and patch hashes. The
control includes the reviewed c2 direct-record prototype-cache change; c2
workspace/profile checks and all 582 runtime tests passed. Both measured Node
binaries use the ordinary production profile, without PGO or guest JIT.

The candidate defers experimental VFS setup while disabled. Fs/Stream remain
eager: their guest factories capture mutable language intrinsics. The larger
full-Fs deferral was not retained. Canonical Buffer/Fs aliases and existing Fs
fallback methods now belong to those modules. A single NodeHost query derives
availability from CLI switches and supplies bootstrap and module visibility.
The last switch wins; option values and eval source cannot enable VFS.

## Richards result

Eleven fixed-work pairs all completed with equal output. Median work RSS falls
from 44,314,624 to 40,046,592 bytes; the candidate uses less in all 11 pairs.
The paired median marginal wall change is -5.34%, with bootstrap 95% interval
[-14.03%, +0.81%]. This does not establish a speed improvement. Fixed-work
reports have zero clean rounds because Linux hardware counters are unavailable;
raw wall values are diagnostic. Defunct rustc processes in host snapshots have
RSS zero and historical CPU percentages; no builds overlapped these timings.

The separate stock blocks each contain 11 interleaved four-engine rounds:

| Engine | Control Score / peak RSS bytes | Candidate Score / peak RSS bytes |
| --- | ---: | ---: |
| Quench | 361 / 44,544,000 | 352 / 40,275,968 |
| QuickJS | 726 / 2,162,688 | 778 / 2,162,688 |
| Bun without JIT | 1,184 / 17,604,608 | 1,266 / 17,735,680 |
| Node without JIT | 1,223 / 46,153,728 | 1,207 / 46,153,728 |

The stock Score difference and all three reference-normalized differences have
wide intervals including no change (`vfs-stock-comparison-analysis.json`). They
are sequential-block screens, not a paired Score gate. Richards still loses
both best-engine bars. This package is not a qualified win or a measurement of
an irreducible floor. TOD-104 K2-K4 remain open.

## Startup and semantic checks

The first empty-process wall block was noisy and is retained as raw evidence.
An idle-host repeat gives paired median wall change -13.71%, bootstrap interval
[-18.32%, -9.98%]; empty RSS falls from 43,053,056 to 39,219,200 bytes. Independent
Callgrind startup instruction totals fall from 1,150,546,244 to 1,001,304,086
(-12.97%). These explain startup cost, not stock Score.

Native pinned Node VFS fixtures retain 98 passes and 1 failure across 99 cases.
Fs FileHandle fixtures retain 8 passes and 11 failures across 19 cases. Inventories
and every outcome agree with the matched control, with no new failure. Fixtures
are the unmodified pinned Node checkout at
`55e4ca33f3a6dfe364bd9558b726576a6d1a2fba`, materialized outside the repository
package scope under `/tmp/quench-node-fixtures`. No harness or host shims.

`oracle/matched-oracle.json` preserves actual Node/control/candidate observations.
FileHandle write/read/sync/datasync/stat/close/utimes, canonical module identity,
child-process Buffer outputs, VFS visibility and mutated Object.defineProperty
behavior agree. Last-disable flag ordering is fixed to match Node. Existing
metadata/facade gaps and the existing compression TypeError are unchanged;
neither is counted as a pass.

All eight fixed-work fixtures complete with equal output and reduced median
RSS in a three-round guard. Eleven-round repeats resolve the apparent RayTrace
and EarleyBoyer slowdowns: paired median wall changes are -0.81%
[-3.26%, +1.00%] and -0.72% [-9.27%, +3.50%], respectively. Neither confirms
a regression. RegExp setup subtraction has nonpositive samples
and cannot support a marginal timing claim. This guard is not all-eight stock
qualification and does not replace either host's win protocol.

## Commands

Build with the repository toolchain/environment:

```sh
source /workspace/quench-build-env.sh
cargo +1.99.0 check --workspace --all-targets
cargo +1.99.0 build --profile production -p quench-node --bin quench-node
cargo +1.99.0 build --profile production -p quench-node-test --bin run-parallel
```

Fixed work, with separately pinned control and candidate binaries:

```sh
work/pinned/quench-bench quench-bench/js-engine-benchmark/v8-v7/richards.js \
  --fixed-work --quench work/pinned/richards-c2-control/quench-node \
  --quench-peer work/pinned/richards-vfs-candidate/quench-node \
  --runs 11 --timeout-ms 300000 --out REPORT.json
```

Use `--all --runs 3` for the initial guard. Stock blocks omit fixed-work/peer,
set `--runs 11`, and explicitly select the pinned QuickJS, Bun and Node paths
recorded in each report. Empty-process pairs alternate process order, use
`-e ''`, monotonic wall timing and Linux wait4 rusage. Callgrind executes each
production binary with `-e ''`; its gzip files preserve complete profiles.
