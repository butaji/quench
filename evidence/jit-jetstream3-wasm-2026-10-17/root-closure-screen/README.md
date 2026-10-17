# JetStream Wasm root-closure screen

These five reports are one-round diagnostics of a temporary Wasm-JIT change
that limited Cranelift compilation to the direct-call closure of the first
called function and later requested roots. JetStream validation passed and
outputs matched for every fixture. The reports record the exact Quench binary
hash, Node/Bun versions, host details, and dirty source state.

The host was Linux x86_64. Comparators were Bun 1.4.2 with JIT enabled and Node
26.10.0 with optimizing compilation proven by preflight. Each fixture had one
valid sample, so these data are only a screening signal and support no
statistical win claim. Root-closure compilation was removed after the screens:
it did not lower RSS and the score screens were below both peers in every case.

The reproduction form was:

```sh
target/production/quench-bench <fixture> --suite jetstream3 --jit --runs 1 \
  --timeout-ms 600000 --quench target/production/quench-node --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun --out <report.json>
```

The fixture names are `HashSet-wasm`, `gcc-loops-wasm`, `quicksort-wasm`,
`tsf-wasm`, and `zlib-wasm`.
