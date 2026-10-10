# V2 merge on Linux EarleyBoyer

The V2 merge was compared against the pre-merge v2-cloud binary at a matched
Thin-LTO production profile. Both binaries used the same 2026-10-10 Linux host,
runner, and materialized fixture. Across eleven alternating pairs, median
Score rose from 209 to 378 (+168; paired-bootstrap 95% interval +161 to +171).
Median maximum RSS fell from 51,486,720 to 42,770,432 bytes (-8,749,056;
95% interval -8,925,184 to -8,404,992); RSS was lower in all eleven pairs.
All paired outputs matched.

This large one-fixture improvement still does not win EarleyBoyer against the
reference engines. A current one-round diagnostic measured Quench at Score
233 / 40,632,320 bytes; QuickJS 2,355 / 16,003,072 bytes; Bun 5,281 /
36,212,736 bytes; and Node `--jitless` 6,912 / 161,640,448 bytes. Output
matched across all four. This is a gap diagnostic, not an 11-round reference
qualification.

A separate `profile-aggregate` / `profile-memory` build measured the hot path
on the same input. The physical census counted 747,368,668 dispatches:
`LoadLocalPlain` 24.9%, `StoreLocalPlain` 10.6%, `LoadConst` 8.6%,
`JumpFalse` 8.4%, `GetField` 8.4%, `Move` 7.5%, `Binary` 7.1%, and
`LoadCapture` 6.9%. Aggregate counters recorded 12,059,499 allocations, 826
collections, 141,970 peak live cells, and 70,984 maximum threshold cells. The
largest pair counts include 54.9M `StoreLocalPlain → LoadLocalPlain` and
54.0M `LoadLocalPlain → GetField` executions. Profiling binaries and runs are
not performance measurements.

The aggregate profile previously aborted when the shared VM's counters
included other programs than the final program being disassembled. The
profile-only reporter now omits the mismatched per-program matrices and
continues printing the aggregate profile. The census, profile, memory snapshot,
four-engine report, paired report, and Node oracle are preserved here.
