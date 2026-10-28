# npm top-300 on the profile-guided build, page releases and module names cheaper (2026-10-21)

Same runner, host and engines as `../npm-2026-10-21` (x86_64 Linux 6.18.44 VM, 4 CPUs,
16 GiB; Node v26.12.0, Bun 1.4.2). Quench: `tools/pgo-build.sh production` at 1e226bd5e on
`v2-npm` (profile-guided, OpenSSL linked statically, `quench-alloc`); JIT mode runs
`quench-node --jit` through an `exec` wrapper. The measured no-JIT run follows one full run of
the same build, which fills its compile cache and startup snapshot; the JIT run follows the
measured one. Reports come from `../npm-2026-10-21/report.mjs`.

## Results

| | no-JIT | JIT | Node | Bun |
| --- | --- | --- | --- | --- |
| behaviour | all pass | all pass | | |
| median wall | 46.7 ms | 48.0 ms | 108.7 ms | 70.4 ms |
| median CPU | 45.4 ms | 47.3 ms | 128.8 ms | 74.3 ms |
| median peak RSS | 14.1 MiB | 14.3 MiB | 63.3 MiB | 37.0 MiB |
| fastest of the three (wall) | 260 | 261 | | |
| fastest of the three (CPU) | 276 | 279 | | |
| lowest peak RSS | 292 | 290 | | |

Node and Bun columns are from the no-JIT run. 286 behaviour and 13 declaration scenarios
pass; the musl rollup addon fails on Node too (oracle-failed). Against `../npm-2026-10-21c`
(257 / 252 fastest wall) on the same day.

## What changed since `../npm-2026-10-21c`

- **Compile boundaries release pages at most once per 100 ms.** Each compilation's phase
  boundary released the read-only code and data and read the whole mapping table; functions
  compiled at run time paid it each time (7% of the `@testing-library/dom` scenario's
  instructions). The table is parsed as bytes.
- **Module names that are already normal are their identity** (`module_cache_key` was 2% of the
  same scenario), and the package-type, real-path and string-constant caches hash bytes.
- **`readFileSync(path, 'utf8')`** no longer loads Node's fs option policy when the options are
  unobservable.
- **A lane callee's view is derived on its second call**, as a first general-loop entry already
  defers it.
- `@testing-library/dom` instructions: 4,494M -> 3,912M (with the page-release, identity and
  hashing changes).

## Peak RSS

Every no-JIT RSS loss is a declaration scenario, whose peak is the TypeScript 7 native compiler
child the scenario spawns (see `../npm-2026-10-21c`); the losses are 0.0-0.9 MiB and change
packages from run to run. The JIT run adds jiti (38.3 against 38.2 MiB).

## Where Quench still loses on wall time

The same compute-bound packages as `../npm-2026-10-21c` (jiti, color-convert, schema-utils and
the ajv family, the typescript-eslint family, playwright, `@testing-library/dom`, semver) and
undici, whose Bun import is Bun's built-in implementation. Run-to-run spread on this host is
about 20 ms per scenario, which moves the small ones (ws, fflate, source-map, tinyexec) across
the line between runs.
