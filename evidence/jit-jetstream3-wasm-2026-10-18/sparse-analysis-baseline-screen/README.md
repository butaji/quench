# Sparse analysis and baseline compiler screens

These reports investigate the HashSet-wasm residual function 3 (`main`) and
function 31 (`operator new`) after sparse per-PC register analysis was added.
They are one-round, root-selected screens. Every report has
`qualification_ready: false`; none establishes a JetStream win.
All reports were captured from clean source revision
`b1f4ec69f9278bc619b9ed79a51b4eb22fc9cda9` on the same Linux x86_64 host;
root selection itself remains an explicitly experimental benchmark mode.

The diagnostic root-3 run recorded 66,145 reachable bytecodes and 97 registers.
It estimated 76,992,780 bytes for a dense register-state matrix and
6,962,568 bytes for the sparse states, including the state table and allocated
slot capacities (about 11.1x smaller). Planning took 5.7-10.2 ms. The Cranelift
body still dominated memory: with `opt_level=none`, function 3 took 875 ms to
compile and process high-water RSS reached 266,068 KiB. The full diagnostic
compile took 944 ms and emitted 1,544,754 native code bytes.

JetStream 3 scores the first, worst, and average iteration components with a
geometric mean. In the diagnostic run, Quench's first HashSet iteration took
1,002 ms (Score 4.99) while the later average was 22.60 ms (Score 221.24).
The compilation delay falls inside that first timed iteration: HashSet's
`init()` loads only the binary, and its first `runIteration()` initializes the
Emscripten module and reaches Quench's lazy whole-program compile. See
`quench-bench/jetstream/JetStreamDriver.js` and
`quench-bench/jetstream/wasm/HashSet/benchmark.js` for the timer boundary.

## One-round screens

| Selection and Cranelift level | Quench Score / RSS MiB | Bun Score / RSS MiB | Node Score / RSS MiB |
| --- | ---: | ---: | ---: |
| root 3, speed | 58.70 / 285.0 | 205.34 / 51.6 | 267.88 / 111.0 |
| root 3, none | 57.00 / 259.1 | 203.81 / 51.5 | 258.26 / 110.9 |
| root 3, speed_and_size | 58.32 / 284.7 | 206.16 / 51.2 | 224.01 / 110.8 |
| root 31, none | 24.15 / 43.7 | 215.82 / 52.1 | 239.56 / 110.8 |
| root 27, speed | 23.48 / 44.6 | 208.04 / 51.1 | 253.61 / 110.9 |

The root-31 and root-27 screens keep RSS below both peers, but their Scores
remain far behind. Root 3 improves warm throughput but loses on aggregate
Score and RSS. The Cranelift `none` level cuts compile time and RSS for root 3
but also lowers its screen Score; `speed_and_size` does not close the gap.
These measurements prioritize a fast baseline compiler, with selective Cranelift
promotion later, over more aggressive Cranelift root admission. The current
root selector remains experimental and opt-in.

## Reproduction

Built with Rust 1.99.1:

```sh
cargo +1.99.1 build --profile production -p quench-node -p quench-bench
```

For `none`, the benchmark harness clears inherited variables, so the Quench
executable was wrapped as follows (the speed-and-size screen used the same
wrapper with `speed_and_size`):

```sh
#!/bin/sh
export QUENCH_WASM_JIT_ROOT_OPT_LEVEL=none
exec /workspace/quench/target/production/quench-node "$@"
```

Then run a one-round diagnostic and a separate uninstrumented screen:

```sh
target/production/quench-bench --fixtures HashSet-wasm --suite jetstream3 \
  --jit --jit-diagnostics --jit-wasm-root 3 --runs 1 --timeout-ms 600000 \
  --quench /tmp/quench-node-root-opt-none \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out target/hashset-wasm-root3-none-diagnostic.json

target/production/quench-bench --fixtures HashSet-wasm --suite jetstream3 \
  --jit --jit-wasm-root 3 --runs 1 --timeout-ms 600000 \
  --quench /tmp/quench-node-root-opt-none \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out target/hashset-wasm-root3-none-screen.json
```

The root-27 and root-31 screens used the same command with their function IDs;
root 27 used the regular `quench-node` binary (default `speed`). Raw JSON reports
are kept alongside this file. For competitive claims, use clean builds and
at least 11 matched rounds with paired score and RSS intervals, as specified in
`docs/v2/jit-design.md`.
