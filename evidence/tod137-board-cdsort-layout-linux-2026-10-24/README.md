# V8-v7 max RSS board with the documented call-graph BOLT layout

The board pipeline (gc lane's `pipe9/10/11.sh`) had bolted with `-reorder-functions=user
-function-order=…`, the trained first-execution order. `docs/README.md` has documented
`-reorder-functions=cdsort` since `ae670246df`. An earlier test of `cdsort` used stale
profiles and came out mixed. It is re-tested here on fresh profiles.

**Binaries:**
- **Same input:** both use `qb9.static`, v2-gc `1bc187d4b5` as a static production build.
- **Same profile:** both use fresh callgrind training (`bolt9/out/profile.fdata`).
- **Only difference:** `-reorder-functions` is `user` in `qb9` and `cdsort` in `qcd`.

**Host:** 4-vCPU Intel Xeon @ 2.80GHz x86_64 VM, 16 GB, MemoryFirst.

## A/B (`cdab.txt`, 9 interleaved runs, binaries evicted)

Max RSS in KiB, user → cdsort:

| Row | Median | Worst | Score |
| -- | --: | --: | --: |
| Crypto | 3000 → 2908 | 3084 → 2960 | 2575 → 2576 |
| Crypto `--jit` | 3100 → 2932 | 3164 → 3032 | 2585 → 2490 |
| RayTrace | 2720 → 2680 | 2816 → 2768 | 2137 → 2152 |
| RayTrace `--jit` | 2720 → 2652 | 3000 → 2900 | 2025 → 2116 |
| RegExp | 5664 → 5460 | 5752 → 5588 | 2510 → 2585 |
| RegExp `--jit` | 5664 → 5588 | 5752 → 5644 | 2600 → 2630 |
| Richards | 2436 → 2380 | 2696 → 2708 | 1419 → 1447 |
| Richards `--jit` | 2456 → 2604 | 2696 → 2648 | 1527 → 1542 |
| DeltaBlue | 2820 → 2652 | 2912 → 2668 | 1223 → 1282 |
| DeltaBlue `--jit` | 2944 → 2668 | 2960 → 2792 | 1235 → 1306 |
| NavierStokes | 3948 → 3796 | 3948 → 3816 | 2851 → 2841 |
| NavierStokes `--jit` | 3964 → 3792 | 3964 → 3952 | 2853 → 2818 |

## Board (`board-qcd.txt`, 5 runs per engine)

Ratio is Quench's max RSS over 0.8 × the best other engine's best run.

| Fixture | no-JIT worst (median) | --jit worst (median) |
| -- | --: | --: |
| Richards | 0.86 (0.83) | 0.93 (0.84) |
| DeltaBlue | 0.86 (0.84) | 0.90 (0.88) |
| Crypto | 0.98 (0.93) | 0.95 (0.95) |
| RayTrace | 1.02 (0.97) | 0.93 (0.93) |
| EarleyBoyer | 0.45 (0.44) | 0.45 (0.44) |
| RegExp | 1.01 (0.98) | 0.98 (0.98) |
| Splay | 0.64 (0.64) | 0.64 (0.64) |
| NavierStokes | 0.91 (0.90) | 0.90 (0.90) |

- **Passing:** 14 of 16 rows on the worst run, and every median.
- **Still missing:**
  - RayTrace without JIT, by 48 KiB: worst 2896 against a 2848 target.
  - RegExp without JIT, by 71 KiB: worst 5796 against 5725.
