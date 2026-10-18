# Backedge-only safepoints

Commit `e955cff99` keeps the compiled function-entry interrupt check and moves
conditional-loop checks onto the taken backedge. Forward branches and loop
exits no longer load the interrupt flag. The regression test starts a native
infinite Wasm loop, requests termination through the shared execution handle,
and verifies the native entry returns the execution-terminated error.

Validation before measurement: `cargo +1.99.1 test --locked -p
quench-runtime --lib` passed all 713 tests; the production `quench-node` and
`quench-bench` build passed. The clean-source JetStream 3.0 run completed 11
matched rounds for both fixtures, passed validation, and produced equal
outputs. The raw report records source revision `e955cff990aa6e10b73c52b32801044b88467923`,
Node v26.12.0, Bun 1.4.3, and Quench `--jit`.

| Fixture | Engine | Median Score | Median peak RSS |
| --- | --- | ---: | ---: |
| gcc-loops-wasm | Quench | 59.80 | 47.79 MiB |
|  | Bun | 176.95 | 51.80 MiB |
|  | Node | 185.91 | 77.32 MiB |
| quicksort-wasm | Quench | 173.19 | 42.43 MiB |
|  | Bun | 333.01 | 43.99 MiB |
|  | Node | 445.64 | 74.05 MiB |

Paired bootstrap intervals confirm an RSS win against Bun and Node for both
fixtures. Score ratios to Bun are 0.341 [0.317, 0.372] for gcc-loops and
0.555 [0.490, 0.589] for QuickSort; the analyzer correctly marks both
fixtures unqualified because their Score intervals remain below parity. The
requested JIT Score-and-RSS win is still open. This result does not establish a
causal before/after speedup for the safepoint change; the earlier run used a
different host sampling window.

Reproduce after building production binaries:

```sh
target/production/quench-bench \
  --fixtures quicksort-wasm,gcc-loops-wasm --suite jetstream3 --jit \
  --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --checkpoint target/jetstream3-backedge-poll.checkpoint.json \
  --out target/jetstream3-backedge-poll.json
target/production/quench-bench \
  --analyze target/jetstream3-backedge-poll.json \
  --out target/jetstream3-backedge-poll-analysis.json
```
