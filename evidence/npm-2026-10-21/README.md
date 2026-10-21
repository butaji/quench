# npm top-300 after the v2-merged merge, no-JIT and JIT, against Node and Bun (2026-10-21)

`run-npm` records each process's wall time, CPU time and peak RSS, as in
`../npm-snapshot-2026-10-20`. `report.mjs` runs on `quench-node` itself.

- Host: x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB, no cgroup limits (as `../npm-speed-2026-10-19/host.txt`).
- Node v26.12.0 and Bun 1.4.2.
- Quench: release `quench-node` from `v2-npm`.
  - Candidate: e325404a4 (merge of `origin/v2-merged` at acd849678,
    npm-lane young pacing, module-graph and real-path caching).
  - `trunk-pacing-*`: the same merge with trunk's pacing table (a98f7b494).
- JIT mode runs `quench-node --jit` through an `exec` wrapper.
- Each measured run follows one full run of the same build, which fills its compile cache
  and builds its startup snapshot.
- The machine ran slower than on 2026-10-20 (Node's median wall 100 ms against 93), so compare
  engines within a run, not across days.

## Results

| | no-JIT | JIT | trunk pacing (no-JIT) | Node | Bun |
| --- | --- | --- | --- | --- | --- |
| behaviour | all pass | all pass | all pass | | |
| median wall | 58.9 ms | 61.6 ms | 63.1 ms | 100.6 ms | 62.4 ms |
| median CPU | 57.9 ms | 60.6 ms | 62.4 ms | 121.2 ms | 66.4 ms |
| median peak RSS | 14.8 MiB | 14.8 MiB | 14.4 MiB | 63.3 MiB | 35.8 MiB |
| fastest of the three (wall) | 153 | 147 | 143 | | |
| fastest of the three (CPU) | 193 | 198 | 192 | | |
| lowest peak RSS | 288 | 290 | 289 | | |

Node and Bun columns are from the no-JIT run. "All pass" counts 286 behaviour and 13
declaration scenarios; the musl rollup addon fails on Node too (oracle-failed), so 299 count.
Per-package tables are in `nojit-report.txt`, `jit-report.txt` and `trunk-pacing-report.txt`;
raw records are in the `*.json.gz` files.

## What changed since 2026-10-20

- **Module bodies keep private bindings in registers.** An ES module body kept every local in
  its module environment and cloned it on every loop iteration. Only the bindings its link plan
  addresses (imports, exported locals, hoisted functions) need the environment now. A top-level
  loop ran 118 ms -> 3.0 ms (Node --jitless 3.4 ms).
- **Static modules compile and cache specialized.** The compile cache now keys and decodes static
  modules by the VM's specialization. vite warm: 423 -> 371 ms. ES modules cost about 1 MiB more
  peak RSS (inline caches).
- **Module graphs resolve each export once per edge and name.** date-fns's index (about 250
  `export *`) made 61,256 ResolveExport steps. Star re-exports are now indexed by the names they
  export, and module edges and real paths are memoized (as Node's `realpathCache`).
  date-fns: 420 -> 196 ms; readlink calls 64,847 -> 2,590.
- **Young pacing.** The npm lane's eighth-of-a-live-set young pacing, kept as its own commit over
  trunk's memory-first table. Matched against trunk's table (`trunk-pacing-*`): median wall 63.1
  -> 57.9 ms, fastest 143 -> 164 of 299, median RSS 14.4 -> 14.7 MiB.
- **Node surface.**
  - `require('constants')`, `punycode`, `readline/promises` and `node:test/reporters` load Node's
    own modules; they were empty objects.
  - crypto, fs and `require('constants')` constants match Node in keys, order, values and
    descriptors. The one exception is `OPENSSL_VERSION_NUMBER`, which reports the linked OpenSSL.
  - `_stream_*` are no longer builtins, as in Node 26.

## Where Quench still loses

- **Wall time, compute.**
  - Packages whose scenarios run a lot of JavaScript: the TypeScript compiler
    (typescript-eslint family), Babel (2.1–2.8x the faster engine), jsdom, `@testing-library/dom`,
    vitest, playwright, jiti (Babel inside), zod, ajv.
  - The JS JIT compiles only small numeric functions, so the interpreter runs these.
- **Wall time, per process.** undici (4.3x), `baseline-browser-mapping` and whatwg-url load more
  library code than the startup snapshot holds.
- **color-convert.** Bun's 66 ms is not comparable: Bun fails the scenario's exact binary64
  assertions and stops early. Node takes 130 ms; Quench's cost is `assert.deepEqual` over 2,176
  cases.
- **Peak RSS.**
  - The `@types/*`, `type-fest`, `undici-types` and `typescript` declaration scenarios are within
    1 MiB of Bun.
  - `@typescript-eslint/type-utils` is 9.5 MiB above Bun.
- **Node gaps found on the way.**
  - The `_http_*` builtins are still empty objects; Node builds them on its `http_parser`
    binding, which the native `http` module does not use.
  - `inspector.Session` is missing, so `inspector/promises` throws.
  - `process.versions.openssl` is hardcoded.
