# No-JIT web lane: integration and Node parity baseline

Runtime source: `1238a8c8a`; no production runtime changes. Host and cgroup limits,
exact binaries, versions, hashes and JIT proof are in `verification/final.json`
and `provenance.json`. Quench is an **iteration build**, for correctness and
harness diagnostics only. No production speed or memory win is claimed.

The user requires full latest-stable Node behavior parity **before optimization**.
That requirement is not met. No optimization was made or authorized by a passing
subset. Node's official release index identifies v26.12.0 (2026-10-14);
`oracle-release.json` retains that record. Binary and source archives were checked
against the release's official SHASUMS256.txt.

## Results

- `verification/final.json`: verification-only, no load. All six Node/Bun
  framework combinations pass. Quench Express/Fastify pass ping, query, dynamic
  query and JSON, but alter the full binary video. Quench Hono returns HTTP 400.
  Failed workload rows have no score. The whole matrix remains incomplete.
- `framework-scenarios/results.json`: all three existing Express/Koa/Fastify
  scenarios pass both Node v26.12.0 (`--jitless`) and Quench. These scenarios do
  not exercise the newly discovered failures.
- `latest-node-focused/results.json`: ten unchanged v26.12.0 upstream HTTP
  fixtures pass Node CLI and fail Quench CLI at entry classification (`require
  is not defined`), before the HTTP assertions. The source archive was placed
  under a `node_modules` boundary to prevent this repository's `type:module`
  scope from changing ordinary Node source behavior. Node treats these entries
  as CommonJS; Quench does not. No test source was changed. This is a CLI parity
  counterexample, not an HTTP API qualification result.
- `parity-coverage.json`: 5,015 recursive JS/CJS/MJS inventory entries with source
  hashes, ten CLI differential checks and explicit untested entries. This is
  not the whole Node behavioral contract. Skips and untested cases remain gaps;
  full Node and npm callable parity are still open.
- `preliminary-load-node24.json`: earlier diagnostic smoke with Node v24.19.0,
  Bun 1.4.3 and Quench, 1 second/workload, 1 round. Valid ping/query rows exist
  for Quench Express/Fastify; concurrent JSON hits socket timeouts at 500 connections (not HTTP 4xx/5xx), and video correctness
  fails. The old Node oracle and iteration build make this unsuitable as a
  latest-stable production comparison. It is retained to reproduce the load
  failure, not to claim throughput gains.

## Checks

Runner unit test, focused clippy, changed-file rustfmt, workspace/all-target check,
and `git diff --check` pass. Package-wide rustfmt also reports pre-existing
formatting differences in `quench-bench/src/main.rs`; that file is unchanged.

`instruction-guard.json` and the raw/summary files retain the all-eight Callgrind
check. Baseline and candidate use the same unchanged runtime executable. Every
fixture is within 1%; no runtime performance change is being asserted.

The full runtime library regression suite and pinned framework-core check are
recorded separately when complete.

## Reproduce

Follow `docs/web-benchmarks.md` to clone the exact upstream revision and install
its frozen dependency lock. Use a new output directory on every attempt:

```sh
cargo run -p quench-bench --bin web -- \
  --suite work/web-upstream --out evidence/web-verification-NEW \
  --quench target/iteration/quench-node \
  --node /path/to/node-v26.12.0/bin/node --bun /path/to/bun \
  --bombardier /path/to/bombardier --rounds 1 --verify-only
```

Node uses `--jitless`; Bun requires the `useJIT=false` option dump; Quench uses
its default interpreted path. Raw server logs, response headers, validation
bodies and expected/actual video hashes remain in the verification directory.
The large video body is removed after hashing.

Coordination: [TOD-133](https://linear.app/todoxapp/issue/TOD-133) owns this
integration; [TOD-134](https://linear.app/todoxapp/issue/TOD-134) records the
blocking host findings for the Node lane. The existing
[TOD-20](https://linear.app/todoxapp/issue/TOD-20) and
[TOD-127](https://linear.app/todoxapp/issue/TOD-127) own broader Node and npm
qualification. Optimization stays blocked until the user's broader parity
requirement is satisfied.
