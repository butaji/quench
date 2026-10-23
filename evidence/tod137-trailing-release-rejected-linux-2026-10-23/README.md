# Trailing first-call release from a helper thread — rejected

Host: Linux 6.18 x86_64 VM, 4 logical CPUs. Plain production `quench-node`, MemoryFirst,
compile cache warm, evicted binaries, 7 alternating runs. Candidate branch:
`cand-gc-trailing-release`.

## Why it was tried

On the retrained bolted binary (`richards-timeline-qb7.txt`), Richards' file pages reached
1956 KiB at 15 ms and stayed there until the first collection, at 317 ms. Its working set
after that collection is 972 KiB.

The 5 ms first-call interval releases at the start of each burst of first calls and skips the
rest. Nothing else releases before a collection, and Richards allocates too little to collect
early. With timed releases at 20–40 ms after the entry starts (measurement binary), Richards'
max RSS fell 250–400 KiB.

## Change

When the interval holds back a first-call release, a helper thread runs that release at the
interval's end, unless a release has run since. Releasing drops only read-only file pages, so
running it from another thread is safe.

## Result (`ab-7-runs.txt`)

The release works as intended: Richards' file pages fell to 1312 KiB at 23 ms on the plain build
(`richards-timeline-trailing.txt`). But each run's max RSS then came from a later peak,
at about 2 s, with anonymous memory at 1984 KiB and file pages at 1696 KiB.

Max RSS change, KiB:

| Fixture | No JIT | `--jit` |
| -- | --: | --: |
| Richards | −48 | −12 |
| DeltaBlue | +104 | +104 |
| RayTrace | +300 | −16 |
| Others | −36 to +96 | −36 to +96 |

Not kept.

## Follow-up the timelines point to

Anonymous memory grows by about 470 KiB over Richards' run while its live data stays the same:

| Component | Change |
| -- | -- |
| Old record spaces | 144 → 264 KiB |
| Property arena | doubles from a 260 to a 516 KiB mapping; 172 → 288 KiB resident |
| Malloc | about +130 KiB |

All three wait for a full collection. Under MemoryFirst that runs only once the old generation
has doubled (`old_growth` 1/1). The next step is to sweep that pacing.
