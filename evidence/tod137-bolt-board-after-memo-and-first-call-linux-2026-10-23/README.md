# V8-v7 on bolted static binaries after the lazy memo tables and the first-call release

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. MemoryFirst, compile cache warm, each
binary evicted before every run.

| Binary | Source | Build |
| -- | -- | -- |
| `qb5` | v2-gc `a391e63280` | The earlier board's binary |
| `qb6` | v2-gc `3d7dc0656a` | `mkbolt.sh` with the same `bolt2` profile and function order, trained on code that predates today's changes |

## Same-procedure A/B, `qb5` → `qb6` (7 alternating runs, `ab-qb5-qb6-7-runs.txt`)

Max RSS change, KiB:

| Fixture | No JIT | `--jit` |
| -- | --: | --: |
| NavierStokes | −440 | −128 |
| RegExp | −384 | −80 |
| RayTrace | −320 | +148 |
| Splay | −188 | −160 |
| DeltaBlue | −176 | +148 |
| Crypto | −108 | −124 |
| Richards | +320 | +88 |
| EarleyBoyer | +88 | +112 |

Scores were equal or higher on every row.

## Why some rows rose: stale layout, not memory management

Richards peaks, sampled from `/proc`:

| Binary | Anonymous | File (code) |
| -- | --: | --: |
| `qb5` | 1588–1592 KiB | 1788–1808 KiB |
| `qb6` | 1464–1468 KiB | 2036–2176 KiB |

The heap side fell by about 120 KiB. Code pages rose by about 350 KiB because the BOLT
profile and function order describe the old code. Retraining the order and the profile on
current code is the RSS lane's TOD-171.

Re-bolting the same static build with `-reorder-functions=cdsort` (`qb6c`) was mixed:
Richards −252 KiB, Crypto `--jit` +232 KiB.

## The 3-run board (`board-3-runs.txt`)

The board script stopped before writing its JSON (EEXIST on a stale file), but its text
holds every sample. Quench's worst no-JIT samples were 3828 KiB (Richards) and 5228 KiB
(NavierStokes); the same fixtures' medians were 3492 and 4396. Three runs are too few to
read the worst sample.
