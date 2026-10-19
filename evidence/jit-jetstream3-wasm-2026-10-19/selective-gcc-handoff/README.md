# Selective `CallKnown` handoff: paired GCC-Wasm confirmation

The three-round screen and randomized eleven-round confirmation use the same
Quench JIT executable and vary only `QUENCH_WASM_JIT_SELECTIVE` between 0 and
1. Bun and Node run as peers in every round. All rounds completed, all four
engines produced valid output, and the fixture outputs matched. The reports
record clean source revision `a4edc8e85176d7d706d3928b29127d25fd5a4d79`;
qualification is deliberately disabled for this experimental A/B.

| Run | Selective/default Score ratio (95% paired interval) | Selective/default peak RSS ratio (95% paired interval) |
| --- | ---: | ---: |
| 3-round screen | 1.272 [1.242, 1.280] | 0.9958 [0.9916, 0.9967] |
| 11-round confirmation | 1.236 [1.195, 1.275] | 0.9942 [0.9939, 0.9966] |

The eleven-round medians were 67.45 Score / 47.50 MiB for selective Quench,
54.63 / 47.77 MiB for default Quench, 182.77 / 55.18 MiB for Bun, and 193.66 /
92.08 MiB for Node. Selective handoff therefore gives a repeatable local
score gain while slightly reducing RSS. It still reaches only 0.358 of Bun's
Score and 0.347 of Node's, so it does not meet the requested JIT peer-score
win. Its RSS remains lower than both peers.

The predeclared advancement gate was at least a 5% Score gain with a paired
95% interval above 1.0 and no more than 0.5 MiB RSS growth. The confirmation
passes that gate. It supports keeping the existing handoff available as an
opt-in experiment; it does not justify enabling it by default or claiming a
peer win.

The one-round instrumented mechanism check is counter evidence only; do not
interpret its Score or wall time as benchmark results. The default run
reported 8 Wasm compilation events, 569,416 summed emitted code bytes, and
603.1 ms summed compile time. Selective mode reported 75 events, 352,517
emitted bytes, and 286.5 ms compile time. Both modes still recorded about
309,000 import callouts (309,150 default; 309,039 selective), so this handoff
does not remove the remaining import boundary cost. The full counter logs are
in `diagnostic-default-1-round.json` and
`diagnostic-selective-1-round.json`; their instrumented timings are not
evidence of speedup.

Raw reports and paired analyses:

- `paired-screen-3-round.json` and `paired-screen-3-round-analysis.json`
- `paired-confirmation-11-round.json` and
  `paired-confirmation-11-round-analysis.json`

The measured binaries were Quench JIT SHA-256
`90914fdc6f19e423d278f0db56d279c6b7bbf016a1c32adcd0a984021bb60119`, Bun
1.4.3, and Node v26.12.0. To reproduce the confirmation from a clean checkout,
build the production binaries and write reports outside the checkout so the
source provenance stays clean:

```sh
cargo build --locked -j1 --profile production -p quench-node -p quench-bench
target/production/quench-bench gcc-loops-wasm \
  --suite jetstream3 --jit --jit-wasm-selective-ab --runs 11 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out /tmp/selective-gcc-confirmation.json
target/production/quench-bench --analyze /tmp/selective-gcc-confirmation.json \
  --out /tmp/selective-gcc-confirmation-analysis.json
```
