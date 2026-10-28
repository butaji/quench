# JetStream 3 `hash-map` JIT screen

Three randomized paired rounds on clean source revision
`1234110111d8120d20ae6f00c258149096c10fc7`. All fixture assertions passed
and Quench output matched both peer engines. This is an optimization screen,
not an 11-round qualification.

| Engine | Median Score | Median peak RSS |
| -- | --: | --: |
| Quench JIT | 5.64 | 39.8 MiB |
| Bun JIT | 162.01 | 110.3 MiB |
| Node JIT | 89.16 | 160.6 MiB |

Quench used less peak RSS but scored about 0.035× Bun and 0.063× Node. The
20%-ahead score and lower-RSS goal remains unmet. Raw per-round samples,
execution order, comparator proofs, and hashes are in `report.json`.

## Reproduction

```sh
target/production/quench-bench --fixtures hash-map --suite jetstream3 --jit \
  --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --out target/jit-hash-map-screen-current.json
```

Production Quench binary SHA-256:
`9b0b1e9621d3a3200db35d59b08e0d625409db4b23cf0ca16b442b8085418f56`.
Host: Linux x86_64, 4 CPU quota, 16 GiB cgroup memory limit.
