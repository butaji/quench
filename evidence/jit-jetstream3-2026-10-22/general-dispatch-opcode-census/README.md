# General-dispatch opcode census

This diagnostic tests whether dynamic opcode counts can guide the next JIT
denotation. The instrumentation was committed at `16cc2ed06`; it counts only
instructions entering the general dispatcher for tracked JavaScript functions.
Threaded fast-lane and native instructions are excluded. Counts are diagnostic
coverage evidence, not time attribution or performance results.

## Web-SSR production diagnostic

Source: `16cc2ed06`; Linux x86_64; Rust 1.99.1; production profile;
JetStream 3 checkout `06785cf861ac44855f168cbbe829278c2802e6de`; one round.
The stock fixture validated. Quench scored 10.27 with 133,382,144 bytes peak
RSS in this instrumented run. These are not clean score/RSS measurements.

Across 243 diagnostic function rows, 169,038 general-dispatch instructions
were counted. Aggregate counts were:

| Opcode | General-dispatch count |
| -- | --: |
| `LoadName` | 158,146 |
| `LoadNameCall` | 6,420 |
| `Construct` | 3,225 |
| `LoadConst` | 190 |
| `GetField` | 179 |
| `Return` | 136 |

Most counts came from generated lane-only `Eval` functions, each with roughly
5,592 counted general-dispatch operations across 107 lane entries. Their
interpreter-inclusive time was not recorded. The highest named inclusive
interpreter rows were small (for example, `logMetric` at about 7.4 ms), so the
high `LoadName` frequency alone does not show that name resolution dominates
fixture time. Correlate with exclusive opcode timing before selecting a name
lookup optimization. Preserve the current exact name-resolution semantics for
`with`, TDZ, getters, receiver binding and errors.

The iteration-profile `typescript-lib` diagnostic hit the 600 s per-engine
timeout and emitted no Quench counters because the process was killed before
its diagnostics could flush. It is retained as a timeout report only.

## Reproduction

```sh
target/production/quench-bench --fixtures web-ssr --suite jetstream3 --jit \
  --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true \
  --out target/web-ssr-dynamic-opcode-diagnostic.json
```

The raw report is `web-ssr-production-1round.json`; the timed-out report is
`typescript-lib-iteration-timeout.json`.
