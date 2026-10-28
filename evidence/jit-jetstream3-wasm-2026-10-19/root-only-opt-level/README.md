# QuickSort root-only codegen-level experiment

These three clean-source, 11-round Linux JetStream 3 reports compare the
selected QuickSort function (residual function 1) compiled with root-only
`opt_level=none` and `opt_level=speed`, plus the normal full-JIT path. All runs
passed fixture validation and output equality. Source revision was
`166e0c2c1b73a239d7d98757e7531c31536ca30e`; the corpus was pinned to
`06785cf861ac44855f168cbbe829278c2802e6de`.

| Mode | Quench median score | Quench median max RSS |
| --- | ---: | ---: |
| Root-only `none` | 247.90 | 41.36 MiB |
| Root-only `speed` | 292.02 | 41.50 MiB |
| Full eager JIT | 237.33 | 42.83 MiB |

Within the root-only experiment, `speed` raised the score by 17.8% relative to
`none`, with 0.34% higher RSS. An independent-window percentile bootstrap
(100,000 resamples of 11 rounds per mode) gave a score-ratio interval of
1.002–1.246 and an RSS-ratio interval of 1.003–1.006 for `speed / none`.
Against full JIT, root-only `speed` was 23.1% faster by median score and used
3.1% less RSS. These are experimental windows, not a qualification claim.

Root-only `speed` still missed Bun's median score (393.36; Quench/Bun = 0.742)
and Node's (468.21; Quench/Node = 0.624). Its median RSS was lower than Bun's
47.72 MiB and Node's 88.38 MiB. Root-only reports are marked
`qualification_ready=false` by design.

Reproduction command for the `speed` arm:

```sh
QUENCH_WASM_JIT_ROOT_OPT_LEVEL=speed target/production/quench-bench \
  --fixtures quicksort-wasm --suite jetstream3 --jit \
  --jit-wasm-root 1 --jit-wasm-root-only --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out target/production/quicksort-root1-only-speed-clean-11.json
```

Raw reports:

- [`root-only-none-11-round.json`](root-only-none-11-round.json)
- [`root-only-speed-11-round.json`](root-only-speed-11-round.json)
- [`full-jit-control-11-round.json`](full-jit-control-11-round.json)
