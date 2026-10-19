# npm top-300 speed, no-JIT and JIT, against Node and Bun (2026-10-19)

This is the first speed measurement of the npm suite. `run-npm` records each process's wall
time (spawn to reap, polled every millisecond) and its CPU time (`wait4`'s user plus system
time), beside peak RSS. `report.mjs` and `compare.mjs` run on `quench-node` itself.

- Host: `host.txt` (x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB, no cgroup limits).
- Node v26.12.0 and Bun 1.4.2.
- Quench: dynamic release `quench-node` from `v2-npm`.
  - Baseline: 9962b1a58.
  - Candidate: 77d5c81ac.
- JIT mode runs `quench-node --jit` through an `exec` wrapper. Child processes a scenario
  starts run without `--jit`.
- Candidate runs use a compile cache warmed by one earlier scenario
  (`QUENCH_COMPILE_CACHE`, see `docs/README.md`, "Host compile cache").

## Results

| | baseline no-JIT | no-JIT | JIT | Node | Bun |
| --- | --- | --- | --- | --- | --- |
| behaviour | all pass | all pass | all pass | | |
| median wall | 513.5 ms | 203.7 ms | 228.4 ms | 93.6 ms | 63.0 ms |
| median CPU | 506.7 ms | 201.8 ms | 218.5 ms | 110.4 ms | 66.6 ms |
| median peak RSS | 18.5 MiB | 19.4 MiB | 19.5 MiB | 63.3 MiB | 38.3 MiB |
| fastest of the three (wall) | 3 | 0 | 0 | | |
| lowest peak RSS | 282 | 282 | 281 | | |

"All pass" counts 286 behaviour and 13 declaration scenarios. The musl rollup addon fails on
Node too (oracle-failed), so 299 count. Per-package tables are in `nojit-report.txt`,
`jit-report.txt` and `baseline-nojit-report.txt`; raw records are in the `*.json.gz` files.

Quench is not yet faster than Node or Bun on any package. The median fell 2.5× (513.5 →
203.7 ms), but it is still 2.2× Node's and 3.2× Bun's.

## What changed (v2-npm 9962b1a58..77d5c81ac)

- **JIT mode crash.** `--jit` panicked on the first function released for compile on first
  call. `js_tier_may_claim` read the slot directly; it now uses the accessor.
- **Compiler.**
  - Interning walked every atom of the VM's shared prefix for each program; it now interns
    only the new atoms.
  - Frame-register promotion now reuses the plain-slot proof instead of deriving it again.
  - A function released as soon as it compiles now skips lowering.
- **Compile cache** (`docs/README.md`, "Host compile cache").
  - Node's library modules and quench-node's host scripts compile once per build.
  - Later runs decode a compact, checksummed form under their own VM's atom ids.
  - Decoding is about a tenth of compiling. A warm lodash run compiles 14 programs instead
    of 135.
- **Module loading.**
  - Each `package.json`'s `"type"` is read once per thread, as Node caches package configs.
    date-fns reparsed a 199 KB `package.json` for each of its 316 modules: 6.5 s → 1.0 s.
  - An import statement's module is resolved once for all its bindings.
  - The builtin export table is parsed once.
  - Module export slots are found by hash. date-fns: 1.0 → 0.40 s (Node 1.4 s).
- **Code page releases.** After library loads, read-only code pages are released at most
  once per 100 ms. Releases at collection boundaries are unchanged, and V8-v7 max RSS
  moves within 0.1 MiB.

## Where the time goes now

A warm `ms` scenario is 117–150 ms; Node takes about 68 ms and Bun about 47 ms.

- **User time (about 100 ms).**
  - About 28% is Node's `primordials.js` running at start-up. Node runs it from its startup
    snapshot.
  - About 16% is compiling the scenario file and the package itself.
- **System time (35–50 ms).** This is page faults: 35 thousand against Node's 5 thousand.
  The memory-first page policy causes them: read-only code pages are released at every
  collection and faulted in one page at a time.
  - Dropping the collection-boundary release alone sped scenarios up 10–27% for 1–3 MiB more
    RSS.
  - It also raised V8-v7 max RSS (NavierStokes 5236 → 5516 KiB with even a 10 ms interval),
    which the RSS lane's QuickJS margin needs. So it is unchanged here.
  - A variant that kept the pages every interval touched cut faults but did not speed runs
    up, and it raised V8-v7 max RSS by 0.1–0.9 MiB. It was not kept.
- **Collections.** Pacing collects young allocations every 1/64 of the live set, which is
  the memory-first direction of 2026-10-17. semver collects about every 1.5 ms.

Reaching Node's or Bun's speed needs larger changes:
- a start-up snapshot, or native primordials and lazily built builtins;
- a page and collection policy that trades some of the RSS margin over Bun for speed.
