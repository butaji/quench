# Wrapped indexed-address experiment

This experiment lowers the final Wasm `i32` effective address before widening
it for a repeated 4 GiB memory mapping. It preserves modulo-2^32 address
calculation and lets Cranelift reuse formed addresses for stores. The test
also covers low-address/wrapped-address coherence, carried out-of-bounds
addresses, access-width traps, and nonzero memarg carry traps.

The candidate is source commit `ee75becfe3ee027210b8db044a625b9355c7ec77`
with a clean tree. The pre-change production binary SHA-256 is
`cf909de333e83d970fce804bb0815baca25c482b4d94f6d5c79940b21b5fc0ef`; the
candidate binary SHA-256 is
`643efffb015441155756f6e263f985ebb43058edc2f2dc4769f8f317dc14f2ed`. Both
measured the same pinned JetStream 3.0 checkout (`06785cf861ac44855f168cbbe829278c2802e6de`), with 11 valid rounds each, JIT enabled, Linux x86_64, rustc 1.99.1, Bun 1.4.3, and Node 26.12.0. The selected GCC and QuickSort outputs validated and matched across engines.

| Fixture | Quench before → candidate Score | Score ratio candidate / before (95% bootstrap CI) | Candidate RSS | Bun Score / RSS | Node Score / RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| gcc-loops-wasm | 58.47 → 57.83 | 0.989 [0.891, 1.007] | 48.62 MiB | 177.12 / 51.30 MiB | 185.68 / 77.25 MiB |
| quicksort-wasm | 163.89 → 232.81 | 1.421 [1.280, 1.561] | 43.66 MiB | 359.09 / 44.06 MiB | 408.66 / 73.99 MiB |

For QuickSort, the median Quench/Bun Score ratio improved 1.344× [1.201,
1.536] between the two run windows. Candidate Quench remains below Bun and
Node on Score (ratios 0.640 [0.624, 0.695] and 0.561 [0.510, 0.662]). Its
RSS is lower than both, including a small 1% edge over Bun. GCC shows no
candidate Score or RSS gain and remains far below both peers on Score. This
does not meet the requested Score win.

The before and candidate runs are separate 11-round windows rather than a
randomized per-round A/B. `comparison.json` independently bootstraps each
revision's rounds and also compares per-round Quench/Bun and Quench/Node score
ratios to reduce host drift; treat the interval as evidence of a strong
QuickSort improvement, not a fully paired causal estimate.

An actual native-code dump for Quicksort function 1 shrank from 573 to 476
bytes. The candidate computes wrapped addresses with a 32-bit `lea`, then
reuses the result for the swap stores. It did not turn indexed loads into
scale-4 SIB loads, so this is a compact-code/store-reuse observation rather
than proof of faster indexed-load latency. The paired peer measurements
show a large end-to-end QuickSort gain, while GCC and RSS remain unchanged.

Raw reports and analyses:

- `baseline-report.json`, `baseline-analysis.json`
- `candidate-report.json`, `candidate-analysis.json`
- `comparison.json` (100,000-resample unpaired bootstrap)

The runtime suite passed 714 tests and the production build succeeded at this
candidate revision.
