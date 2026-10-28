# TOD-137: first-call release every 5 ms under MemoryFirst — kept (policy-scoped)

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. Plain production `quench-node`, compile
cache warm, binary evicted from the page cache before each run, pairs in alternating order.
Measurement binary: `meas-gc-early-release`. Candidate: `cand-gc-first-call-release`.

## Finding

On several unmet V8-v7 fixtures, the run's max RSS falls during warm-up, not in the steady
state. Samples from `/proc` on the board binary (static + BOLT):

| Fixture | When the max falls |
| -- | -- |
| DeltaBlue, no JIT | 20–28 ms |
| RayTrace, no JIT | 18–24 ms |
| RayTrace, `--jit` | about 52 ms |
| RegExp, both modes | 63–102 ms |

What happens in DeltaBlue:

1. The entry's release leaves 388 KiB of file pages at 12.9 ms.
2. Its first 2 ms of execution fault in about 1.4 MB of code.
3. That code stays resident until the first collection releases it, at about 26 ms.
4. From then on, each cycle needs about 1.05 MB of code.

The first-call compile-phase release, one per 100 ms, is the boundary that falls inside this
warm-up.

## Sweep (same binary, `QUENCH_FIRST_CALL_INTERVAL_MS`, 5 runs)

| Interval | V8-v7 effect | npm effect |
| -- | -- | -- |
| 0 (every first call) | No JIT: Richards −412, DeltaBlue −440, RayTrace −680, NavierStokes −624, RegExp −336, Crypto −220 KiB; scores within noise | Each release costs about 0.35 ms of refaults; 1.4–2.9× slower (TypeScript 313 → 829 ms, ESLint 258 → 736) |
| 2 ms | Most of the gain | +7 to +33% |
| 5 ms | RayTrace −432, RegExp −376, NavierStokes −488, Richards −164, DeltaBlue −120 | +3 to +16% |
| 10 ms | Small gains | +0 to +9% |
| Doubling (1st, 2nd, 4th… first call) | Mixed: DeltaBlue `--jit` +312 KiB, RayTrace `--jit` +164 | Free |

V8-v7 programs make 54–108 first calls; the npm scenarios make 292–1843.

## Candidate: 5 ms under memory-first only (10 runs, `ab-candidate-10-runs.txt`)

Max RSS change, KiB:

| Fixture | No JIT | `--jit` |
| -- | --: | --: |
| Richards | −160 | −32 |
| DeltaBlue | −174 | −92 |
| Crypto | −124 | +32 |
| RayTrace | −154 | +48 |
| RegExp | −222 | −144 |
| NavierStokes | −516 | −30 |
| EarleyBoyer, Splay | flat | flat |
| print1 | −12 | −82 |

Scores are within noise; the A/A floor is ±22 KiB and ±3%.

npm scenarios (`npm/`, 9 runs):

| Scenario | Memory-first | Throughput (default) |
| -- | -- | -- |
| TypeScript transpile | 49.6 → 48.8 MB, 296 → 324 ms | unchanged |
| ESLint lint | 29.5 → 26.0 MB, 255 → 308 ms | unchanged |
| Acorn parse | 16.6 → 16.7 MB, 392 → 421 ms | unchanged |
| Ajv compile | 11.6 → 11.2 MB, 137 → 166 ms | unchanged |

Throughput is the trunk default (TOD-167). Memory-first is the opt-in policy that trades speed
for memory, and it is the one this trade applies to.

`cargo test -p quench-node --lib`: 27 passed.
