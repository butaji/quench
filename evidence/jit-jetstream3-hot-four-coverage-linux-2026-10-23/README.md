# JetStream hot-workload JIT coverage screen

Host: Linux x86_64, 4 vCPU quota, 16 GiB memory cgroup. Source: `v2-jit` at
`1d70d4b340590d8b3b36e2619cee06197133c4bc`. The production Quench executable
was built from that source before this diagnostic. JetStream corpus:
`06785cf861ac44855f168cbbe829278c2802e6de`. Engines: Node v26.10.0 and Bun
1.4.2. This is one diagnostic round per fixture with `--jit-diagnostics`, so
scores are not qualification results.

The selected JetStream fixtures are the stock `crypto`, `delta-blue`,
`navier-stokes`, and `richards` tests. They are related to similarly named
V8-v7 fixtures, but are not those V8-v7 runs.

| Fixture | Quench score | Bun score | Node score | Quench RSS | JS function native entries | OSR region runs |
| -- | --: | --: | --: | --: | --: | --: |
| crypto | 95.26 | 696.96 | 268.98 | 34.2 MiB | 1,529 | 7,949,086 |
| delta-blue | 12.08 | 338.74 | 54.05 | 34.3 MiB | 278,307 | 0 |
| navier-stokes | 221.24 | 415.66 | 449.59 | 34.3 MiB | 0 | 29,773 |
| richards | 16.72 | 434.88 | 23.18 | 34.3 MiB | 0 | 0 |

The counts are from the Quench diagnostic stderr records for the single measured
round. They are execution coverage, not exclusive CPU samples. Crypto is
primarily using its loop OSR region, DeltaBlue has a small set of native
function entries amid many interpreted lane calls, NavierStokes reaches OSR
regions but no compiled JS function entry, and Richards reaches neither native
path in this sample. The next compiler milestone should cover hot effectful JS
calls in a frame-preserving region with exact interpreter continuation; these
fixtures show the available native loop path alone leaves large score gaps.

The first attempt used V8-v7 labels for DeltaBlue and NavierStokes; JetStream
reported those names missing. That report is excluded. This report uses the
canonical JetStream IDs above, all four output validations passed, and the
stock harness/iteration counts were used.

Reproduce from the repository root:

```sh
target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out target/jit-hot-four-corrected-diagnostic-2026-10-23.json
```
