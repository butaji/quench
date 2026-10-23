# Shared Number bitwise stage: JetStream 3 hot-four screen

**Source:** `v2-jit` commit `b00f1cd5e458c758e1555ede913a78d916dd6831`, clean.  
**Build:** `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo build --locked --profile production -p quench-node -p quench-bench`.  
**Host:** Linux x86_64, kernel 6.18.55, 4 CPU quota, 16 GiB cgroup memory.  
**Corpus:** JetStream 3.0 revision `06785cf861ac44855f168cbbe829278c2802e6de`.  
**Peers:** Node v26.10.0 and Bun 1.4.2, both with optimizing JITs enabled. The unsupported QJS comparison was disabled with `/bin/true`.

Each selected fixture completed 11 valid rounds with equal outputs. This selected subset is not qualification-ready and is not a full JetStream score claim.

| Fixture | Quench | Bun | Node | Quench/Bun | Quench/Node | Quench RSS | RSS/Bun | RSS/Node |
| -- | --: | --: | --: | --: | --: | --: | --: | --: |
| Crypto | 114.35 | 710.52 | 284.62 | 0.161x | 0.402x | 34.25 MiB | 0.621x | 0.379x |
| DeltaBlue | 33.52 | 400.36 | 53.66 | 0.084x | 0.625x | 34.25 MiB | 0.535x | 0.361x |
| NavierStokes | 209.90 | 442.53 | 471.30 | 0.474x | 0.445x | 34.25 MiB | 0.703x | 0.379x |
| Richards | 25.43 | 363.90 | 18.39 | 0.070x | 1.383x | 34.25 MiB | 0.637x | 0.394x |

The JIT score goal is not met. Quench remains below Bun on all four fixtures and below Node on three. Peak RSS is lower than both peers on every fixture. This operation-semantics change did not provide broad hot-four coverage; per-function native coverage remains a separate diagnostic that must be measured with `--jit-diagnostics`.

Reproduce from the repository root:

```sh
target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true \
  --out target/jit-bitwise-shared-stage-hot-four-11.json
```

The raw report is `report.json`. The compiler run succeeded; the runtime library tests passed 852/852 before this production screen.
