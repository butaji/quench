# TOD-147: functions as a record family

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB, no cgroup CPU quota; rustc 1.99.1.
Release builds of `quench-node`; hashes in `binaries.sha256`.

- base `m146` = `v2-gc` a6051239 (TOD-146 published)
- candidate `c147` = `v2-gc-tod147` df9e8d2f

## Why

profile-gc-time on a6051239: the legacy slot sweep was 5.3 s of 9.0 s collection on
doxbee-promise and 4.9 s of 11.9 s on async-fs; object records swept 34.8M dead objects in
0.11 s. profile-aggregate (doxbee-promise): 60.0M allocations, 20.4M of them functions, each a
24-byte slot plus a malloc'd 24-byte `FunctionObject` box.

## Results

| Measure | Base | Candidate |
| -- | -- | -- |
| doxbee-promise slot sweep / record sweep / mark (profile-gc-time, 1 run) | 5.53 / 0.11 / 3.33 s | 3.68 / 0.41 / 2.84 s |
| doxbee-promise Score, paired (`async-pairs-5.txt`) | — | ×1.081, 4/5 |
| doxbee-promise peak RSS | 73.8 MB | 70.7 MB |
| doxbee-async Score / RSS | — / 65.0 MB | ×1.017, 3/5 / 64.5 MB |
| async-fs Score / RSS (few closures) | — / 22.6 MB | ×0.990, 2/5 / 22.6 MB |
| V8-v7 Score geomean (`v8-pairs-3.txt`) | — | ×1.008 |
| npm top 300 geomean RSS (`npm-top300-rss.txt`) | — | ×0.9987, none >5% worse, all pass but musl rollup (both) |

V8-v7 peak RSS read 0.1–0.5 MB higher. `rss-split.txt` splits it: `RssAnon` (heap and
allocator) is equal or lower; the difference is `RssFile`, code pages, which moves by up to
0.6 MB between builds without heap changes (fin2, c146c, m146) and between runs of one
binary. profile-memory composition at completion (crypto): accounted runtime bytes
817,600 → 766,484; boxed function headers 39 KB → 0; slot arena 128 → 64 KiB.

Runtime lib 739/739 (minor verifier on). Test262 full inventory 53,353 passed, 53 waived,
0 failed on base and candidate.
