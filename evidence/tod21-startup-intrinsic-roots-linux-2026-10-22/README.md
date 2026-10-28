# Startup intrinsic roots derived from the sealed holders — kept

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. Production profile `quench-node` with
`profile-memory`, compile cache on, print1. Malloc in use was read with `mallinfo2` at the
host bootstrap checkpoints (branch `meas-gc-process-split`).

## Change

`StartupIntrinsics` kept every sealed holder's values twice: in the holder's `values`, and again
in `sealed_roots`, a flat copy for the collector. The roots now come from the holders
themselves. Only the facts that need a walk of the shapes are still stored up front, in
`shape_roots`: the sealed shapes' symbol keys and accessor functions. One representation per
fact; collection reads the same number of values.

## Result

| Checkpoint | Base | Candidate |
| -- | --: | --: |
| `node_console_global` (two seals) | +40.1 KiB | +32.1 KiB |
| `node_commonjs` (end of bootstrap), malloc in use | 823,872 B | 818,304 B |

A small result (−5 to −8 KiB) that removes a duplicate.

## Verification

- `cargo test -p quench-runtime --lib`: 823 passed. This includes the `api_native` startup
  intrinsics tests.
- `sealgc.js`: replaces `Array.prototype.map`, defines a throwing `Object.prototype` getter,
  forces 50 collections (`--expose-gc`), then loads `util`, `path` and `events`. Output matches
  Node 22: `{ a: [ 1, 2 ] } a/b function false`.
