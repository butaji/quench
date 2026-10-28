# Unified JIT hot-four screen

Date: 2026-10-23 UTC  
Branch / source: `v2-jit`, commit `b207df134c37d711b36f090f52f0c7d1a9c56127`  
Host: Linux x86_64, 4 CPU quota, 16 GiB memory cgroup.  
Corpus: pinned JetStream 3 corpus `06785cf861ac44855f168cbbe829278c2802e6de`.  
Engines: Node v26.10.0, Bun 1.4.2.  
Quench production binaries were built from the source commit above.

This is an 11-round paired screen over Crypto, DeltaBlue, NavierStokes, and Richards, not a qualification run: the full fixture set is not selected, so the runner marks `qualification_ready=false`. Outputs were valid and equal for all selected fixtures. Raw score artifacts are `jit-hot-four-score-current.json` and `nojit-hot-four-score-current.json`. `jit-hot-four-diagnostic-current.json` is a separate one-round run with JIT diagnostics enabled; its scores are not used for comparison.

## JIT mode

| Fixture | Quench | Node | Bun | Quench RSS | Node RSS | Bun RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Crypto | 116.59 | 288.23 | 755.45 | 34.1 MiB | 90.1 MiB | 54.8 MiB |
| DeltaBlue | 40.62 | 57.53 | 433.20 | 34.1 MiB | 93.9 MiB | 65.6 MiB |
| NavierStokes | 233.10 | 530.58 | 490.09 | 34.1 MiB | 90.5 MiB | 48.4 MiB |
| Richards | 25.92 | 21.50 | 296.32 | 34.1 MiB | 87.0 MiB | 53.8 MiB |

## Default no-JIT mode

| Fixture | Quench | Node | Bun | Quench RSS | Node RSS | Bun RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Crypto | 57.42 | 34.70 | 47.20 | 33.7 MiB | 74.5 MiB | 39.2 MiB |
| DeltaBlue | 38.72 | 24.91 | 31.20 | 33.7 MiB | 79.6 MiB | 40.3 MiB |
| NavierStokes | 62.07 | 40.08 | 54.61 | 33.7 MiB | 78.0 MiB | 34.0 MiB |
| Richards | 31.69 | 12.87 | 29.54 | 33.7 MiB | 74.4 MiB | 38.9 MiB |

Quench's no-JIT score beats both engines on these four fixtures. Its lead over Bun is under 20% on NavierStokes and Richards. No-JIT RSS is lower than both measured engines on this subset.

JIT mode remains well short of the target. It has lower RSS than both engines on the selected fixtures, but score trails Bun on all four and trails Node on Crypto, DeltaBlue, and NavierStokes. Richards JIT score beats Node but is far below Bun. There is no 20% JIT win.

## Diagnostic coverage

From the separate one-round `--jit-diagnostics` run, summed per-function stats:

| Fixture | JS native entries | OSR region runs |
| --- | ---: | ---: |
| Crypto | 1,529 | 7,949,086 |
| DeltaBlue | 278,307 | 0 |
| NavierStokes | 0 | 31,122 |
| Richards | 0 | 0 |

Richards' hot loop is rejected at `JumpBinaryFalse`; the next coverage work should generalize guarded conditional control flow. Effectful `Call`/`LoadNameCall` coverage needs an exact-PC rooted continuation before deopt-safe lowering.
