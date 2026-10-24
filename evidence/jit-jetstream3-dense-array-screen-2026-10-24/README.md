# Dense-array GetIndex JIT screen · 2026-10-24

Source: `1cf2f753f37c3f6c3422d6e76ce6f84a890ab6dc`; JetStream 3 corpus:
`06785cf861ac44855f168cbbe829278c2802e6de`. Linux x86_64, four CPU quota,
16 GiB memory limit, rustc 1.99.0. Production `quench-node` and
`quench-bench` were built with `--locked --profile production`. The runner
preflight confirmed Quench `--jit`, Bun 1.4.3 `useJIT=true`, and Node v26.12.0
Maglev/TurboFan. The `--qjs /bin/true` argument satisfies the unused QuickJS
path option; JetStream 3 does not include QuickJS as a comparator.

The uninstrumented stock JetStream run used three rounds for `web-ssr`,
`hash-map`, and `stanford-crypto-sha256`. All fixtures passed validation and
had output-equal results. This is a screen, not a qualification run: the
required 11+ rounds are absent and the stock analyzer rejected the report as
not qualification-ready.

| Fixture | Quench JIT median Score | Bun JIT | Node JIT | Quench/Bun | Quench/Node | Quench median max RSS | Bun RSS | Node RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `web-ssr` | 12.60 | 65.25 | 78.31 | 0.193× | 0.161× | 113.6 MiB | 226.3 MiB | 214.6 MiB |
| `hash-map` | 8.61 | 153.73 | 188.93 | 0.056× | 0.046× | 36.7 MiB | 89.1 MiB | 162.8 MiB |
| `stanford-crypto-sha256` | 33.06 | 421.91 | 240.54 | 0.078× | 0.137× | 35.4 MiB | 57.5 MiB | 60.5 MiB |

Quench uses less maximum RSS in these screens, but its score is far below both
peers on every selected fixture. The prior 11-round Web-SSR screen is not a
matched control for this source/host run, so these numbers do not establish a
causal effect for the new array read lowering. The ≥1.20× peer score target is
not met.

The separate one-round `web-ssr` diagnostics run is overhead-instrumented and
is not score evidence. It recorded 1,169 per-function rows, 62 compile attempts,
154,441 native function entries, and 532,629 interpreter fallback entries.
The counters overlap across nested functions and are not exclusive time.
General-dispatch callout opcode counts were led by `Call` (183,078),
`GetIterator` (100,331), `LoadNameCall` (59,434), `GetField` (12,559), and
`SetIndex` (6,574). The current diagnostics do not record native GetIndex hit
and guard-failure counts, so those counts cannot establish that this new
denotation materially covered benchmark execution.

Raw artifacts:

- `screen.json` — 3-round uninstrumented peer report.
- `web-ssr-diagnostics-1round.json` — one-round instrumented function census.

Commands:

```sh
target/production/quench-bench --preflight-only --suite jetstream3 --jit \
  --quench target/production/quench-node \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun target/tools/bun/bun-linux-x64/bun --qjs /bin/true

target/production/quench-bench \
  --fixtures web-ssr,hash-map,stanford-crypto-sha256 \
  --suite jetstream3 --jit --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun target/tools/bun/bun-linux-x64/bun --qjs /bin/true \
  --out evidence/jit-jetstream3-dense-array-screen-2026-10-24/screen.json

target/production/quench-bench --fixtures web-ssr --suite jetstream3 --jit \
  --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun target/tools/bun/bun-linux-x64/bun --qjs /bin/true \
  --out evidence/jit-jetstream3-dense-array-screen-2026-10-24/web-ssr-diagnostics-1round.json
```
