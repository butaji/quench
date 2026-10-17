# Peak RSS on the 300 npm package scenarios: Quench, Node, Bun (2026-10-17)

The owner's requirement: Quench has the lowest peak RSS among Node and Bun on every
npm package execution. This is the first measurement.

`target/iteration/run-npm NODE QUENCH REPORT 60 --rss BUN` runs each package's
`tests/npm-top300/scenario.mjs` as a fresh process under each engine. It records the
peak resident set size from `wait4`'s `ru_maxrss`, which covers the process and the
descendants it reaped (so `tsc` and `vitest` children count).

- Quench: release `quench-node` at 54aeec881 (sha256 `dabbc4d4534d…`).
- Node v26.12.0 and Bun 1.4.2.
- Host: `host.txt` (x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB RAM, no cgroup limits).
- Raw data: `rss-scenarios.json.gz`. The table comes from `rss_report.py`, in
  `rss-report.txt`.

## Result

Behaviour: all 300 scenarios pass on Quench (286 behavior, 13 declarations). The musl
rollup addon is oracle-failed: it also fails on glibc Node.

| | Quench | Node | Bun |
| --- | --- | --- | --- |
| median peak RSS | 27.5 MiB | 71.9 MiB | 36.3 MiB |

Quench is lowest on **252 of 300** packages and not lowest on 48. Those 48 are the
code-heavy workloads. The worst are typescript-eslint and @typescript-eslint/* at
414 MiB against Bun's 144 MiB and Node's 233 MiB, then playwright, @testing-library/dom,
jsdom, vitest, babel and the declaration packages that run `tsc`.

## What the gap is made of (first census)

- **Live memory, not allocator retention:** glibc tunables (`MALLOC_MMAP_THRESHOLD_`,
  `MALLOC_TRIM_THRESHOLD_`, `MALLOC_ARENA_MAX`) do not move any of these peaks.
- **Code memory per byte of source:** `require("typescript")` peaks at 132 MiB
  (Node 111, Bun 126). Compiling `typescript.js` (9 MB) holds a 55 MB OXC AST arena,
  and leaves about 45 MB of malloc'd compiled programs and a 30 MB JS heap. Quench
  compiles every function body before running the module. V8 and JSC compile bodies
  on first call, so code that a scenario never runs costs them nothing.
- **Fixed library cost:** the scenario's own imports (assert/strict, fs, stream, http,
  url, child_process) cost Quench 25.1 MiB against Bun 22.5. `assert/strict` alone is
  24.8 MiB and `fs` 21.7 MiB, against Bun's 13.5 and 13.8. An empty script is lowest on
  Quench: 10.6 MiB against Bun 12.1 and Node 48.7.
