# Shared compiled region exit screen

Date: 2026-10-24 UTC. Source: `v2-jit` at `e3d756a7fe5bedd6967d182f3b9e8b7c645a550c`. Host: Linux x86_64, 4 CPU quota, 16 GiB memory cgroup. Corpus: JetStream 3 revision `06785cf861ac44855f168cbbe829278c2802e6de`. Engines: Node v26.10.0 and Bun 1.4.2. Production Quench binaries were built from the recorded source revision.

This is an 11-round paired screen on Crypto, DeltaBlue, NavierStokes, and Richards, not a suite qualification run: it uses only four fixtures and reports `qualification_ready=false`. Each fixture's outputs validated equal across engines. The one-round diagnostic is separate and is not used for score comparison.

## JIT mode

| Fixture | Quench | Node | Bun | Quench RSS | Node RSS | Bun RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Crypto | 129.17 | 331.01 | 730.75 | 33.9 MiB | 90.2 MiB | 54.8 MiB |
| DeltaBlue | 38.53 | 62.81 | 401.66 | 33.9 MiB | 93.9 MiB | 68.2 MiB |
| NavierStokes | 233.06 | 491.67 | 523.35 | 33.9 MiB | 90.5 MiB | 48.7 MiB |
| Richards | 27.30 | 21.57 | 456.62 | 33.9 MiB | 87.0 MiB | 53.8 MiB |

Quench RSS is lower than Node and Bun on all four fixtures. JIT score loses to Bun on all four and loses to Node on Crypto, DeltaBlue, and NavierStokes. There is no 20% JIT score win.

## JIT diagnostic coverage

The separate one-round diagnostic reports the same hot-four coverage counts as the previous checkpoint: Crypto has 1,529 JS native entries and 7,949,086 OSR region runs; DeltaBlue has 278,307 native entries and no OSR runs; NavierStokes has no JS native entries and 31,122 OSR runs; Richards has neither JS native entries nor OSR runs. Richards remains rejected before native entry; the exact-PC exit ABI change does not broaden coverage by itself.

## No-JIT comparison

The attempted same-source 11-round no-JIT screen was interrupted before it wrote an artifact. The historical no-JIT 11-round screen in `evidence/jit-unified-region-hot-four-2026-10-23/nojit-hot-four-score-current.json` is from source `b207df134c37d711b36f090f52f0c7d1a9c56127`, not this commit. This checkpoint changes compiled/JIT region code only, so that baseline is informative but is not a same-source measurement. Its Quench/Node/Bun scores were Crypto 57.42/34.70/47.20, DeltaBlue 38.72/24.91/31.20, NavierStokes 62.07/40.08/54.61, Richards 31.69/12.87/29.54. Quench beat both peers on those four fixtures, with less than a 20% lead over Bun on NavierStokes and Richards.
