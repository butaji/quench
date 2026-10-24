# MemoryFirst loop tier compiles without Cranelift's optimizing pass

Question: under `--jit`, what sets the MemoryFirst max RSS of Crypto and NavierStokes, and can
the memory policy remove it?

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. Static production builds (unbolted),
binaries evicted before each run, compile cache warm.

## Where the peak comes from

- `rsssplit3.py` on NavierStokes `--jit`: steady anonymous memory stays at 3.4-4.5 MB, but one
  ~2.7 MB spike at ~145 ms sets the 7.9 MB peak.
- `strace -k` of the anonymous mmaps on warm runs: both of Crypto's transient chunks
  (266,240 and 135,168 bytes) come from `regalloc2` under `Context::compile` in
  `compile_osr_region`. On a cold run, entry-script parse-goal detection also parses with OXC.
  Warm runs read that from the compile cache.
- Massif at the NavierStokes peak, from the dynamic build: regalloc2 holds about 2.8 MB, the
  e-graph's IR value lists 0.85 MB and lowering 0.56 MB. The region is 3,893 IR instructions
  and 24 KB of code, so the IR is not abnormal in size.

## Same-binary probes (environment switches, 5-11 runs, `--jit`, MemoryFirst)

| Probe | Crypto | NavierStokes | Other rows |
| -- | -- | -- | -- |
| `regalloc_algorithm=single_pass` | -640 KiB, score -22% | -1432 KiB, score -65% | rejected |
| verifier off | 0 | 0 | no memory effect |
| `opt_level=none`, all JS tiers | -512 KiB, -5.4% | -492 KiB, -2.9% | noise |

## Change: `LOOP_TIER_OPT_LEVELS` = `speed` / `none` by `MemoryPolicy`

The A/B compares the base (`v2-gc 758b49c398`) with the change: 11 alternating runs, median,
`--jit`.

| Row | MemoryFirst RSS KiB A → B | Δ | Score A → B |
| -- | --: | --: | --: |
| Richards | 2836 → 2812 | -24 | 1507 → 1436 |
| DeltaBlue | 3132 → 3036 | -96 | 1154 → 1144 |
| Crypto | 4252 → 3676 | **-576** | 4360 → 4238 (-2.8%) |
| RayTrace | 3052 → 3032 | -20 | 2122 → 2066 |
| EarleyBoyer | 6596 → 6572 | -24 | 3726 → 3583 |
| RegExp | 6052 → 6032 | -20 | 2494 → 2490 |
| Splay | 31576 → 31588 | +12 | 4425 → 4530 |
| NavierStokes | 7744 → 7096 | **-648** | 25484 → 23921 (-6.1%) |

Throughput (5 runs) is unchanged, as intended: Crypto 9044 → 9136 and NavierStokes 8592 → 8576,
with equal scores.

Decision: keep. MemoryFirst is the opt-in policy that trades speed for peak memory. Its
largest JIT transient shrinks by 13.5% on Crypto and 8.4% on NavierStokes, and the other rows
are unaffected. The remaining NavierStokes `--jit` gap is regalloc2's own working set for one
large region.
