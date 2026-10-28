# node:buffer from Node's lib/buffer.js (2026-10-17)

`node:buffer` and `internal/buffer` load Node v26.12.0's own `lib/buffer.js` and
`lib/internal/buffer.js` over a native `internalBinding('buffer')`
(`crates/quench-node/src/modules/buffer_binding_shared_vm.rs`, mirroring
`src/node_buffer.cc`). The hand-written Buffer modules are removed.

Host: x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB RAM, no cgroup CPU or memory limit
(`host.txt`). Oracle: `node-v26.12.0-linux-x64`.

## npm top 300 (`npm-scenarios.json.gz`, `npm-surface.json.gz`)

Measured with `target/iteration/run-npm` and the release `quench-node` built from
9b1e0898d (the port commit), against the oracle.

| run | before (f65e7b80d) | after |
| --- | --- | --- |
| scenarios | 286 behavior / 13 declarations / 0 failed / 1 oracle-failed | unchanged |
| surface | 267 match / 24 diff / 9 oracle-failed | 274 match / 17 diff / 9 oracle-failed |

Surface now matches for @vitejs/plugin-react, eslint, estree-walker, playwright,
vite, vitest and yargs. No package regressed. The Buffer differences in safe-buffer
and safer-buffer are gone. What remains there comes from Quench's Blob/File and
the missing ICU `transcode`.

## Upstream tests (`upstream-test-buffer.txt`)

`run-upstream-buffer-tests.sh` runs `test/parallel/test-buffer*.js` from a CommonJS
copy of Node's test tree. Exit status per file: oracle `node`, `before` (f65e7b80d)
and `after`.

Results: Node passes 74/74; Quench passed 65 before and passes 69 after, with no
regressions. The five that still fail:
- three use V8 natives syntax (`%PrepareFunctionForOptimization`);
- `test-buffer-backing-arraybuffer` needs V8's on-heap view probe;
- `test-buffer-alloc` expects V8's "Invalid typed array length: N" message (a
  runtime message).

## Instruction guard (`callgrind-guard.txt`)

`npm-icu-2026-10-16/npm-icu-fixed-callgrind-guard.sh`, iteration profile, base
4c360013e (the verified v2-npm/v2-merged merge), candidate 7058b3150.

Startup falls from about 1.2–3.9 G to 0.08–2.7 G instructions. The cause: a script's
first `console.log` used to load all of `util.js` through the old `util/types`
mapping, and `util.js` requires `buffer`. `util/types` now resolves to
`lib/util/types.js` (the `internal/util/types` object `util.types` names). Empty
script: 56.3 M → 55.5 M. `console.log("x")`: 1153 M → 56 M. First `Buffer.from`:
798 M → 1024 M, because Node's lib/buffer.js loads `internal/util/inspect` eagerly.
Per-run deltas run from -10.2 % (earley-boyer) to +0.58 % (richards). These look
like heap and GC effects of the smaller startup heap; no per-run path touches Buffer.
