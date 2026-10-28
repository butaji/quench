# npm top-300 on the profile-guided build with the small-object allocator (2026-10-21)

Same runner, host and engines as `../npm-2026-10-21` (x86_64 Linux 6.18.44 VM, 4 CPUs,
16 GiB; Node v26.12.0, Bun 1.4.2). Quench: `tools/pgo-build.sh production` at 120d61fac on
`v2-npm` (profile-guided, OpenSSL linked statically, `quench-alloc` the global allocator);
JIT mode runs `quench-node --jit` through an `exec` wrapper. The measured no-JIT run follows
one full run of the same build, which fills its compile cache and startup snapshot; the JIT
run follows the measured one. Reports come from `../npm-2026-10-21/report.mjs`.

## Results

| | no-JIT | JIT | Node | Bun |
| --- | --- | --- | --- | --- |
| behaviour | all pass | all pass | | |
| median wall | 48.2 ms | 48.1 ms | 109.0 ms | 68.4 ms |
| median CPU | 45.5 ms | 46.3 ms | 125.0 ms | 70.2 ms |
| median peak RSS | 13.9 MiB | 14.1 MiB | 63.4 MiB | 36.4 MiB |
| fastest of the three (wall) | 257 | 252 | | |
| fastest of the three (CPU) | 277 | 265 | | |
| lowest peak RSS | 290 | 292 | | |

Node and Bun columns are from the no-JIT run. 286 behaviour and 13 declaration scenarios
pass; the musl rollup addon fails on Node too (oracle-failed).

Steps on the same day (no-JIT fastest wall / CPU, lowest RSS, of 299):

| build | wall | CPU | RSS |
| --- | --- | --- | --- |
| plain release, module bodies specialized (`../npm-2026-10-21b`) | 208 | 243 | 294 |
| profile-guided | 230 | 263 | 293 |
| profile-guided + `quench-alloc` + no exit teardown (this run) | 257 | 277 | 290 |

## What changed since `../npm-2026-10-21b`

- **Production build.** The production profile is profile-guided (`tools/pgo-build.sh`, trained
  on the interpreter curriculum and the Node compatibility fixtures, never on these scenarios).
- **The npm lane links `quench-alloc` again.** The merge of trunk's memory-first lane settings
  had turned its default off.
- **The CLI leaves its heap to the process exit.** Freeing the heap and programs one
  allocation at a time was 11% of an empty script's instructions.

## Peak RSS: the declaration scenarios

Every RSS loss is a declaration scenario. Each runs the package's `tsc` twice through
`spawnSync(process.execPath, ...)`, and the installed TypeScript 7 `tsc` starts its native
(Go) compiler, so the scenario's peak (`wait4` counts descendants) is that compiler's, the same
in every engine: Quench and Bun differ by 0.0-0.8 MiB in either direction from run to run
(no-JIT loses 9, JIT 7, different packages). The harness process itself peaks near 14 MiB.

## Where Quench still loses on wall time

- **Compute.** jiti (Babel), color-convert (`assert.deepEqual` over 2,176 cases; Bun fails the
  scenario's exact assertions and stops early), `@testing-library/dom`, schema-utils and the ajv
  family (validators compiled with `new Function`), playwright, the typescript-eslint family.
  The JIT compiles only small numeric and string functions.
- **Per process.** undici (Bun answers `undici` with its own built-in implementation, 15 ms to
  import against Quench's 125 and Node's 137), ws, whatwg-url, node-fetch.
