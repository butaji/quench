# Splay RSS mapping capture at runtime source 4062a6be9 (M4)

This is a process-memory attribution for the active V8-v7 fixture, Splay. It is not a timing result, a max-RSS qualification run, or a change to runtime code.

## Source and method

The pinned production binary is `098cc7aa69195bab87115b496a4df9660f78c5d7364e60a52c8761f2e857f5e6` (SHA-256). Its stock diagnostic identifies runtime source `4062a6be9`. This capture was made from local evidence head `ebcf80c4b`; its runtime subtree matches `4062a6be9`. A subsequent fetch found `origin/v2-merged` at `a70849cf6`, which contains newer runtime changes. The exact remote-publication time relative to capture is unknown, so this evidence is a measurement of binary `098cc7…` only and must not be presented as the newest `v2-merged` source result. Inputs are V8-v7 `base.js` (`ac383759…`) and `splay.js` (`1f875cec…`).

`capture_vmmap_4062.py` runs the uninstrumented pinned `quench-node` in three fresh processes. Each loads the materialized base and Splay program; the phases stop before benchmark `Setup`, after `Setup` (K=0), and after exactly 300 `run()` calls. The harness prints a marker, then the parent stops the process and captures `vmmap`, `vmmap -summary`, and `ps` before teardown. Load average was 2.865/6.844/8.362 before capture and 2.795/6.764/8.324 after, on 10 logical CPUs.

The corrected final run is in [`final-capture/`](final-capture/). Its metadata includes the script, binary, input hashes, phase PIDs, and load before and after. The earlier duplicate captures were removed; this directory is the authoritative snapshot.

## Results

Values below come from `final-capture/*.process.txt` and `*.vmmap-summary.txt`. `ps` RSS is in KiB; vmmap reports its physical-footprint and region figures in MiB.

| Phase | `ps` RSS | vmmap physical footprint | `MALLOC_SMALL` resident | ObjectRecordSpace resident | ArrayRecordSpace resident |
| -- | --: | --: | --: | --: | --: |
| Before Splay `Setup` | 45,472 KiB | 32.5 MiB | 30.6 MiB | 0.05 MiB | 0.02 MiB |
| After `Setup` | 77,008 KiB | 63.3 MiB | 35.8 MiB | 15.8 MiB | 9.8 MiB |
| After 300 `run()` calls | 114,352 KiB | 99.8 MiB | 39.3 MiB | 31.6 MiB | 19.0 MiB |

From Setup to the 300-run snapshot, RSS rose 37,344 KiB and physical footprint rose 36.5 MiB. The two record mappings gained 25.0 MiB, about 68.5% of that physical-footprint increase. Their combined resident bytes were 25.6 MiB after Setup and 50.6 MiB after work. This confirms that the two record spaces are the largest measured run-phase term in this snapshot.

Source layout explains the mapping reservations: `ObjectRecordSpace` is `RecordSpace<Object>` and `ArrayRecordSpace` is `RecordSpace<ArrayRecord>`, each reserving `1 << 24` records. `Object` is 32 bytes (`proto`, `ValueVec`, and two inline `Value`s); `ArrayRecord` adds the 8-byte `Rc<Vec<Value>>` elements handle, for 40 bytes. The corresponding ranges reserve 512 MiB and 640 MiB. `vmmap` reports pages resident within those ranges, not live-record counts.

The remaining 11.5 MiB of Setup-to-work footprint growth is not assigned to one cause. In the work snapshot, the primary `MALLOC_LARGE` resident figure is 53.6 MiB, of which the record regions account for 50.6 MiB; other small and empty malloc regions also appear. Do not infer that all unexplained bytes are fragmentation. `__TEXT`'s 106.5 MiB mapped resident figure is shared mapping data and is not added to the process physical footprint.

## What this means for Splay

At this binary's stock diagnostic, Quench measured 9,167 Score vs Node `--jitless` 14,848 (**1.620× Score distance**) and 117,489,664 B vs Bun no-JIT 65,945,600 B (**1.782× RSS distance**). These are candidate diagnostics, not the official `v2` row, and are historical relative to the later `a70849cf6` source update; neither bar was won.

For memory, the next Splay candidate should reduce the per-record footprint in these measured spaces, with one shared property/array semantics path and an exact fallback for arrays with named properties. The capture alone does not distinguish live records from allocator high-water and does not prove which record layout is cheapest. A proposed width change still needs source-level invariants, correctness tests, the Splay speed/RSS gate, and the all-eight guard before retention.

## Artifacts

- [`summary.json`](summary.json): normalized values and source/input identities.
- [`capture_vmmap_4062.py`](capture_vmmap_4062.py): reproducible capture script.
- [`final-capture/metadata.json`](final-capture/metadata.json): complete phase and provenance metadata.
- `final-capture/{pre-setup,setup,work}.vmmap.txt` and `.vmmap-summary.txt`: raw OS snapshots.
- `final-capture/{pre-setup,setup,work}.process.txt`: raw `ps` snapshots.
- [`SHA256SUMS`](SHA256SUMS): checksums for this README, summary, capture script, and raw capture files.

The work snapshot is after the 300th `run()` and before `TearDown`; it can miss transient RSS peaks. These readings must not replace the stock-harness max-RSS gate.
