# Deegen-to-Quench V8v7 comparison (2026-09-11)

Both runs used the same `js-engine-benchmark` V8v7 fixture set and the
upstream exact timing policy (1 s measurement window, 32 benchmark runs).

## Historical pre-migration dispatch

The initial Quench dispatch path measured before the full source VM was moved
into `quench-runtime`:

| Fixture | Source Deegen | Quench migrated VM |
| --- | ---: | ---: |
| Richards | 1212.94 | 34.60 |
| DeltaBlue | 1011.83 | 28.50 |
| Crypto | 2798.78 | 34.90 |
| RayTrace | 1780.49 | 85.20 |
| EarleyBoyer | 3113.93 | 55.50 |
| RegExp | 3160.93 | 60.80 |
| Splay | 3244.28 | 281.00 |
| NavierStokes | 6198.78 | 101.00 |
| **Geomean** | **2428.71** | **63.98** |

That earlier run measured the old Quench dispatch path. After moving the full
Deegen VM into the runtime-owned private core, the canonical exact driver
(`scripts/run-v8v7.sh`, 1 s window, 32 runs; thin-LTO binaries) produced:

| Run | Geomean |
| --- | ---: |
| Source Deegen | 2,512.49 |
| Quench runtime-owned migrated core | 2,524.36 |

All eight fixtures were valid in both runs, including EarleyBoyer. The raw
logs are retained in `reports/v8v7-source-exact-20260911.log` and
`reports/v8v7-migrated-core-exact-20260911.log`; scores naturally vary with
machine load because each fixture uses a one-second wall-clock window.
