# Splay speed-lane head on M4 — `b72da593e`

Candidate source `b72da593ef2b878292834e696cb13527cd287034` was fast-forwarded into M4 trunk at the same commit after the gates below. Its tested tree is therefore the current M4 trunk tree. The pinned production binary is `target/pinned/6e14719d1a33cb3d1c968f0bb8d92a4feb7a52a253545035bcec32d0f7ba5f31/quench-node` (SHA-256 `6e14719d1a33cb3d1c968f0bb8d92a4feb7a52a253545035bcec32d0f7ba5f31`). Measurements below are Apple M4 arm64 macOS unless stated otherwise.

## Correctness

- `cargo test -p quench-runtime --lib`: 544/544 passed.
- Test262 stages 59–63: 4,944/4,944 passed; raw report: [test262-stages-59-63.json](test262-stages-59-63.json).
- Full shared Wasm suite: 67,124/67,124 passed at pinned corpus revision `67c8b9dc32c47d61c05a1681df349d01c85fa8ff`; raw report: [wasm-full.json](wasm-full.json).
- Stage 10: 11,087/11,102 passed; 15 known compound-assignment/postfix-decrement failures remain, matching the pre-existing baseline failures recorded for this repository. Raw report: [test262-stage-10.json](test262-stage-10.json).
- Stage 26 was stopped after 22/94 batches to avoid prolonging host load before the requested stock timing window; this partial run is not a pass. Stages 31, 50, 51 and 72 were not run.

## Fixed-work performance gate

The all-eight fixed-work guard has 11/11 valid, clean pairs per fixture. The candidate is faster on all eight by median cycles and lower in instructions on seven; RSS is flat within the established per-fixture A/A noise policy. The raw uniform +0.5% RSS rule initially flagged RegExp, so a same-binary A/A calibration was collected before deciding it: RegExp candidate A/A median RSS movement was +0.683% (95% interval 0 to +2.145%); candidate-vs-baseline RegExp A/B was +0.862% (95% interval −0.597% to +1.081%). The A/B interval includes zero and is inside the measured A/A band. The raw fixed-budget decision is preserved unchanged; this A/A-calibrated interpretation is in [regexp-rss-aa-summary.json](regexp-rss-aa-summary.json).

| Fixture | Median cycles | Median instructions | Median max RSS |
|---|---:|---:|---:|
| Crypto | −10.481% | −7.195% | −0.075% |
| DeltaBlue | −6.182% | −6.499% | −0.157% |
| EarleyBoyer | −6.661% | −7.940% | −0.264% |
| NavierStokes | −1.467% | −6.234% | +0.078% |
| RayTrace | −4.715% | −4.292% | −1.166% |
| RegExp | −0.202% | −0.130% | +0.862% (within A/A band) |
| Richards | −17.574% | −16.190% | −0.245% |
| Splay | **−11.279%** | **−13.549%** | **−0.084%** |

Raw reports: [all-eight-fixed-work-11.json](all-eight-fixed-work-11.json), [resumable-guard.json](resumable-guard.json), [candidate RegExp A/A](regexp-candidate-aa-11.json), and [RegExp A/A summary](regexp-rss-aa-summary.json).

## Stock-harness diagnostic is invalid for distance

[splay-stock-11.json](splay-stock-11.json) was collected during heavy host contention (load roughly 27–34 with Deno workers active). Its stock scores and engine ratios are retained only as contaminated raw data and must not be used for qualification, distance updates, Linear, or the platform scoreboard. A detector-clean 11-round stock Splay campaign is still required.

## Clean stock Splay retake

[splay-stock-clean-11.json](splay-stock-clean-11.json) is a candidate-only, 11-round stock-harness Splay diagnostic on Apple M4 arm64 macOS. All 11 rounds were valid and output-equal. The raw runner recorded 88 before/after host snapshots; every one-minute load snapshot was below the established limit of 1.0 per logical CPU (maximum 3.15 load on 10 logical CPUs, or 0.315 per CPU). There were no Deno/Node worker bursts in those snapshots. The candidate's stock medians were:

| Engine | Median Score | Median max RSS |
|---|---:|---:|
| Quench candidate | 7,043 | 111.41 MiB |
| Node v26.10.0 `--jitless` | 14,897 | 386.63 MiB |
| Bun 1.4.2, JIT disabled | 9,948 | 59.03 MiB |
| QuickJS 2026-06-04 | 8,622 | 144.06 MiB |

Thus the best stock Score bar is Node and the lowest stock RSS bar is Bun. Current M4 trunk distance is **2.12× speed** and **1.89× RSS**. This one-fixture report is diagnostic, not an all-eight qualification report; Splay remains open.
