# Selective JIT handoff for hot `CallKnown` functions

Commit `7961b7bde` lets a hot eligible Wasm `CallKnown` activation leave the
frameless interpreter lane and reach the existing one-shot Cranelift tier
gate. Cold calls remain on the lane; terminal calls keep their existing tail
path. The regression test verifies the fixture really contains `CallKnown`,
keeps the target from frontend inlining, and checks the interpreted result and
native-entry threshold.

The clean-source reports use revision
`7961b7bdea895c1611d356651fe075e5e9ae2ddb`, Linux x86_64, 11 rounds, Quench
production binaries, Bun 1.4.3, and Node v26.12.0. Both fixtures completed,
passed validation, and produced equal output. `default-report.json` and
`default-analysis.json` are the ordinary `--jit` run. `selective-report.json`
uses `--jit --jit-wasm-selective`; the harness deliberately marks this mode
`qualification_ready: false`, so the standard analyzer rejects it. Treat its
11-round medians as a comparative screen, not a qualification or a paired
confidence claim. `diagnostic-report.json` is a separate instrumented,
one-round run and is not timing evidence.

| Fixture | Mode | Quench Score / RSS MiB | Bun Score / RSS MiB | Node Score / RSS MiB |
| --- | --- | ---: | ---: | ---: |
| gcc-loops Wasm | default JIT | 57.11 / 47.98 | 175.15 / 51.99 | 183.07 / 77.39 |
| gcc-loops Wasm | selective JIT | 71.28 / 48.14 | 171.27 / 51.75 | 189.94 / 77.33 |
| QuickSort Wasm | default JIT | 183.47 / 43.00 | 374.83 / 44.18 | 434.60 / 74.07 |
| QuickSort Wasm | selective JIT | 180.05 / 42.81 | 366.52 / 44.12 | 420.85 / 73.99 |

Selective mode raises gcc-loops' median Score by 24.8% on this same-revision
screen and changes its first timed iteration from about 306 ms to 118 ms; the
reported average iteration time changes from about 33 ms to 35 ms. QuickSort
is essentially flat in Score and RSS. The default report's paired analysis
puts Quench/Bun Score ratios at 0.320 [0.311, 0.351] for gcc-loops and 0.474
[0.459, 0.514] for QuickSort; its RSS ratios are below 1 for both fixtures.
The selective medians still lose Score to Bun and Node while keeping lower
RSS, so neither fixture meets the requested JIT score-and-RSS win.

The diagnostic screen records 75 gcc-loops tier compile events, 345.5 ms
summed compilation time, 388,485 emitted code bytes, and 309,089 compiled-to-VM
callouts, 309,039 of them imported calls. QuickSort records 6 events, 39.7 ms,
51,293 emitted bytes, and 9,955 callouts. This attributes a large VM-boundary
cost to gcc-loops, while QuickSort's hot sort path has few callouts and calls
for better native code generation or lower entry/runtime overhead. The
aggregate does not identify individual imported symbols, so further per-site
attribution is needed before specializing an import.

The raw reports preserve rounds, host and executable provenance, fixture
hashes, output, and diagnostic stderr. To reproduce the selective measurement
after building the production binaries:

```sh
target/production/quench-bench \
  --fixtures quicksort-wasm,gcc-loops-wasm --suite jetstream3 \
  --jit --jit-wasm-selective --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out target/jetstream3-qsort-gcc-selective-handoff-final.json
```
