# npm top-300 with module bodies specialized, no-JIT and JIT, against Node and Bun (2026-10-21)

Same runner, host and engines as `../npm-2026-10-21` (x86_64 Linux 6.18.44 VM, 4 CPUs,
16 GiB; Node v26.12.0, Bun 1.4.2). Quench: plain release `quench-node` at 61c7a8fcc on
`v2-npm`; JIT mode runs `quench-node --jit` through an `exec` wrapper. The measured no-JIT
run follows one full run of the same build, which fills its compile cache and startup
snapshot; the JIT run follows the measured one. Reports come from `../npm-2026-10-21/report.mjs`.

## Results

| | no-JIT | JIT | Node | Bun |
| --- | --- | --- | --- | --- |
| behaviour | all pass | all pass | | |
| median wall | 58.2 ms | 59.4 ms | 107.5 ms | 67.5 ms |
| median CPU | 55.7 ms | 58.0 ms | 126.7 ms | 71.1 ms |
| median peak RSS | 14.4 MiB | 14.5 MiB | 63.4 MiB | 36.3 MiB |
| fastest of the three (wall) | 208 | 203 | | |
| fastest of the three (CPU) | 243 | 235 | | |
| lowest peak RSS | 294 | 293 | | |

Node and Bun columns are from the no-JIT run. 286 behaviour and 13 declaration scenarios
pass; the musl rollup addon fails on Node too (oracle-failed). Against
`../npm-2026-10-21` (153 / 147 fastest, 288 / 290 lowest RSS) Node's median wall moved 100.6 ->
107.5 ms, so the machine ran slower; compare engines within a run.

## What changed

- **ES module bodies kept the VM specialized.** Evaluating a module's locals cleared the VM's
  specialization flag for the whole evaluation, so every program the module ran (required
  CommonJS code and the jobs it queued included) read and wrote properties without inline
  caches. Every scenario's entry is an ES module. semver's scenario: 1,145M -> 813M
  instructions.
- **RegExp layout proofs per receiver shape.** A literal and a `new RegExp` result differ in
  shape (the `Constructed` transition), so the one-entry builtin-layout, `lastIndex` and
  symbol-method proofs re-verified at about 3,800 instructions on every alternation.
- **Range sets derive their ASCII mask** instead of testing each of 128 units.
- **Liveness lists each instruction's successors once** per analysis instead of per pass
  (functions compiled at run time: ajv's generated validators).
- **A new data property is defined in one shape transition.**

## Where Quench still loses

- **Wall time, compute.** color-convert (`assert.deepEqual` over 2,176 cases; Bun fails the
  scenario's exact assertions and stops early), jiti (Babel inside), schema-utils, ajv and
  ajv-keywords (ajv compiles validators with `new Function`), playwright, the
  typescript-eslint family, `@testing-library/dom`, Babel. The JIT compiles only small numeric
  and string functions, so the interpreter runs these.
- **Wall time, per process.** undici, whatwg-url, ws, node-fetch and agent-base load more
  library code than the startup snapshot holds.
- **Peak RSS.** Five declaration scenarios (`@types/node`, `@types/json-schema`, `@eslint/core`,
  `@types/babel__template`, `@types/yargs`) are within 0.7 MiB of the lowest.
  `@typescript-eslint/type-utils` is now lowest.
