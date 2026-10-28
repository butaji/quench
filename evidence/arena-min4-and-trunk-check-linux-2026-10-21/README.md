# Trunk check of TOD-166, and the arena minimum of the inline count plus one

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`. Every binary runs with
`QUENCH_COMPILE_CACHE=`, so no side starts with a warmer compile cache.

Binaries:
- `vm167`: v2-merged at the time of measuring.
- `gc167`: v2-gc at b8e8e0ae2, which includes TOD-166 and the startup snapshot.
- `min4`: `cand-gc-arena-min4` (1886c2b4c) on top of `gc167`.

## Trunk check: v2-merged → v2-gc

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 5 | ×1.002 | ×0.954 |
| Frameworks | 5 | ×0.983 | ×0.996 |
| npm top 300 | 1 | — | ×0.9932, none worse by 5%, statuses unchanged |

TOD-166's saving holds on top of the startup snapshot.

## Arena minimum of four values: kept

`MIN_CAPACITY` becomes `(INLINE_PROPERTY_COUNT + 1).next_power_of_two()`. An object that
widens because of a double keeps its first arena range as it grows past the inline count.

Rule, posted on TOD-137 before measuring:
- Speed up on V8-v7 or JetStream, with no suite below ×0.99.
- RSS ≤ ×1.005 per suite; npm ≤ ×1.005 with no new failures.

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 10 | ×1.002 (raytrace ×1.040, 7/10) | ×1.000 |
| JetStream (12 tests) | 5 | ×1.004 | ×1.002 |
| Frameworks | 5 | ×1.037 | ×1.002 |
| npm top 300 | 1 | — | ×0.9994, none worse by 5% |

The suite gains are within noise. The targeted raytrace gain matches the instruction-count
reduction measured with callgrind (TOD-166 evidence). Memory is neutral, so the round 2
npm ×1.018 was the compile-cache artifact.
