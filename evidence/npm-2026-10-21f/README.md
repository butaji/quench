# npm top-300 with more lane calls at df147a2f1 (2026-10-21)

Same runner, host and engines as `../npm-2026-10-21` (x86_64 Linux 6.18.44 VM, 4 CPUs,
16 GiB; Node v26.12.0, Bun 1.4.2). Quench: `tools/pgo-build.sh production` at df147a2f1 on
`v2-npm` (profile-guided, OpenSSL linked statically, `quench-alloc`); JIT mode runs
`quench-node --jit` through an `exec` wrapper. The measured no-JIT run follows one full run of
the same build, which fills its compile cache and startup snapshot; the JIT run follows the
measured one. Reports come from `../npm-2026-10-21/report.mjs`.

## Results

| | no-JIT | JIT | Node | Bun |
| --- | --- | --- | --- | --- |
| behaviour | all pass | all pass | | |
| median wall | 42.1 ms | 45.0 ms | 95.5 ms | 61.8 ms |
| median CPU | 41.2 ms | 43.8 ms | 114.6 ms | 66.1 ms |
| median peak RSS | 14.1 MiB | 14.4 MiB | 63.3 MiB | 37.4 MiB |
| fastest of the three (wall) | 265 | 266 | | |
| fastest of the three (CPU) | 284 | 283 | | |
| lowest peak RSS | 293 | 291 | | |

Node and Bun columns are from the no-JIT run. 286 behaviour and 13 declaration scenarios
pass; the musl rollup addon fails on Node too (oracle-failed). Against `../npm-2026-10-21e`
(267 / 284 / 292) the counts are within this host's run-to-run spread.

## What changed since `../npm-2026-10-21e`

- **`in` with a string key over ordinary objects is decided in the lane** from the shapes up
  to the null prototype (typescript-eslint left the lane 209,589 times at one `in`).
- **The lane calls every native the general loop's native branch calls**, all but the
  call-forwarding natives, direct `eval` and `with` entry and exit, instead of a list.
- **`Function.prototype.call` and argument-free bound functions forward in the lane** to a
  frameless function's activation or a native call in place: Node's primordials
  (`call.bind(fn)`) and `hasOwnProperty.call(o, k)` no longer leave it.

## Peak RSS

The RSS losses are declaration scenarios and `typescript`, whose peak is the TypeScript 7
native compiler child they spawn, the same process in every engine (see `../npm-2026-10-21c`);
the differences are 0.0-0.8 MiB and change packages between runs.

## Where Quench still loses on wall time

The compute-bound scenarios the interpreter runs: jiti (Babel), color-convert, semver,
schema-utils and the ajv family, playwright, the typescript-eslint family, Babel; undici,
whose Bun import is Bun's built-in implementation. Their profiles are flat interpreter cost
(native call entry, array callbacks, property lookup, collection), with no single hot path
left; color-convert's real scenario runs 1.24G instructions.
