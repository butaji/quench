# Richards field-cache hit/miss diagnosis (M4 arm64 macOS)

This attribution checks whether the compact-record candidate's Richards regression comes from extra field-cache misses. It does: the baseline and candidate perform the same 25,256,800 counted lookups, but 4,046,463 immediate-prototype hits become misses in the candidate.

## Counter comparison

The baseline profile build (runtime revision `a03e0646ff216a192d85c6d1d22307855cdb21f2`) records 25,256,606 hits and 194 misses for K=100−K=0. The pre-fix compact candidate's recorded census was 21,210,143 hits and 4,046,657 misses. Its depth-0 hit count is unchanged; its 4,046,463 depth-1 holder hits disappeared. After the fix, the candidate counter build exactly matches the baseline totals and depths, with equal Richards output.

The source cause is the compact-value branch in `get_field_cached`: `heap.get(value)` returns `None` for a compact-space handle while `object_data(value)` succeeds. That branch returned `FieldCacheRead::Miss` without checking either the own-shape cache or `cached_holder_field_value`. The fix performs `shape_property_lookup` and the same own/holder cache probes used by the legacy-cell branch. See the candidate source in the compact worktree at `crates/quench-runtime/src/vm/object.rs` around `get_field_cached`.

The profiling builds needed two measurement-only repairs because the current optional `profile-aggregate` feature has stale GC-output fields and a missing `program` argument in the regional counter call. These changes were removed from the candidate source before the production build; no production behavior depends on them.

## Production gate

The compact candidate with the holder-cache fix passes Richards output validation. The current detector produced 10 clean pairs out of 11. On clean pairs, marginal instructions per run remain +8.022% (range +8.013% to +8.039%) and marginal cycles +7.587% (range +5.704% to +8.765%) versus the pinned trunk binary. Work max RSS improves from 20,119,552 B to 19,513,344 B median. This fails the all-fixture no-regression gate on Richards, so the compact candidate is not promoted; no eight-fixture campaign is warranted for this candidate state.

Splay's trunk distances remain 1.24x speed and 2.09x RSS: no runtime change was merged. The production gate report, raw profile-counter report, and summary JSON are kept here. The earlier pre-fix counter report remains in [the compact-object profile evidence](../task61-splay-compact-object-profile-2026-10-10/richards-field-cache-profile-counter-census-2026-10-10.json).

## Reproduction

- Profile counters: fixed Richards source at K=100 and K=0, `profile-aggregate` enabled; subtract setup-only counters. Raw report: [richards-cache-fix-profile.json](richards-cache-fix-profile.json).
- Production gate: `quench-bench ... richards.js --fixed-work --runs 11 --quench <pinned-trunk> --quench-peer <compact-candidate>`. Raw report: [richards-production-11.json](richards-production-11.json).
- Pinned binaries and hashes are recorded in [summary.json](summary.json).
