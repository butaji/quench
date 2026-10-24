# OSR numeric truthiness branches: matched JetStream 3 hot-four screen

The executable was built from source commit `41a735811` (the following evidence-only commit is `611606432`). Both score reports used the same executable, host, corpus, and four selected JetStream 3 fixtures. They contain 11 valid rounds per engine and fixture, with equal validated output. `fixture_set_matches=false` and `qualification_ready=false`: this hot-four subset is a coverage screen, not full JetStream qualification.

Host: Linux x86_64, kernel `6.18.55`, four-CPU quota, 16 GiB memory cgroup. JetStream 3 is pinned at `06785cf861ac44855f168cbbe829278c2802e6de`. Peers were Node v26.10.0 and Bun 1.4.2. In no-JIT mode, Quench ran without `--jit` and Node ran with `--jitless`; Bun ran with its default options.

## JIT mode

Median Score / median max RSS in MiB:

| Fixture | Quench | Node | Bun |
| --- | ---: | ---: | ---: |
| Crypto | 114.53 / 34.2 | 310.48 / 90.6 | 701.00 / 54.7 |
| DeltaBlue | 38.75 / 34.2 | 63.96 / 93.6 | 395.54 / 64.7 |
| NavierStokes | 228.50 / 34.2 | 536.44 / 90.5 | 488.56 / 48.5 |
| Richards | 31.98 / 34.2 | 28.58 / 86.9 | 526.04 / 53.8 |

Quench's RSS is lower than both peers on all four fixtures. Bun scores higher on all four; Node scores higher on Crypto, DeltaBlue, and NavierStokes. The 1.20x score goal is not met.

## No-JIT mode

Median Score / median max RSS in MiB:

| Fixture | Quench | Node | Bun |
| --- | ---: | ---: | ---: |
| Crypto | 63.89 / 33.8 | 38.16 / 74.5 | 53.78 / 39.2 |
| DeltaBlue | 41.54 / 33.8 | 24.50 / 79.7 | 34.90 / 40.3 |
| NavierStokes | 68.00 / 33.8 | 40.38 / 78.0 | 56.56 / 34.0 |
| Richards | 27.69 / 33.8 | 11.00 / 74.3 | 28.86 / 38.8 |

Quench beats both peers on the first three scores. Its leads over Bun on Crypto and DeltaBlue are under 20%; it trails Bun on Richards. RSS is below both peers on all four fixtures, narrowly below Bun on NavierStokes. The 1.20x score goal is not met.

## Native-entry diagnostics

This is a separate one-round instrumented run; its scores are excluded from the tables. Summed `QUENCH_JS_JIT_STATS`:

| Fixture | JS native entries | OSR region runs |
| --- | ---: | ---: |
| Crypto | 1,529 | 7,949,086 |
| DeltaBlue | 278,307 | 0 |
| NavierStokes | 0 | 31,122 |
| Richards | 0 | 0 |

Richards remains uncovered by native code. This numeric truthiness branch extension therefore did not change hot-four native-entry coverage or produce a JetStream win. The instrumented report marks the source tree dirty because the new evidence directory existed during collection; the executable itself came from source commit `41a735811`.

The runtime library suite passed 863/863 before a test-only function-record lifecycle adjustment. The final focused parity test passed with and without `QUENCH_JS_JIT_STATS`; the instrumented test compiled one OSR site and ran native code 48 times. `git diff --check` passed.

## Reproduction

```sh
target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out evidence/jit-osr-truthiness-2026-10-24/jit-hot-four-score.json

target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out evidence/jit-osr-truthiness-2026-10-24/nojit-hot-four-score.json

target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out evidence/jit-osr-truthiness-2026-10-24/jit-hot-four-diagnostic.json
```
