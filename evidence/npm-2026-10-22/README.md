# npm top-300 with cheaper RegExp compiles and lane views at 4136b7df8 (2026-10-22)

Same runner, host and engines as `../npm-2026-10-21` (x86_64 Linux 6.18.44 VM, 4 CPUs,
16 GiB; Node v26.12.0, Bun 1.4.2). Quench: `tools/pgo-build.sh production` at 4136b7df8 on
`v2-npm` (profile-guided, OpenSSL linked statically, `quench-alloc`); JIT mode runs
`quench-node --jit` through an `exec` wrapper. The measured no-JIT run follows one full run of
the same build, which fills its compile cache and startup snapshot; the JIT run follows the
measured one. Reports come from `../npm-2026-10-21/report.mjs`.

## Results

| | no-JIT | JIT | Node | Bun |
| --- | --- | --- | --- | --- |
| behaviour | all pass | all pass | | |
| median wall | 39.6 ms | 43.5 ms | 89.9 ms | 61.2 ms |
| median CPU | 38.9 ms | 42.6 ms | 108.6 ms | 65.6 ms |
| median peak RSS | 14.1 MiB | 14.4 MiB | 63.4 MiB | 38.0 MiB |
| fastest of the three (wall) | 268 | 268 | | |
| fastest of the three (CPU) | 287 | 282 | | |
| lowest peak RSS | 291 | 289 | | |

Node and Bun columns are from the no-JIT run. 286 behaviour and 13 declaration scenarios
pass; the musl rollup addon fails on Node too (oracle-failed).

## What changed since `../npm-2026-10-21f`

- **A lane view is derived after four view-less runs** of a function instead of two (a view
  costs about a thousand instructions per bytecode instruction; jiti -3.3% instructions).
- **A RegExp pattern is parsed once when its matcher compiles**: a constructed pattern no
  longer parses for the stack and string-property checks first, and a literal's pattern,
  which passed its program's early errors, skips the text checks too (jiti -2.9%).
- **A case-folded literal's ASCII start mask comes from its canonical form** instead of
  canonicalizing all 128 ASCII units, and **start-set analysis keeps only masks**: the
  128-flag tables are derived once for the sets a RegExp program matches with (a compile
  benchmark of large alternations: -9.7%).
- jiti 235 -> 176 ms, semver 198 -> 142 ms, color-convert 212 -> 176 ms wall against
  `../npm-2026-10-21f`.

## Peak RSS

The RSS losses are declaration scenarios and `typescript`, whose peak is the TypeScript 7
native compiler child they spawn, the same process in every engine (see `../npm-2026-10-21c`);
the differences are under 1 MiB and change packages between runs.

## Where Quench still loses on wall time

color-convert, jiti, schema-utils, semver, lucide-react, whatwg-url, playwright, Babel and
the typescript-eslint family run compute the interpreter makes slower than Bun's JIT; undici
is Bun's built-in implementation; ws, axios and node-fetch are within this host's spread.
