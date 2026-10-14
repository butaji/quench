# Published npm package qualification

This suite pins 100 published packages and compares representative API scenarios
on Quench with Node. A scenario pass establishes only the exercised behavior;
it does not establish that an entire package or its upstream suite is compatible.
The current work and remaining qualification obligations belong to
[TOD-127](https://linear.app/todoxapp/issue/TOD-127).

`inventory.json` records the ranking source, capture time, exact versions and
tarball integrities. The ranking uses current weekly download counts from the
npm registry, sorted across 4,665 candidates discovered through 273 popular-package
seed searches. The seed inventories are npmleaderboard and Commit; adjacent
registry search results expand their coverage. This is a reproducible snapshot,
not an exhaustive registry census. The raw observations and seed-query exceptions
are in `evidence/npm-top100-2026-10-14/ranking-source-current.json`.

The first published-ranking snapshot had inconsistent historical windows. Its
inventory, lockfile and raw response are retained alongside the initial baseline
so those observations remain attributable.

Install the lockfile with the selected Node/npm installation:

```sh
cd tests/npm-top100
npm ci --ignore-scripts --no-audit --no-fund
```

Run from the repository root, supplying absolute executable paths and a positive
per-package deadline in seconds:

```sh
cargo build --profile iteration -p quench-node --bin quench-node
cargo build --profile iteration -p quench-node-test --bin run-npm
target/iteration/run-npm /path/to/node /absolute/path/to/quench-node \
  /absolute/path/to/report.json 30
```

Append an exact package name to select a focused case. Without a filter the
runner validates the 100-entry inventory and installed versions, runs Node and
Quench in separate temporary working directories, and saves every observation.
Each process owns its process group and deadline. Crashes, timeouts, oracle
failures, unexpected stderr and missing success markers never count as passes.
The report records source revision, dirty state, executable hashes, Node version,
and inventory, lockfile and scenario hashes. Reports are checkpointed after each
package. A partial or failing qualification returns nonzero.

`type-fest`, `undici-types` and `@types/node` contain declarations rather than
runtime JavaScript. Their scenarios run the pinned TypeScript CLI through each
engine, accepting a valid public-type consumer and rejecting an invalid one.
Successful declaration checks are reported separately as `declarations-pass`.
TypeScript 7's CLI invokes its native compiler; `@esbuild/linux-x64` likewise
checks native tool launch through the engine's child-process API. These tests
currently require Linux x86-64.

Other entries invoke published APIs, including asynchronous streams, module
loading, filesystem access and child processes. Some scenarios cover only a small
part of their package's API; package-wide upstream qualification remains separate.

The original `tests/npm-popular` suite retains its separately pinned versions and
broader filesystem regressions. This suite adds a larger inventory and newer
versions; it does not replace those checks.
