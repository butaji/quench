# TOD-149: await states as a record family

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB, no cgroup CPU quota; rustc 1.99.1.
Release builds; hashes in `binaries.sha256`. Base `c148` = v2-gc a3492724 code (TOD-148),
candidate `c149` = v2-gc-tod149 db74913b. Row formats as in the TOD-148 evidence.

| Test | Score, paired | Peak RSS |
| -- | -- | -- |
| doxbee-async | ×1.082, 4/5 | 58.7 → 54.3 MB |
| async-fs | ×1.015, 3/5 | 21.7 → 21.5 MB |
| doxbee-promise | ×1.029, 3/5 | 65.8 → 65.9 MB |
| V8-v7 (no awaits) | geomean ×1.004 | +0.0 to +0.1 MB |
| npm top 300 | — | geomean ×1.005, median +0.1 MB, none >5% worse |

The npm and V8-v7 offsets are file-backed code pages: RssAnon / RssFile (KiB) on semver
20600/5080 → 20612/5232, debug 17140/4716 → 17136/4872, typescript-eslint 192712/5352 →
192784/5500 (repeat runs within ±600 KiB anon). The same offset moves between unrelated builds
(TOD-147 evidence). All npm packages pass on both except fs-extra, graceful-fs and the musl
rollup addon (fail on both). Runtime lib 744/744 (minor verifier on).
