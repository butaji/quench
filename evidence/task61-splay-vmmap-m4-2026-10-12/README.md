# Splay M4 VM-map snapshot

This is an uninstrumented OS-level snapshot of the pinned record-candidate process after `Setup()` and after exactly 300 `run()` calls. It adds allocator-region detail to the M4 side of TOD-102; it does not complete the structural profile-memory census or change the candidate gate.

## Reproduction identity

- Host: Apple M4 arm64, macOS 26.5 (25F71), 10 logical CPUs.
- Candidate source: `f9e7123e96d36fd894a7d947fcba3864d1f9ca7f`.
- Pinned `quench-node` SHA-256: `afde285b7f8dff56da12f44eb915034b0599d7aec95a2ff39d55ae18a8d7c4cf`.
- Inputs: base.js SHA-256 `ac383759c3cbf5f3a499ade526a4e296f7925733d8c3889b6d5840ce6867ff51`; splay.js SHA-256 `1f875cec579d03af40d256ceaeb6823f3ef16c883652650ca51d183472a8185d`.
- The runner materializes base.js + splay.js + a wrapper that calls benchmark `Setup()`, invokes `run()` K times (K=0 or 300), prints a marker, then spins. The parent stops the process at the marker and captures `vmmap`, `vmmap -summary`, and `ps` before killing it.
- Re-run from the record candidate worktree with `python3 evidence/task61-splay-vmmap-m4-2026-10-12/capture_vmmap.py --binary /Users/admin/Code/GitHub/quench-v2/target/pinned/afde285b7f8dff56da12f44eb915034b0599d7aec95a2ff39d55ae18a8d7c4cf/quench-node --base <base.js> --fixture <splay.js> --output-dir evidence/task61-splay-vmmap-m4-2026-10-12`.

## Observed memory by process stage

| Metric | Setup, K=0 | After 300 runs | Change |
| -- | --: | --: | --: |
| `ps` RSS at stop | 49,088 KiB (47.94 MiB) | 85,584 KiB (83.58 MiB) | +36,496 KiB (+35.64 MiB) |
| `vmmap` physical footprint (and reported peak) | 38.7 MiB | 74.3 MiB | +35.6 MiB |
| Default malloc-zone resident | 37.5 MiB | 73.2 MiB | +35.7 MiB |
| MALLOC_LARGE resident | 15.8 MiB | 33.1 MiB | +17.3 MiB |
| 512 MiB ObjectRecordSpace-size range | 512 MiB virtual (128 MiB mapped + 384 MiB reserved), 15.8 MiB resident | 512 MiB virtual (128 MiB mapped + 384 MiB reserved), 31.5 MiB resident | +15.7 MiB resident |
| Additional MALLOC_LARGE mapping | — | 2,160 KiB virtual, 1,728 KiB resident | Appears after work |
| MALLOC_SMALL resident | 21.2 MiB | 39.4 MiB | +18.2 MiB |
| MALLOC_TINY resident | 224 KiB | 208 KiB | −16 KiB |

The record space reserves exactly 16,777,216 × 32 B = 512 MiB virtually (`OBJECT_RECORD_SPACE_CAPACITY` and `size_of::<Object>() == 32`). The single source allocation matches the unique contiguous 512 MiB MALLOC_LARGE range in both vmmap snapshots; that range is split into a 128 MiB mapped subrange and a 384 MiB reserved subrange. The mapped segment’s resident size grows 15.8 → 31.5 MiB (+15.7 MiB). A separate 2,160 KiB large mapping appears after work, with 1,728 KiB resident. vmmap does not label the Rust owner, so the record-range association is strongly supported by the unique size match but remains an OS-map correlation.

MALLOC_SMALL residency grows another 18.2 MiB. This identifies where pages become resident, but not how much is live array data versus allocator metadata, fragmentation or other allocations. The exact structural answer still requires a profile-memory census on the same candidate and stage.

## Limits and qualification status

`ps` RSS is a point-in-time value; `vmmap` physical-footprint peak covers only the lifetime of each stopped process. The K=300 snapshot occurs after the final run and before teardown, so it can miss an intra-run peak. This custom input and these OS metrics are not comparable to the stock-harness qualification RSS. Official M4 Splay distances remain trunk 1.664× Score and 1.810× max RSS (snapshot 2026-10-11); the current record candidate remains unmerged and has not beaten either bar. The latest candidate stock diagnostic is recorded on TOD-8.

Raw outputs and hashes are in this directory; `summary.json` has the machine-readable values.
