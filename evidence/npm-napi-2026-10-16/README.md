# Node-API addons and large module graphs, 2026-10-16

Published source `3e7f186de` (v2-npm merged with v2-merged `efaa9749f`) adds Node-API
addon loading (`crates/quench-node/src/modules/napi/`) and removes three quadratic paths in
module linking and lane-view derivation. Oracle: official Node v26.12.0 (`engines.json`).

## 300-package scenarios (60 s deadline per package)

| run | source | behavior | declarations | Quench failures | oracle failures |
| --- | --- | --- | --- | --- | --- |
| `baseline-scenarios-1305d2582.json.gz` | 1305d2582 (clean) | 273 | 13 | 13 | 1 |
| `scenarios-3e7f186de.json.gz` | 3e7f186de (clean) | 282 | 13 | 4 | 1 |
| `scenarios-ec3b526e8.json.gz` | ec3b526e8 (clean, merged with v2-merged 5c7715c92) | 282 | 13 | 4 | 1 |

Newly passing: @rollup/rollup-linux-x64-gnu, rollup, @img/sharp-linux-x64, sharp,
@img/sharp-libvips-linux-x64, @tailwindcss/oxide-linux-x64-gnu, vite and
@vitejs/plugin-react (Node-API), and lucide-react (its import fell from 53-74 s to 3 s).
No scenario lost a pass. Remaining Quench failures: esbuild (needs `worker_threads` with
shared memory), vitest, @aws-sdk/nested-clients and minizlib. The musl Rollup addon fails
on the glibc Node oracle as well. These are representative package scenarios, not
all-functions qualification.

## Node-API contract

`crates/quench-node-test/tests/napi_addons.rs` builds `node-tests/napi/napi_contract.c`
against the pinned `tests/node/src` headers and compares Node and Quench output line by
line: values and strings, properties and key collection, classes, wraps and signatures,
errors and pending exceptions, references, handle scopes, buffers and typed arrays,
externals and type tags, promises, async work, thread-safe functions and `dlopen`
failures. `napi-addon-tests.log` records both tests passing on 3e7f186de.

Known differences: external buffers and ArrayBuffers are copied into guest storage (their
finalizers run when the guest object is collected); environment cleanup hooks and
instance-data finalizers do not run at exit.

## Instruction guard

`instruction-guard.txt` runs the existing all-eight Callgrind guard
(`evidence/npm-icu-2026-10-16/npm-icu-fixed-callgrind-guard.sh`) on iteration-profile
executables of efaa9749f (base) and 3e7f186de (candidate), both Cargo-built with identical
settings on this host. Per-run deltas are at most +0.21% (Splay), below the 1% hold
threshold, and all 32 runs completed. Startup instructions fall by about 159-162 million on
every fixture, from linking Node's bootstrap modules without the quadratic export paths.
This is a regression screen, not a performance claim.
