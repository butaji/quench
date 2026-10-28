# Splay retake after refreshing `origin/v2`

Date: 2026-10-11. Branch: `v2-cloud`. Refreshed trunk `origin/v2` from
`c80952374` to `daea35418`; the clean merge into this branch is
`c49419a3b4b76ce39541e9d6216a42462e159542`. The merge includes the latest
speed-lane runtime change and its M4 evidence.

## Linux build and correctness

- Rust: `1.99.0 (b940084d7 2026-09-28)`.
- `RUSTFLAGS='-D warnings' cargo +1.99.0 check --locked --offline -p quench-runtime`: passed.
- Optimized production build of `quench-node` and `quench-bench`: passed.
- `quench-runtime --lib`: 553 passed, 1 failed. The sole failure is the known
  unrelated `api::tests::regression_intl_german_date_format_ignores_string_split_override`
  date expectation (`2.1.1970` vs `1.1.1970`).
- Test262 stage 47: 47/47 passed. Stages 79–81: 255/255 passed.

Logs are in this directory; the runtime test log is compressed.

## Linux stock Splay diagnostic

The pinned V8-v7 Splay run completed 11/11 valid rounds with matching output.
The runner's `qualification_ready` field is false because this report covers
only Splay; it is a single-fixture distance update, not the all-eight gate.
The binary was built from clean source revision `c49419a3b4b76ce39541e9d6216a42462e159542`.

| Engine | Median Score | Median maximum RSS |
| --- | ---: | ---: |
| Quench | 2,614 | 114,819,072 B |
| Node v26.10.0 `--jitless` | 5,346 | 404,574,208 B |
| Bun no-JIT | 3,709 | 62,672,896 B |
| QuickJS | 2,898 | 149,565,440 B |

Quench is 2.05× behind Node's Score and 52,146,176 B (1.83×) above Bun's
RSS. The current Splay bars remain open. The repeated stock run is a Linux
screen; the runner reports it as not qualification-ready.

- Quench binary SHA-256:
  `1eabc1fe4614dd8a05928fe2062917ac49bc25cbc956f20c3e253c5f4d8b0e92`
- Measurement runner SHA-256:
  `924e62eee00ea5964d140f1cb04a6d1e125a2dfec08271cb0f28d4b933f79deb`
- Node SHA-256: `ab9c8eecf9f82d6693cdc3accced17034065c8d96213b0aa76a7e803d20ae1da`
- Bun SHA-256: `a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c`
- QuickJS SHA-256: `c798e32aac14495856a4709be67a11b0562a6e2d07fa89ad4f28efe46c7c7955`

The source-level stock measurements before the trunk merge were Score 2,736
vs Node 5,433 and RSS 114,819,072 B vs Bun 62,672,896 B. This separate-session
comparison does not establish a causal score change; the RSS median is
unchanged.

## M4 trunk evidence

The merged speed-lane M4 report at source `81c514727` records Splay Score 7,500
vs Node 14,913 and max RSS 116,670,464 B vs Bun 62,062,592 B (1.99× speed,
1.88× RSS). Its all-eight fixed-work gate passed and its runtime/WAST results
were 544/544 and 67,124/67,124. Those results are recorded in
[`task61-speed-81c-m4`](../task61-speed-81c-m4-2026-10-11/README.md); they do
not close either Splay stock bar.

## Reproduction

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' cargo +1.99.0 build --locked --offline \
  --profile production -p quench-node --bin quench-node \
  -p quench-bench --bin quench-bench
export PATH="$PWD/target/stageb-tools/bun/bun-linux-x64:$PWD/target/stageb-tools/qjs/quickjs-2026-06-04:$PWD/target/stageb-tools/node/node-v26.10.0-linux-x64/bin:$PATH"
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench target/production/quench-node \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 --out work/splay-stock-11.json
```
