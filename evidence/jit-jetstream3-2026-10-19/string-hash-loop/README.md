# Guarded string hash loop JIT measurement

This clean-source 11-round JetStream 3 Web-SSR run measures commit
`4cacc5b1642e686ce35cd86624d52003d8a7315d`. The pinned suite revision was
`06785cf861ac44855f168cbbe829278c2802e6de`, and all 11 rounds produced equal
outputs across Quench, Bun 1.4.3, and Node 26.12.0.

| Runtime | Median score | Median maximum RSS |
| --- | ---: | ---: |
| Quench JIT | 16.73 | 129.0 MiB |
| Bun JIT | 122.17 | 226.5 MiB |
| Node JIT | 133.92 | 203.3 MiB |

Quench's peak RSS was lower, but the score was far below both peers. Paired
Quench-to-peer score ratios were 0.135 [0.121, 0.142] versus Bun and 0.125
[0.120, 0.139] versus Node. The comparison did not qualify. Diagnostics on a
separate three-round run showed `quickHash` entered native code 61 times; the
primary Web-SSR workload still had no native entries. This string-loop
specialization therefore does not close the score gap.

Reproduce with:

```sh
cargo build --locked --profile production -p quench-node -p quench-bench
PATH="/path/to/pinned-rustc/bin:$PATH" target/production/quench-bench \
  --fixtures web-ssr --suite jetstream3 --jit --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node <node-26.12.0> --bun <bun-1.4.3> \
  --out target/web-ssr-string-hash-jit.json
target/production/quench-bench --analyze target/web-ssr-string-hash-jit.json \
  --out target/web-ssr-string-hash-jit-analysis.json
```
