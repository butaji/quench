# npm top-300 with the startup snapshot, no-JIT and JIT, against Node and Bun (2026-10-20)

`run-npm` records each process's wall time, CPU time and peak RSS, as in
`../npm-speed-2026-10-19`. `report.mjs` runs on `quench-node` itself.

- Host: x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB, no cgroup limits (as `../npm-speed-2026-10-19/host.txt`).
- Node v26.12.0 and Bun 1.4.2.
- Quench: release `quench-node` from `v2-npm`.
  - Baseline: d19, before the startup snapshot (`baseline-*`).
  - Candidate: e6a395810.
- JIT mode runs `quench-node --jit` through an `exec` wrapper.
- Each candidate run follows one full run of the same build, which fills its compile cache and
  builds its startup snapshot (`docs/README.md`, "Host compile cache" and "Startup snapshot").

## Results

| | baseline no-JIT | no-JIT | JIT | Node | Bun |
| --- | --- | --- | --- | --- | --- |
| behaviour | all pass | all pass | all pass | | |
| median wall | 96.1 ms | 54.5 ms | 57.2 ms | 93.0 ms | 50.9 ms |
| median CPU | 92.5 ms | 51.9 ms | 51.5 ms | 105.7 ms | 53.0 ms |
| median peak RSS | 17.1 MiB | 14.9 MiB | 14.9 MiB | 70.2 MiB | 34.5 MiB |
| fastest of the three (wall) | 2 | 107 | 113 | | |
| fastest of the three (CPU) | 2 | 155 | 159 | | |
| lowest peak RSS | 287 | 283 | 287 | | |

Node and Bun columns are from the no-JIT run; they vary by a few ms between runs (Bun's
median wall was 55.6 ms in a run of the previous candidate). "All pass" counts 286 behaviour
and 13 declaration scenarios; the musl rollup addon fails on Node too (oracle-failed), so 299
count. Per-package tables are in `nojit-report.txt`, `jit-report.txt` and
`baseline-nojit-report.txt`; raw records are in the `*.json.gz` files.

## What changed

- **Startup snapshot.** A process restores Node's bootstrap with the builtins nearly every
  program loads (events, buffer, util, path, fs, url, timers, querystring, async_hooks,
  diagnostics_channel, stream, module, child_process, http) instead of running it, as Node
  does from its own snapshot. The scenario harness's imports cost about 40 ms of every run
  before; restoring them costs a few. lodash: wall 135 → 67 ms, CPU 136 → 67 ms.
- **Resident pages.** A restore releases the snapshot's pages as it reads them. Registering a
  read-only mapping for touched-pages-only faults had failed for every file whose length is
  no page multiple (compile-cache entries), so those mappings kept fault-around; huge pages are
  now refused for them too, which had mapped 4 MB of the snapshot on its first record reads.
  Together these took the median peak RSS below the baseline's.
- **Math.random** draws a per-process seed, as Node's does; every run had drawn the same
  sequence.

## Where Quench still loses

- **Wall time.** Packages whose scenarios run a lot of JavaScript: the TypeScript compiler
  (typescript-eslint family, 2.8–2.9× Bun), Babel (3.0×), jsdom, vitest, playwright, and
  compute loops in color-convert, semver and date-fns. The interpreter runs these; the JS JIT
  compiles none of their functions (functions over 256 instructions, or with handlers,
  generators or async bodies, stay interpreted).
- **Peak RSS.** The typescript-eslint family (12–52 MiB above Bun) holds the TypeScript
  compiler's program; the `@types/*`, `type-fest`, `undici-types` and `typescript`
  declaration scenarios are within 1.2 MiB of Bun, where the type checker child process
  dominates every engine's peak.
