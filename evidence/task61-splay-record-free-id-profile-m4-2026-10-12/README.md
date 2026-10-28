# Splay record free-ID capacity profile (M4)

This is a profile-memory attribution run for the v2-splay-array-record-next candidate at source 7513a05067acb790f133214837f2a27afd3f4aa8. It ran the existing 300-run materialized Splay input (SHA-256 ac9c33765fb2968c78c1f116563913f575451d84826ea8a7bdf79a791bb3f345) with QUENCH_MEMORY=1 and QUENCH_MEMORY_PEAK=1. The pinned diagnostic executable is target/pinned/d0925ac62d9e8a88f6e7f654e7988bc7912472878cad1ee3779207fcc0931e22/quench-node-profile-memory (SHA-256 d0925ac62d9e8a88f6e7f654e7988bc7912472878cad1ee3779207fcc0931e22).

At the two late GC checkpoints, the record free-ID vectors reserve 2,097,152 B for objects and 1,048,576 B for arrays, 3,145,728 B combined (3 MiB). The capacities are unchanged between GC 15 and GC 16; the same values appear in the completion snapshot.

| Snapshot | Occupied slots | Object free-ID capacity | Array free-ID capacity | Accounted runtime bytes |
| --- | ---: | ---: | ---: | ---: |
| gc_15_before | 1,563,848 | 2,097,152 B | 524,288 B | 65,175,263 B |
| gc_15_after | 781,735 | 2,097,152 B | 1,048,576 B | 63,817,719 B |
| gc_16_before | 1,563,561 | 2,097,152 B | 1,048,576 B | 65,887,961 B |
| gc_16_after | 781,977 | 2,097,152 B | 1,048,576 B | 63,844,095 B |
| complete | 1,328,066 | 2,097,152 B | 1,048,576 B | 65,369,760 B |

The shared host had Deno test workers during the run. These counters are used only to evaluate allocated record metadata. Profile instrumentation added 147,348,608 B at completion; the profile process RSS and elapsed time are not qualification data. Raw output is preserved in profile.stdout and profile.stderr; hashes and selected fields are in summary.json.
