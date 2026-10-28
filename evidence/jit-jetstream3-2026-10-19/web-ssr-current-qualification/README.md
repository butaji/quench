# JetStream 3 `web-ssr` JIT baseline

This is an 11-round matched Linux x86_64 comparison from clean source commit
`dbc87555796d4045fd48c3a106b95c55c0dc1a80` and the pinned JetStream 3.0
corpus at `06785cf861ac44855f168cbbe829278c2802e6de`. Stock JetStream
validation succeeded and outputs matched in all rounds. Node and Bun JITs were
verified by the runner.

Median Score / maximum RSS:

| Runtime | Score | Maximum RSS |
| --- | ---: | ---: |
| Quench `--jit` | 11.57 | 131.6 MiB |
| Bun 1.4.3 | 93.06 | 221.6 MiB |
| Node 26.12.0 | 90.92 | 207.5 MiB |

Paired Quench-to-peer ratio medians and 95% intervals:

| Peer | Score ratio | Maximum-RSS ratio | Result |
| --- | ---: | ---: | --- |
| Bun | 0.1291 (0.1067–0.1389) | 0.5939 (0.5910–0.6034) | Lower RSS; Score loses |
| Node | 0.1224 (0.0991–0.1325) | 0.6366 (0.6224–0.6508) | Lower RSS; Score loses |

The intervals exclude parity. This is a valid single-fixture baseline, not a
win: Quench is about 7.7× behind Bun and 8.2× behind Node on Score, while
using less peak RSS. The host is Linux x86_64 with a four-CPU quota; M4
qualification remains outstanding.

Raw samples, engine provenance, input hashes and the paired bootstrap analysis
are in [`report.json`](report.json) and [`analysis.json`](analysis.json).

Reproduce from the repository root after building the production binaries:

```sh
source /workspace/quench-build-env.sh
target/production/quench-bench --fixtures web-ssr --suite jetstream3 --jit \
  --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /path/to/node-v26.12.0/bin/node \
  --bun /path/to/bun-v1.4.3/bun \
  --out target/web-ssr-jit.json
target/production/quench-bench --analyze target/web-ssr-jit.json \
  --out target/web-ssr-jit-analysis.json
```
