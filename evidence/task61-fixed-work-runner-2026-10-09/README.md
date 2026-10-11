# Task 61 fixed-work runner pilot

This record validates the diagnostic fixed-work mode added to
`quench-bench`. It is not a performance comparison or a task-61 qualification
run: there was one round, and the Quench executable was unchanged from the
baseline path during this pilot.

The runner materialized the standard V8-v7 `base.js` plus each fixture, called
each benchmark's `Setup`, invoked that benchmark's `run()` the named number of
times, then called `TearDown`. It recorded process wall time, cycles,
instructions and maximum RSS. The report sets `qualification_ready` to false.

Command:

```sh
target/pinned/298e2afc13500ff2d52b8bdbb9a80b998c1af7574538b99d053e47d5869c18b7/fixed-work-runner \
  --all --fixed-work --runs 1 \
  --quench target/pinned/a4975440c92cb288f27ac291a77b5b005128529e5b419d2146a7657f5b43970e/quench-node \
  --qjs /opt/homebrew/bin/qjs \
  --bun /opt/homebrew/bin/bun \
  --node /opt/homebrew/bin/node \
  --timeout-ms 300000 \
  --out target/fixed-work-pilot-rerun.json
```

The report completed all eight fixtures, and semantic stdout matched across
Quench, QuickJS, Bun with JIT disabled, and Node with `--jitless`. The named
`run()` counts and Quench process measurements were:

| Fixture | Calls per benchmark | Wall time | Cycles | Retired instructions | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| Crypto | 4 | 8.925 s | 7,251,897,924 | 36,195,615,233 | 20.8 MiB |
| DeltaBlue | 40 | 5.334 s | 7,043,753,728 | 26,921,791,553 | 20.0 MiB |
| EarleyBoyer | 3 | 2.720 s | 6,381,794,993 | 30,814,101,241 | 55.5 MiB |
| NavierStokes | 5 | 2.085 s | 6,160,607,066 | 43,316,516,379 | 20.0 MiB |
| RayTrace | 10 | 1.333 s | 4,506,648,542 | 24,936,370,216 | 20.3 MiB |
| RegExp | 1 | 2.038 s | 6,494,414,845 | 33,604,122,456 | 43.2 MiB |
| Richards | 100 | 1.150 s | 3,654,173,235 | 21,673,738,465 | 19.6 MiB |
| Splay | 300 | 1.517 s | 4,816,257,980 | 26,119,491,032 | 259.7 MiB |

This final provenance run confirms the exact rebuilt runner and equal fixed work,
but its timing is host-noisy: an `uptime` sample immediately afterward showed a
load average of 19.41, and the Crypto process recorded 5,500 involuntary
context switches. Treat these single-round time and cycle values as diagnostics,
not a performance gate. Fixed work removes adaptive-work variation; it does not
remove scheduler, frequency or GC timing noise.

The report records host `Mac16,10`, engine executable hashes, the production
runner hash, the source revision and the materialized-input hashes. Quench's
executable SHA-256 was
`a4975440c92cb288f27ac291a77b5b005128529e5b419d2146a7657f5b43970e`; the
measurement runner's SHA-256 was
`298e2afc13500ff2d52b8bdbb9a80b998c1af7574538b99d053e47d5869c18b7`.

The fixed-work unit filter passed 3/3 tests, the full benchmark-binary test set
passed 5/5, and `cargo check -p quench-bench --bin quench-bench` passed.
Setup-time work is included in process counters; for example, the existing
RegExp setup performs its own warmup call in addition to the plan's one
explicit `run()` call.
The alternating Quench-only path also passed a two-round Richards smoke test;
the order rotated between `quench_baseline` and `quench_candidate`, both pinned
to the same executable for this plumbing check. See
[`quench-pair-smoke.json`](quench-pair-smoke.json). Qualification continues to
use the unchanged stock adaptive harness.
