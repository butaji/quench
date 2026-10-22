# TOD-164 step 2: minor record sweeps visit only young words

Host: Linux x86_64 VM, 4 logical CPUs, 15 GB. Production `quench-node`, compile cache off
(`QUENCH_COMPILE_CACHE=`).

- Base `p173b`: v2-gc `c28f33ae6`.
- Candidate `p173c`: `cand-gc-young-words`.
- `p173bg` / `p173cg`: the same trees with `quench-runtime/profile-gc-time`.

## Change

This replaces the planned lazy sweep at allocation. Record-sweep time is mostly the drop of dead
records, which lazy sweep only moves. Deferring those drops would also delay property-arena
releases, payload accounting (which drives pacing), promise job-context abandonment and
sparse-array removal.

- Every sweep frees each occupied, unmarked record it visits. So between collections, only ids
  taken since the last sweep can be occupied and unmarked.
- `RecordSpace` records two things:
  - `young_floor`: `next_id` at the last sweep.
  - The base of each free word `load_free_word` loads. This is the slow path only, so the
    allocation fast path is unchanged.
- A minor sweep visits those words alone. Live becomes the previous live count minus freed.
- Full sweeps visit every word, as before.
- A debug assertion checks after every sweep that no occupied, unmarked record remains.

## Result: kept (rule written on TOD-164 before measuring)

The rule: score up on V8-v7 or JetStream, no suite below ×0.99, max RSS geomean ≤ ×1.005 per
suite, npm top 300 ≤ ×1.01.

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 10 | ×0.997 | ×0.996 |
| JetStream (8 GC-heavy tests) | 5 | ×1.027 (lazy-collections ×1.039 5/5, acorn ×1.056) | ×0.999 |
| Frameworks | 5 | ×0.981 (fastify ×0.895 2/5, h3 ×0.950) | ×0.999 |
| Frameworks, rerun | 10 | ×0.997 | ×0.992 |
| npm top 300 | 1 | — | ×0.9991; statuses unchanged; sharp 31.1 → 32.9 MB |

- The 5-pair framework run broke the floor on sub-second runs where A/A noise is about ±2%. The
  10-pair rerun settles it at ×0.997.
- The gc-time split (`gc-time-split.txt`, 3 runs each) shows V8's record sweeps unchanged. Its
  heaps are 4–8 MB, so the old full-word walk was already cheap there. The gain is on
  JetStream's 14–58 MB heaps, where every minor collection walked every word.
- quench-runtime lib: 793/793 with the invariant assertion on.

## Current per-collection time split (base, median of 3)

| Test | Collections | Full mark | Minor mark | Sweep | Roots | Post-collection |
| -- | --: | --: | --: | --: | --: | --: |
| earley-boyer | 4297 | 512 ms | 324 ms | 162 ms | 271 ms | 65 ms |
| raytrace | 5527 | 188 ms | 120 ms | 54 ms | 113 ms | 59 ms |
| regexp | 2891 | 39 ms | 115 ms | 124 ms | 41 ms | 29 ms |

The VM root scan is now the largest fixed cost per minor collection. That is step 3.
