# Disjoint general-profile Splay screen — Linux

Source revision: `f76795545b4f2f8cc7cf423da49e758215525b9f` (`v2-cloud`),
clean. The pinned V8-v7 corpus was clean at
`64e1860c736c1b899708f4cd646721bc71e53d8b`.

The profile was collected from `general-workload.js`, a synthetic non-V8-v7
JavaScript workload included here. It exercises object construction, field and
array access, mutation, recursion, and calls. It does not run any scored
V8-v7 fixture. Profile data and binaries are not committed; their hashes are
recorded below.

## Build

Instrumented build:

```sh
CARGO_TARGET_DIR=/workspace/work/quench-target-pgo-general-instrumented \
RUSTFLAGS='-C profile-generate=/workspace/work/pgo-general-data -C target-cpu=native -C llvm-args=-align-all-functions=6' \
cargo build --profile production -p quench-node --bin quench-node --locked
```

The instrumented Quench binary ran `general-workload.js` three times. The raw
profiles were merged with Rust 1.99's LLVM 23.1.1 `llvm-profdata`; merged
profile SHA-256:
`666760f0b34c20d618d9b1f9f2e5122b566c64f01ab8edce613cdd7c68a0e51f`.

Profile-use build:

```sh
CARGO_TARGET_DIR=/workspace/work/quench-target-pgo-general-native \
RUSTFLAGS='-C profile-use=/workspace/work/quench-general.profdata -C target-cpu=native -C llvm-args=-align-all-functions=6' \
cargo build --profile production -p quench-node --bin quench-node --locked
```

Production binary SHA-256:
`6b5e200c9dd8de5fcd159b99dc039d81fcdefc25e5ed6fc7a0796f3b90c7085e`.

## Stock Splay screen

Eleven stock-harness rounds, equal output across all four engines. Medians:

| Engine | Score | Max RSS |
| -- | --: | --: |
| Quench | 4,611 | 111,472,640 B |
| QuickJS | 3,517 | 149,569,536 B |
| Bun, JIT disabled | 5,403 | 63,930,368 B |
| Node `--jitless` | 7,466 | 504,348,672 B |

Quench remains behind Bun and Node on Score and above Bun on RSS. Reject this
profile for Splay. The one-fixture report has `qualification_ready=false`, as
expected; a win also requires a second Linux session at least 30 minutes later
and the M4 stock result.

Raw report SHA-256:
`6f2ff7769474ce00eab3e90526c8535242305c658b4d83676b03c40dd83f0ff3`.
