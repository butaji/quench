# Linux Splay screen of Task 61's revised 40-byte layout

Date: 2026-10-11 UTC  
Measurement lane: Task 94 (`v2-cloud`)  
Candidate ref: `origin/v2-splay-single-space-40b-candidate` at `9824fc2842c58d5ac4c408c1d2a49ab8d3fd06c9`  
Exact parent/control: `4a678f3967443b931e6eb7a50225a132cc5ab122`  
Rust: `1.99.0 (b940084d7 2026-09-28)`

The candidate source diff is retained as `task61-candidate.patch` (SHA-256 `6d16656e8a1ebe7ff22f977116536a3c1f873eb10bc1a5ea1118e6d2fbadd767`). It changes Task 61 heap-record and array paths; no candidate source was applied to or retained on `v2-cloud`.

## Host and tools

Linux x86_64, kernel `6.18.44`, 4 logical CPUs, CPU quota `400000/100000` (4 CPUs), and 16 GiB cgroup memory limit. Production executables were built from the exact parent and candidate worktrees with Rust 1.99. The stock harness used Node v26.10.0 `--jitless`, Bun 1.4.2 with JIT disabled, and QuickJS 2026-06-04. The fixed-work runner binary SHA-256 is `f0daee1becc22ee85ecbce48ca60c29291496b4f893b980581a25c762238be9d`; its report `source_revision` is the `v2-cloud` harness checkout `e99016d38f486bcb82423c16fc6add0b0318dc73`. Engine binary hashes identify the actual measured revisions:

- Parent/control `quench-node`: `f90e00537dfb65b1af63ea17d85c6ba4dcf03a22120c2a33bdfe63321b25ee38`
- Candidate `quench-node`: `963f5689d5da4eb7f362621ea76fcff4729c8a876008076866a01edd0f5c3c43`

The host has no usable `perf` counters. All fixed-work reports have null cycles/instructions and `qualification_ready: false`; each fixture has 0/11 clean samples. Treat these fixed-work results as correctness and RSS diagnostics, not a timing gate.

## Splay results

Both stock reports are valid and output-equal in all 11 rounds. The stock-harness win requires Quench Score strictly above Node and Quench max RSS below Bun.

| 11-round stock median | Parent | Candidate |
| --- | ---: | ---: |
| Quench Score | 2,409 | 2,316 |
| Node `--jitless` Score | 5,501 | 5,036 |
| Quench / Node Score | 0.438 | 0.460 |
| Quench max RSS | 120,287,232 B | 95,301,632 B |
| Bun no-JIT max RSS | 60,522,496 B | 60,334,080 B |
| Quench / Bun RSS | 1.99× | 1.58× |

These stock campaigns ran sequentially and show host score drift; their cross-campaign Quench Score change is not a paired speed result. Within the candidate campaign, Quench remains well below Node. The candidate lowers Quench max RSS by 24,985,600 B (about 20.8%) against its parent, but its work max RSS remains 34,967,552 B above Bun. Setup-only fixed-work RSS was 53,915,648 B, below Bun's stock RSS; the full Splay work peak is the limiting measurement.

The paired Splay fixed-work report used 300 `run()` iterations. It was valid and output-equal 11/11: parent work/setup-only median max RSS was 120,123,392 / 66,416,640 B; candidate was 95,096,832 / 53,915,648 B. The report has no qualified wall/cycle/instruction comparison.

## Requested array-path guard and Node oracle

The Crypto and NavierStokes fixed-work guards were valid and output-equal 11/11. Their work RSS medians were:

| Fixture | Parent | Candidate | Difference |
| --- | ---: | ---: | ---: |
| Crypto | 19,910,656 B | 19,759,104 B | −151,552 B |
| NavierStokes | 19,705,856 B | 19,607,552 B | −98,304 B |

There is no qualified timing result for either guard, so these samples do not establish that the candidate passes the speed regression gate.

I also ran the candidate's expanded array-store oracle with Node v26.10.0 `--jitless` and the Linux candidate binary. Their outputs match byte-for-byte; both output hashes are `c20ec3581a4413cd22a6c5a4e152c34137941ed5d8818b3955b7b3e77d8ab9f2`. The oracle script and both outputs are included.

## Decision and next gate

The revised record layout is a promising Splay RSS reduction, but this Linux screen does **not** win Splay: Score is below Node and max RSS is above Bun. Keep it as Task 61's candidate, not as a retained Task 94 code change. The Crypto/NavierStokes Linux checks are output guards only; the revised candidate still needs its clean M4 Splay and all-eight performance gates. Task 94 remains In Progress with Splay active. The next collector screen can measure whether returning empty high-index slots after full sweep lowers the candidate's 95.3 MB work peak without losing its 53.9 MB setup footprint.

## Raw evidence and hashes

- `splay-fixed-11.json`: `9c2ae66ed0f4036348ea38dee500014dc290d3fdf8ac88732957da2846b8c8cb`
- `splay-stock-baseline-11.json`: `96846b8a868bb4c8e3da34fbaa74f2ceede354092ca0d1f0db19cf35a0061263`
- `splay-stock-candidate-11.json`: `f8b3f2b9a96f1282e0679a1bf2117e9f6d3d816109b622770f20d4908dd903ed`
- `crypto-guard-11.json`: `31a12039cb91eadf544397beb162c96a6bf6a0eabd3d2c42e47b919771a800e1`
- `navier-stokes-guard-11.json`: `fe77b4ecd612943d797d3d385b6dbf1cd2b8a11171eb18f59561fd1b80666789`
- Both Node/Quench oracle outputs: `c20ec3581a4413cd22a6c5a4e152c34137941ed5d8818b3955b7b3e77d8ab9f2`
