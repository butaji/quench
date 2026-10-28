# Instrumented JetStream 3 `hash-map` JIT profile

One opt-in diagnostic round on clean source revision
`54f0e3ad951df576a1c18514273726e8ebb35f89`. The stock fixture validated and
outputs matched Bun and Node. Because `--jit-diagnostics` adds instrumentation,
the displayed scores/RSS are not clean performance comparisons.

The tracked JS JIT diagnostics reported 65 functions, 54,003,151 total entries,
54,002,098 native function entries, 21,600,150 lane entries, and 2 compiled
functions. The `equals` helper was entered 54,000,000 times; 53,999,969 entries
ran native, with 31 interpreter fallbacks. It is a 25-instruction function
with a numeric ABI and 4,096 code bytes. `calculateCapacity` had 2,160 entries,
2,129 native entries and was also compiled.

`runIteration` had only 32 entries but 36.37 seconds of *inclusive*
interpreter time and 120 general-dispatch `LoadConst`, `LoadNameCall`, `Call`,
and `Return` operations. This time includes nested callees and must not be
attributed to `runIteration` itself. The profile demonstrates a coverage
boundary: the hot caller remains interpreted while a frequent equality callee
runs native. It supports a general JS-region call/inlining capability as a
research target; it does not prove that one particular call site is the score
bottleneck or that compiling/inlining it will pay back.

## Diagnostic run values (not performance evidence)

| Engine | One-round Score | Peak RSS |
| -- | --: | --: |
| Quench JIT, instrumented | 13.64 | 40.3 MiB |
| Bun JIT | 281.72 | 104.7 MiB |
| Node JIT | 196.88 | 163.0 MiB |

These differ from the separate clean three-round screen and are retained only
as part of the profile report. Do not use them for score comparisons.

## Reproduction

```sh
target/production/quench-bench --fixtures hash-map --suite jetstream3 --jit \
  --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --out target/jit-hash-map-diagnostics-current.json
```
