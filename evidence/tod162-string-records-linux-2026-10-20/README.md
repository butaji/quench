# TOD-162: strings as a record family

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`.

- Base `b162`: v2-gc `origin/v2-gc` at the time, which equals v2-merged `e67e2896`.
- Candidate `c162`: this change.
- Hashes are in `binaries.sha256`.

## Change

- **Storage.** Strings leave the 40-byte legacy slots for a 16-byte `JsString` record space,
  using the last free `Value` family code.
- **Access.** `Cell::String` is deleted, so any site that still expects a legacy string fails
  to compile. Every read goes through `Heap::string` / `Heap::is_string`, and every
  allocation through `Heap::alloc_string`.
- **Collection.** A string is an immutable leaf: marking it sets its bit, and it needs no
  write barrier.
- **Capacity.** The space holds `MAX_HEAP_CELLS` records, because strings have no
  legacy-slot fallback.

## Decision rule (TOD-162, set before measuring)

- Score not lower on V8-v7, JetStream and the frameworks.
- Max RSS geomean ≤ ×1.005 per suite.
- npm top 300 RSS ≤ ×1.01.
- Runtime lib passes.

## Result: kept

| Suite | Pairs | Speed geomean | RSS geomean |
| -- | --: | -- | -- |
| V8-v7 | 5 | ×1.003 (regexp ×1.059, 5/5) | ×1.004 |
| JetStream (26 tests; typescript-lib excluded for time) | 3 | ×1.029 | ×0.994 |
| Frameworks | 5 | ×1.019 | ×0.989 |
| Wasm CoreMark (no strings involved) | 5 | ×0.995 | ×1.000 |
| npm top 300 | 1 | — | ×0.9915, no package worse by 5% |

**JetStream highlights**
- source-map ×1.219 (3/3, RSS 56.0 → 54.0 MB).
- json-parse ×1.137, UniPoker ×1.047, acorn ×1.051, hash-map ×1.036 (RSS −0.8 MB).

**RSS rises**
- esprima-next +1.9 MB (+4.2%) and web-ssr +1.7 MB (+3.1%).
- V8-v7 deltablue and richards each read +0.2 MB. These are separate builds, so this is
  within code-page layout noise.

**Correctness**
- quench-runtime lib: 776/776.
- One test changed: it observed a dead key through a raw handle, and string record ids are
  reused by later string allocations.
- Every npm package's status is the same on both builds.

## Base failures (upstream, not this change; filed on TOD-133)

These fail identically on the base and the candidate:

- JetStream: babel-wtb, babel-minify-wtb, postcss-wtb, prettier-wtb, validatorjs.
- The fastify scenario aborts in `ResidualProgram::function`.
- npm: browserslist, jiti, @babel/core, @babel/helper-module-transforms, schema-utils,
  foreground-child, @babel/plugin-syntax-typescript.
