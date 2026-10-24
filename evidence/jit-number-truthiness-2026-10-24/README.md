# Unified JS JIT hot-four screen

This is an 11-round JetStream 3 screen from executable revision `38c4fdd32c58bdb93c178e252e8021fb8277787a`. It uses only Crypto, DeltaBlue, NavierStokes, and Richards; `fixture_set_matches` is false, so this is not a full-suite qualification. All selected fixture outputs matched and every engine had 11 valid samples per fixture.

The host was Linux x86_64 (`6.18.55`), with a 4 CPU quota and a 16 GiB memory cgroup. JetStream 3 was pinned at `06785cf861ac44855f168cbbe829278c2802e6de`. Node was v26.10.0, Bun was 1.4.2 with JIT enabled, and Quench ran with `--jit`.

| Fixture | Quench Score | Node Score | Bun Score | Quench max RSS | Node max RSS | Bun max RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Crypto | 109.61 | 313.27 | 674.64 | 34.3 MiB | 90.3 MiB | 54.9 MiB |
| DeltaBlue | 34.43 | 54.46 | 425.93 | 34.3 MiB | 94.1 MiB | 65.2 MiB |
| NavierStokes | 224.12 | 505.92 | 484.76 | 34.3 MiB | 90.4 MiB | 48.6 MiB |
| Richards | 30.85 | 22.77 | 463.88 | 34.3 MiB | 87.1 MiB | 53.6 MiB |

Quench's max RSS was lower than both peers on all four fixtures. Its Score was below Bun on all four and below Node on Crypto, DeltaBlue, and NavierStokes. It led Node on Richards, but this does not meet the 20% score objective.

The separate one-round diagnostic is also a selected-fixture screen, not score evidence. It recorded:

| Fixture | JS native entries | OSR native region runs |
| --- | ---: | ---: |
| Crypto | 1,529 (`nbits`) | 7,949,086 (`am3`) |
| DeltaBlue | 278,307, concentrated in small helpers | 0 |
| NavierStokes | 0 | 31,122 across 7 functions |
| Richards | 0 | 0 |

Richards remains uncovered by native code in this diagnostic. Counts show entry coverage, not useful time spent in optimized code. The screen uses the executable built from the clean `38c4fdd32` source revision; the diagnostic was collected while later, unbuilt OSR source edits were present in the worktree, so its `source_dirty` metadata should not be read as describing the executable.

Commands:

```sh
target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out evidence/jit-number-truthiness-2026-10-24/jit-hot-four-score.json

target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out evidence/jit-number-truthiness-2026-10-24/jit-hot-four-diagnostic.json
```
