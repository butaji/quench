# Throughput young sizing: npm's proposals measured; table kept

Candidates, from v2-npm:
- a fixed minimum young of 128K or 256K weight, with a 2M maximum;
- a young budget that grows with smoothed survival, `young × (SCALE + g × survival) / SCALE`.
  It is now a `GenerationalPacing::survival_growth` field and a gc-tuning knob
  (`QUENCH_GC_SURVIVAL_GROWTH`), 0 in both tables, on branch `cand-gc-survival-young`.

Probe: one gc-tuning static binary (`cand-gc-survival-young` = v2-gc `c2f92fbf8d` + the field),
not PGO. Throughput, no JIT, compile cache warm, alternating arms. x86_64 Linux 6.18 VM,
4 CPUs. Medians:
- npm-top300 scenarios: 5 runs, CPU (user + system) and max RSS;
- V8-v7: 3 runs, Score and max RSS.

`sweep.txt` holds the raw rows.

## npm (CPU change / RSS change against base)

| Package | base | m128 | m256 | g4 | g8 |
| -- | -- | -- | -- | -- | -- |
| typescript-eslint | 1368 ms, 127.9 MB | +6% / +0.2 | −6% / −0.8 | +8% / +3.5 | +6% / +1.1 |
| eslint-plugin | 1319 ms, 125.8 MB | +1% / +1.6 | −7% / −0.3 | +8% / +6.2 | +3% / +1.8 |
| playwright | 657 ms, 79.5 MB | +7% / −0.9 | 0% / +0.9 | +4% / +0.2 | −2% / +1.6 |
| jsdom | 664 ms, 83.9 MB | +3% / +0.1 | −9% / +4.4 | −3% / +0.3 | +4% / +2.6 |

The scenarios after jsdom were not reached: vitest crashes in every arm, the base included.
Its child segfaults inside rolldown's native binding, on trunk too, while Node passes. That
was reported to the coordinator for the N-API owner; it is unrelated to pacing.

## V8-v7 under Throughput (Score change / max RSS change)

| Fixture | base | m128 | m256 | g4 | g8 |
| -- | -- | -- | -- | -- | -- |
| Richards | 1538, 5.2 MB | +6% / +0.1 | −9% / 0 | −3% / −0.1 | +4% / 0 |
| DeltaBlue | 1490, 7.1 MB | −8% / +2.9 | −3% / +8.8 | −18% / +0.1 | −10% / +0.4 |
| Crypto | 2580, 8.6 MB | −1% / +3.7 | −1% / +11.9 | +1% / +0.1 | −1% / +0.4 |
| RayTrace | 2625, 7.2 MB | +2% / +3.0 | −2% / +9.2 | +2% / +0.4 | −3% / +0.5 |
| EarleyBoyer | 5276, 11.1 MB | +6% / +3.6 | −2% / +9.9 | −2% / +1.5 | +2% / +3.2 |
| RegExp | 3040, 10.8 MB | −6% / +4.9 | −7% / +12.5 | −5% / +2.9 | −22% / +4.0 |
| Splay | 4114, 32.0 MB | +2% / 0 | +7% / +0.1 | +4% / +0.1 | −11% / +0.1 |
| NavierStokes | 2823, 5.0 MB | −9% / +0.1 | −5% / +0.1 | −4% / +0.1 | −3% / +0.1 |

## Decision

The Throughput table stays as it is.
- **256K minimum:** the only arm that sped npm up here (−6 to −9% on three of four packages).
  It costs 9–12 MB of max RSS on five V8-v7 fixtures; Crypto at 20.5 MB would approach Bun
  no-JIT's 22 MB. Its V8-v7 scores do not improve.
- **Survival growth:** the g4 and g8 arms gave no consistent npm gain on this host
  (v2-npm measured one). They cost Score on DeltaBlue, RegExp and Splay.
- **Kept for later:** the field stays on the candidate branch, so the lanes can re-measure it
  with PGO builds and more rounds.
