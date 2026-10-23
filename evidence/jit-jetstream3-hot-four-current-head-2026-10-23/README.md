# Current-head JetStream JIT coverage screen

Host: Linux x86_64, 4 vCPU quota, 16 GiB memory cgroup. Source: `v2-jit`
revision `93101140d9c23761d3f944ae1375578e1e2bd30a`, including
`origin/v2-merged` tip `ec8496b68037c55e5c5ba405697f585db77b0c46`. The production
binary was rebuilt from this exact clean source using rustc 1.99.0. JetStream
corpus: `06785cf861ac44855f168cbbe829278c2802e6de`; Node v26.10.0; Bun 1.4.2.

This is one diagnostic round over a selected four-fixture subset. Every fixture
validated and Quench output matched both peers. `fixture_set_matches` is false
because this is intentionally not the full suite. Scores and RSS are one-round
diagnostics and do not establish a performance win or qualification. Raw report
with hashes, timings, outputs, stderr counters and environment: `report.json`.

| Fixture | Quench score | Bun score | Node score | Quench max RSS | JS native entries | OSR region runs |
| -- | --: | --: | --: | --: | --: | --: |
| crypto | 42.22 | 591.78 | 198.53 | 33.1 MiB | 1,529 | 7,949,086 |
| delta-blue | 7.79 | 303.16 | 48.42 | 34.4 MiB | 278,307 | 0 |
| navier-stokes | 166.26 | 184.27 | 178.58 | 31.1 MiB | 0 | 31,122 |
| richards | 9.72 | 212.56 | 10.19 | 30.6 MiB | 0 | 0 |

DeltaBlue's native entries are concentrated in three anonymous functions
(242,369; 23,969; 11,969). Richards still has no native function or OSR entries.
Crypto's `am3` loop and NavierStokes numeric loops enter OSR; NavierStokes has
no JS function-native entries. This confirms the current broad JS coverage gap;
it does not measure exclusive native time.

Reproduction from repository root:

```sh
target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out target/jit-hot-four-current.json
```
