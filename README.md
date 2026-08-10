# quench-node

Node-compatible JavaScript runtime built in Rust on top of
[rquickjs](https://github.com/DelSkayn/rquickjs), with readable JavaScript
polyfills and a staged compatibility harness. Its implementation strategy is
data-first: compact API declarations generate repetitive wrappers, registration,
validation, and tests; handwritten code is reserved for irreducible behavior.
See [`docs/data-first-minimal-runtime.md`](docs/data-first-minimal-runtime.md).

## Quick start

````sh
cargo run -p quench-node -- --stage 284
tools/run-node-tests.sh tests/node/test/parallel/test-querystring.js
tools/compat-coverage.sh
tools/compat-inventory.sh target/compat/inventory.json
tools/diff-node-quench.sh tests/node/test/parallel/test-url-format.js
tools/diff-node-quench-parallel.sh tests/node/test/parallel
tools/compat-queue.sh target/compat/diff-url.json
tools/compat-goal-audit.sh
tools/check-application-stages.sh
tools/check-focused-stages.sh
tools/check-focused-policy.sh
tools/check-all-tests.sh

Feature-gated `stream/iter` stages are run with:

```sh
cargo run -p quench-node -- --experimental-stream-iter --stage 169
````

Runnable application examples are in [`examples/`](examples/). They can be
executed directly with the runtime:

```sh
cargo run -p quench-node -- examples/cli-summary.js
cargo run -p quench-node -- examples/crypto-file-summary.js
cargo run -p quench-node -- examples/http-loopback.js
cargo run -p quench-node -- examples/stream-summary.js
tools/run-examples.sh
```

## Next.js benchmark history

The reproducible benchmark lives in `/tmp/nextjs/my-app/bench.cjs` and runs
route loading, props JSON serialization, and SSR-style response assembly seven
times per runtime. Startup is process wall time, RSS is peak resident memory
from `/usr/bin/time`, and request/sec is in-process synthetic workload
throughput. It does not claim that quench-node runs the Next.js compiler or
`next dev`; those remain Node-only.

Historical measurements:

| Snapshot | Scenario | Runtime | Startup | Peak RSS | Workload | Req/sec |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| initial smoke | shared route workload | Node | 35.7 ms | 46.9 MiB | 8.5 ms | — |
| initial smoke | shared route workload | quench-node | 114.0 ms | 17.8 MiB | 26.9 ms | — |
| scenario run A | route-load | Node | 43.8 ms | 49.9 MiB | 15.8 ms | 126,705 |
| scenario run A | route-load | quench-node | 127.2 ms | 18.5 MiB | 39.9 ms | 50,096 |
| scenario run A | props-json | Node | 28.0 ms | 45.8 MiB | 0.4 ms | 4,587,598 |
| scenario run A | props-json | quench-node | 103.7 ms | 17.8 MiB | 17.1 ms | 117,144 |
| scenario run A | ssr-response | Node | 28.8 ms | 45.9 MiB | 0.6 ms | 3,584,229 |
| scenario run A | ssr-response | quench-node | 96.2 ms | 18.5 MiB | 9.2 ms | 217,699 |
| scenario run B | route-load | Node | 67.8 ms | 50.0 MiB | 20.1 ms | 99,594 |
| scenario run B | route-load | quench-node | 270.3 ms | 18.6 MiB | 124.8 ms | 16,032 |
| scenario run B | props-json | Node | 45.3 ms | 45.8 MiB | 0.4 ms | 4,463,449 |
| scenario run B | props-json | quench-node | 122.5 ms | 18.6 MiB | 17.4 ms | 115,121 |
| scenario run B | ssr-response | Node | 35.7 ms | 45.9 MiB | 0.6 ms | 3,575,419 |
| scenario run B | ssr-response | quench-node | 98.4 ms | 18.5 MiB | 9.3 ms | 216,053 |

Runtime-change ledger:

| Commit | Improvement | Evidence |
| --- | --- | --- |
| `c1518030b` | ESM entry promises are polled so timers and I/O can progress. | Stage 1930 passed. |
| `1980092dc` | Experimental stream/iter flags propagate through the host boundary. | Stream/iter focused stages became runnable. |
| `0495f2a83` | Unknown builtin-loader errors are preserved instead of masked. | Loader compatibility regression coverage. |
| `d9731014f` | Nonrecursive directory removal rejects directories consistently. | Focused filesystem contract coverage. |
| `37e3d03b6` | Differential heartbeat stops when all reports are complete. | Parallel report smoke completed without a lingering heartbeat. |
| `c2d64e3ce` | Compatibility work and generated-run clutter were consolidated. | 503-file cleanup commit; no runtime benchmark claim. |

Benchmark changes are only eligible for a runtime commit when they reduce
tracked implementation LOC and improve at least one measured value by 10% or
more against a rerun baseline.

Node 24 is the compatibility target, initially on Linux x86_64. The Node test
suite is tracked as the `tests/node` submodule. Compatibility stages live under
`tests/node-compat`; each stage is committed and verified before advancing. The
primary manifest covers `test/parallel/`, `test/es-module/`, and required
`test/common/` and `test/fixtures/` support files.

## Scope

The authoritative test-source map is documented in
[`docs/authoritative-test-sources.md`](docs/authoritative-test-sources.md). It
covers the Node.js suite, LLRT, Deno's node compatibility runner, WPT, and
Test262, with Node's suite as the primary oracle.

The repository contains only the `quench-node` crate, its polyfills, the Node
test submodule, compatibility stages, and the small harness needed to run them.
Declarations and exceptional polyfills are intentionally readable. Mechanical
duplication should be generated and removed, with minimum maintainable LOC as
the primary implementation objective.

`tools/compat-coverage.sh` reports the current fixture and upstream-test
inventory. It deliberately reports Node API coverage as `unmeasured`: a count of
focused fixtures is not a valid percentage of the full Node API surface. Node's
upstream suite is the primary behavioral oracle; Hono and a representative npm
CLI are the initial release-facing application gates.
`tools/check-focused-stages.sh` runs every focused stage and reports concrete
pass/fail counts; it does not turn those counts into an API percentage. Both
focused-stage runners validate their actual failure list against
`tools/focused-compat-policy.json` through `tools/check-focused-policy.sh`.
`tools/run-node-fixture.cjs` provides the isolated CommonJS wrapper used by the
Node side of differential comparisons. `tools/diff-node-quench-parallel.sh` runs
the same single-fixture comparator in isolated workers and merges a sorted
complete-corpus report. `tools/check-all-tests.sh` runs Rust tests with
`cargo-nextest` when installed (or Cargo's standard runner), then runs the Node
API stages in parallel. Because stages are CLI-driven JavaScript processes,
their parallel runner is separate from nextest's Rust test process model. For
the empirical test-file percentage requested during development, run
`tools/measure-node-tests.sh [directory]`. It builds once and executes each
JavaScript file individually, reporting passed, failed, skipped, and the
resulting file pass rate. `tools/compat-goal-audit.sh` joins task status,
focused metrics, API inventory, and differential evidence into a ranked,
machine-readable next-action report. `tools/check-application-stages.sh` runs
the maintained real-application gates without requiring a full focused-stage
sweep.

## Faster compatibility workflow

The implementation roadmap for 2–5x faster progress is tracked in
`tasks/016-compatibility-throughput.md`. The key investment is a local
Node-vs-quench differential runner that persists normalized results, clusters
failures, and emits an owned work queue. Related failures should be grouped into
readable API slices instead of forcing one stage per mismatch.

Work can be partitioned into up to five isolated streams: URL/encoding,
streams/events, filesystem/modules, crypto/network/OS, and harness/globals. Each
stream must own distinct files or use an isolated worktree. Local reports should
show fixture pass/fail/skip/timeout counts, cluster rates, unique failure
signatures, and regressions. These metrics measure test progress, not the
percentage of the Node API surface. Release acceptance additionally requires
zero application-gate failures and no manifest regressions.

## Runtime boundary

`quench-node` uses `rquickjs` as its JavaScript engine and Rust host boundary.
There is no `quench-runtime` crate in this repository, and compatibility work
must not add or restore one. Keep engine integration in the `quench-node` crate
and API behavior in the JavaScript polyfills.

## License

MIT
