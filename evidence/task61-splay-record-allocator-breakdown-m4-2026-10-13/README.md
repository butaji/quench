# Splay record-space allocator memory profile (M4)

This profile isolates memory held by the active Splay candidate's object and array record spaces. It is not a timing or qualification run.

- Platform: M4 arm64 macOS.
- Source: `2a9d02b103c378e09b6232c1db474f385f2a9b63` (profile-only breakdown fields).
- Profile binary SHA-256: `45e81c6f4e3f9df59d4c1af7cfc9752daa2a603c2861a8a0d6d34ca136963321`.
- Input SHA-256: `ac9c33765fb2968c78c1f116563913f575451d84826ea8a7bdf79a791bb3f345`.
- Work: setup, 300 `run()` calls, teardown; checkpoint `gc_16_after`.
- Profile options: `QUENCH_MEMORY=1 QUENCH_MEMORY_PEAK=1`; production profile plus `profile-memory`.

At the checkpoint there were 512,272 live ObjectRecords and 256,048 live ArrayRecords. Record-space `free: Vec<u32>` held 757,692 IDs in 786,432 allocated entries, or 3,145,728 bytes of capacity (3 MiB). This is a direct measurement of allocator capacity, not a claim that every byte contributes to max RSS. The bitmap already stores occupancy, so the free vector duplicates availability membership; replacing it with a bitmap search is the next Splay-only candidate, but its changed allocation order and cycles must be measured.

The records themselves and block metadata account for 33,062,912 bytes at this checkpoint. Full selected counters are in `summary.json`; raw profile stderr is gzip-compressed in `profile.stderr.gz`. The instrumented process reached 244,891,648 bytes max RSS, which is intentionally excluded from the production RSS comparison.
