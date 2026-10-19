# Web-SSR JIT baseline

This clean-source, 11-round Linux x86_64 run records the current `v2-jit`
baseline before the next JavaScript compiler extension. Source revision is
`8715445b38a7fe9a830de25e91567797ade0dfa4`; JetStream is pinned at
`06785cf861ac44855f168cbbe829278c2802e6de`. The stock JetStream driver
validated all 11 rounds and outputs matched.

| Engine | Median Score | Median maximum RSS |
| --- | ---: | ---: |
| Quench `--jit` | 15.75 | 125.9 MiB |
| Bun 1.4.3 | 129.63 | 219.7 MiB |
| Node 26.12.0 | 125.08 | 214.3 MiB |

Paired bootstrap ratios (Quench / peer, 95% interval):

| Peer | Score ratio | Maximum-RSS ratio |
| --- | ---: | ---: |
| Bun | 0.124 [0.113, 0.131] | 0.575 [0.569, 0.589] |
| Node | 0.128 [0.123, 0.135] | 0.589 [0.584, 0.631] |

Quench clears the RSS bars but loses Score by about 8×, so this fixture does
not qualify. A separate one-round diagnostic run on the same source revision
recorded no native JavaScript entries among 116 tracked functions. This
indicates a compiler-coverage gap; the 11-round report above is the acceptance
measurement. The diagnostics report's `source_dirty` flag reflects untracked
scratch files present during that exploratory run; no tracked source changed.

A one-round `hash-map` diagnostic did enter its compiled `equals` function
53,999,969 times, but scored 5.88 versus Bun 476.80 and Node 350.90 while
using 70.3 MiB RSS versus 85.0 and 158.7 MiB. An attempted integer-pair
argument conversion optimization measured 5.24 / 70.3 MiB and was reverted;
the single-round screens are diagnostic only and are not paired evidence.

Reproduction:

```sh
cargo build --locked --profile production -p quench-node -p quench-bench
target/production/quench-bench --fixtures web-ssr --suite jetstream3 --jit \
  --runs 11 --timeout-ms 600000 --quench target/production/quench-node \
  --node <node-26.12.0> --bun <bun-1.4.3> \
  --checkpoint target/web-ssr-jit.checkpoint.json \
  --out target/web-ssr-jit.json
target/production/quench-bench --analyze target/web-ssr-jit.json \
  --out target/web-ssr-jit-analysis.json
```
