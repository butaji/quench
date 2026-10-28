# npm top-300 on the profile-guided build at 2ca8d0562 (2026-10-21)

Same runner, host and engines as `../npm-2026-10-21` (x86_64 Linux 6.18.44 VM, 4 CPUs,
16 GiB; Node v26.12.0, Bun 1.4.2). Quench: `tools/pgo-build.sh production` at 2ca8d0562 on
`v2-npm` (profile-guided, OpenSSL linked statically, `quench-alloc`); JIT mode runs
`quench-node --jit` through an `exec` wrapper. The measured no-JIT run follows one full run of
the same build, which fills its compile cache and startup snapshot; the JIT run follows the
measured one. Reports come from `../npm-2026-10-21/report.mjs`.

## Results

| | no-JIT | JIT | Node | Bun |
| --- | --- | --- | --- | --- |
| behaviour | all pass | all pass | | |
| median wall | 42.3 ms | 43.7 ms | 93.3 ms | 64.3 ms |
| median CPU | 40.5 ms | 42.3 ms | 112.9 ms | 67.9 ms |
| median peak RSS | 14.2 MiB | 14.4 MiB | 72.0 MiB | 37.4 MiB |
| fastest of the three (wall) | 267 | 264 | | |
| fastest of the three (CPU) | 284 | 280 | | |
| lowest peak RSS | 292 | 292 | | |

Node and Bun columns are from the no-JIT run. 286 behaviour and 13 declaration scenarios
pass; the musl rollup addon fails on Node too (oracle-failed).

Fastest wall / CPU / lowest RSS (no-JIT) over the day's runs:

| run | wall | CPU | RSS |
| --- | --- | --- | --- |
| `../npm-2026-10-21` (before the day's work) | 153 | 193 | 288 |
| `../npm-2026-10-21b` module bodies specialized | 208 | 243 | 294 |
| `../npm-2026-10-21c` profile-guided, `quench-alloc` | 257 | 277 | 290 |
| `../npm-2026-10-21d` page releases, module identity | 260 | 276 | 292 |
| this run | 267 | 284 | 292 |

## What changed since `../npm-2026-10-21d`

- **String keys keep their atoms** between collections; `getOwnPropertyDescriptor` looks a
  property up once; lazy-member checks return at once for shapes without getter-and-setter
  accessors; `ToPropertyDescriptor` reads through a cached field layout. typescript-eslint:
  5,736M -> 5,189M instructions.
- **Start-set unions move their unit sets** instead of copying the growing test per branch
  (16% of the jiti scenario).
- **A lane view decodes its function's instructions once.**
- **super() updates this bindings from the owning constructor's activation up** (3.6% of
  ajv-keywords).
- **An ES module's unresolved names are globals** read through the global object's cache, as a
  script function's are (baseline-browser-mapping looked up `isNaN` and `Number` by
  environment search 48,000 times).

## Peak RSS

The no-JIT and JIT RSS losses are declaration scenarios and `typescript`, whose peak is the
TypeScript 7 native compiler child they spawn, the same process in every engine (see
`../npm-2026-10-21c`); the differences are 0.1-0.8 MiB and change packages between runs.

## Where Quench still loses on wall time

Compute-bound scenarios the interpreter runs (the JIT compiles only small numeric and string
functions): color-convert, jiti, schema-utils and the ajv family, playwright, whatwg-url,
browserslist, the typescript-eslint family, Babel, `@testing-library/dom`, vitest; and undici,
whose Bun import is Bun's built-in implementation.
