# TOD-148: promises as a record family

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB, no cgroup CPU quota; rustc 1.99.1.
Release builds of `quench-node`; hashes in `binaries.sha256`.

- base `b148` = `v2-gc` 62ac2c7f (TOD-147 published)
- candidate `c148` = `v2-gc-tod148` 9e980576

`summary.txt` is `pairsum.py` over `async-pairs-5.txt` (rows: `round test binary
max_rss_kb wall_s exit score`) and `v8-pairs-3.txt` (`round binary bench score
max_rss_mb`).

| Test | Score, paired | Peak RSS |
| -- | -- | -- |
| doxbee-async | ×1.224, 5/5 | 64.9 → 58.8 MB |
| doxbee-promise | ×1.142, 4/5 | 71.3 → 65.9 MB |
| async-fs | ×1.031, 4/5 | 22.1 → 21.8 MB |
| V8-v7 (no promises) | geomean ×0.997 | −0.1 MB on each |
| npm top 300 | — | geomean ×0.9965, none >5% worse |

npm: every package passes on both except fs-extra, graceful-fs and the musl rollup addon,
which fail on both. Runtime lib 739/739 (minor verifier on).

profile-gc-time + profile-aggregate on the candidate (one run each): doxbee-promise slot
sweep 0.64 s (5.53 s before TOD-147). Remaining legacy allocations by variant
(`quench-allocation-variants`): async-fs 13.3M await states, 7.8M iterators; doxbee-async
9.6M await states.
