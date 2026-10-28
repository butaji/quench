# Field-add cache sets: first entry inline, eight prototype shapes — kept

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. Static builds, each bolted with a profile
retrained on its own code (base `qm1`: v2-gc `d771cef14d`, the trunk merge; candidate `qf1`:
`cand-gc-field-add-sets`). Binaries are evicted before each run and pairs alternate.

## Census (`profile-memory`, end of run, `field_add_set_occupancy` and `field_add_cache_depth_counts`)

| Workload | Sets | One entry | Deepest chain |
| -- | --: | --: | --: |
| Crypto | 162 | 162 | 3 |
| RayTrace | 197 | 196 | 3 |
| Richards | 86 | 86 | 3 |
| npm ESLint | 123 | 122 | 4 |
| npm TypeScript | 1426 | 1413 | 3 |

Each set nevertheless held four ways of entries carrying sixteen prototype shapes: about 392
bytes per site.

## Change

- A set keeps its first entry inline. The second to fourth live in a box made when a second
  arrives.
- The oldest-first replacement order is kept.
- Add entries carry eight prototype shapes. Node's streams need five, as the code notes; deeper
  chains stay uncached, as before past sixteen.

Field-add set bytes:

| Workload | Before | After |
| -- | --: | --: |
| Crypto | 100,352 | 20,480 |
| RayTrace | 100,352 | 20,480 |
| npm TypeScript | 802,816 | 163,840 |

Entry counts are equal.

## Result

V8-v7 under MemoryFirst (`v8v7-bolted-memory-first-7-runs.txt`): within the ±130 KiB floor
(−116 to +148 KiB), scores within noise. The expected saving there is about 50 KiB.

npm, max RSS (`npm-9-runs.txt`):

| Workload | MemoryFirst | Throughput |
| -- | --: | --: |
| TypeScript | −480 KiB | −68 KiB |
| ESLint | −356 KiB | −1344 KiB |
| Acorn | −152 KiB | −244 KiB |
| Ajv | −124 KiB | −1096 KiB |

Times are within noise.

`cargo test -p quench-runtime --lib`: 835 passed.
