# JavaScript runtime stress corpus

This directory contains 100 checked-in, standalone JavaScript stress programs
(`001.js` through `100.js`). There is no generator: the numbered files are the
corpus, and `manifest.json` is their static measurement index.

The suite is a 10 × 10 matrix. Each family has ten different stress kernels,
not padded copies of one loop:

| Cases | Surface | Examples of pressure |
| --- | --- | --- |
| 001–010 | numeric | integer overflow, floating point, bitwise, BigInt, conversions |
| 011–020 | control | branch entropy, nesting, switch, exceptions, state machines |
| 021–030 | arrays | packed/sparse access, sorting, splicing, flattening, matrices |
| 031–040 | objects | shapes, descriptors, prototypes, accessors, symbols, graphs |
| 041–050 | functions | closures, recursion, call/apply/bind, constructors, classes |
| 051–060 | strings | ropes, Unicode, regex, replacement, normalization, search |
| 061–070 | iterables | iterators, generators, close/throw, composition, spread, destructuring |
| 071–080 | collections | Map/Set churn, WeakMap identity/lifetime, bounded caches |
| 081–090 | typed-memory | typed lanes, DataView, float/BigInt arrays, aliasing, copies |
| 091–100 | meta | Proxy, Reflect, JSON, Date, errors, dynamic functions |

Every file defines `globalThis.microRun`. Setup is local to a workload call,
the target operation is the repeated work, and the returned checksum keeps the
work observable. The header and manifest declare the operation, work units,
and memory profile so timing can be normalized without inspecting benchmark
source at runtime.

## Running

Check that the static corpus and manifest agree, then run differential timing
and RSS measurements against Node and a candidate engine:

```sh
node micros/verify.mjs --runs 3 --out micros-results.json
node micros/verify.mjs --engine path/to/quench-node --runs 3 --out quench-micros-results.json
```

For warmed throughput, the harness performs one warmup window followed by five
timed windows and reports elapsed time per declared work unit:

```sh
QUENCH_MICRO_PATH=$PWD/micros/001.js node micros/harness.cjs
node tools/run-micro-score.cjs 001 041 081 100
```

`verify.mjs` uses `/usr/bin/time` when available for process wall time and peak
RSS/footprint. RSS is host-level evidence, not a GC explanation. Use an
instrumented runtime separately for allocation bytes, live heap, collection
count, pauses, promotion, and external memory. Quench's optional
`QUENCH_EXEC_TRACE=1` report is intended for that attribution pass, not for a
timed run.

Do not collapse CPU and memory into one claim. Report per-family and
per-profile scores, variance, and raw samples; compare runs only on the same
machine, runtime flags, and corpus revision. The suite is stress evidence and
compatibility evidence, not proof of general application performance.
