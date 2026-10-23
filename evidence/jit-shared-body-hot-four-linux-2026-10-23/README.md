# Shared JS numeric Body migration: JetStream 3 hot-four screen

**Source:** `v2-jit` commit `80ca341253c8200465f74f5f46ad8f148f1fc231`, clean.  
**Build:** `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo build --locked --profile production -p quench-node -p quench-bench`.  
**Host:** Linux x86_64, kernel 6.18.55, 4 CPU quota, 16 GiB cgroup memory.  
**Corpus:** JetStream 3.0 revision `06785cf861ac44855f168cbbe829278c2802e6de`.  
**Peers:** Node v26.10.0 (V8 optimizing tier verified), Bun 1.4.2 (`useJIT=true` verified).

The selected fixtures are the hot-four JIT coverage set, not the full JetStream corpus. Each engine completed 11 rounds for each fixture and output validation passed. The benchmark report is complete but `qualification_ready=false` because `fixture_set_matches=false`; the stock analyzer refuses to qualify this subset. These figures are useful as a matched diagnostic screen, not a full-suite qualification or a claim of a 20% score win.

| Fixture | Quench | Bun | Node | Quench/Bun | Quench/Node | Quench RSS | RSS/Bun | RSS/Node |
| -- | --: | --: | --: | --: | --: | --: | --: | --: |
| Crypto | 79.78 | 596.84 | 217.87 | 0.134x | 0.366x | 31.57 MiB | 0.575x | 0.339x |
| DeltaBlue | 26.85 | 333.59 | 43.23 | 0.080x | 0.621x | 34.14 MiB | 0.525x | 0.352x |
| NavierStokes | 201.06 | 367.68 | 426.35 | 0.547x | 0.472x | 34.14 MiB | 0.704x | 0.367x |
| Richards | 24.30 | 387.89 | 18.34 | 0.063x | 1.325x | 32.71 MiB | 0.607x | 0.365x |

The matched score screen misses the 1.20x target against both peers on three of four tests; Richards exceeds Node but remains far behind Bun. Quench RSS is lower than both peers for every selected test. The architectural relocation did not change candidate admission or native coverage.

A separate one-round `--jit-diagnostics` sample recorded native JS function entries: Crypto 1,529; DeltaBlue 278,307; NavierStokes 0; Richards 0. This is diagnostic coverage only and includes instrumentation overhead. Richards still needs native coverage; Native entry counts on Crypto/DeltaBlue are not broad workload coverage.

Reproduce the score screen from the repository root:

```sh
target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --out target/jit-shared-body-hot-four-11.json
```

The raw score report and diagnostics are retained alongside this note. The single-round diagnostic report must not be used for score qualification.
