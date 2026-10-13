# Splay ArrayRecord plus prototype-shape candidate — M4

This is isolated evidence for candidate source `31e0b77a2d899186e9ff08e35c3daefa077496ec` on branch `task61-splay-array-proto-m4`, not a trunk result. Runtime commits include `6186d6a51` (direct ObjectRecord and ArrayRecord values), `a44c421de` (prototype roots on shapes), and `496b8045d` (realm Object-prototype correction). Production binary SHA-256: `188a715128b64a1dfc409174069a0b5bf3695aca159c760ddc5b9f303529b94c`. The fixed-work runner SHA-256 is `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`; comparison parent is the prototype-only binary `f7efc3e788cde3467efdf80f62c528a1889ee8c2729dd254bb35d7a9cac5cbe4`.

Host: M4 arm64 macOS, Mac16,10, Darwin 25.5, 10 logical CPUs, 16 GiB; rustc 1.99.0 / LLVM 23.1.2. Corpus revision: `64e1860c736c1b899708f4cd646721bc71e53d8b`. Reports record runner source revision `597dd55d41a79584e73d8965a16d5243a784349b` with a dirty shared checkout; executable and corpus hashes are pinned in each raw report.

## Correctness

- `cargo test --locked -p quench-runtime --lib`: 556 passed, 0 failed.
- Test262 stages 31–32: 3,302 passed, 0 failed; stages 60–63: 4,604 passed, 0 failed.
- Full pinned WAST: 67,124 passed, 0 failed; inventory complete.
- `RUSTFLAGS="-D warnings" cargo check --locked -p quench-node`: passed.

## Splay fixed-work

The parent-to-candidate A/B and same-binary A/A each have 11/11 clean paired rounds (300 `run()` calls per work sample), with equal output. Metrics below use per-round K=0-subtracted marginal cost per run; RSS is work max RSS.

| Metric | Parent | Candidate | Delta | Paired bootstrap 95% interval |
| -- | --: | --: | --: | --: |
| Marginal wall/run | 845,148 ns | 832,917 ns | -1.252% | [-1.901%, -0.762%] |
| Marginal cycles/run | 3,391,072 cycles | 3,328,782 cycles | -1.462% | [-1.946%, -0.618%] |
| Marginal instructions/run | 19,735,236 instructions | 19,532,836 instructions | -1.022% | [-1.031%, -1.006%] |
| Work max RSS | 75,431,936 B | 72,810,496 B | -3.475% | [-3.624%, -3.264%] |

The fixed-work Splay pair is a resolved improvement: wall -1.252% and work max RSS -3.475%. The raw work RSS medians are 75,431,936 B → 72,810,496 B. This does not establish the stock-harness win.

## All-eight guard

A/A has 11/11 clean pairs on every fixture. A/B rows use 11 clean pairs per fixture, merging the clean Crypto/DeltaBlue/EarleyBoyer/NavierStokes/RayTrace rows from the all-eight run with dedicated 11-round RegExp and Richards top-ups and the separate 11-round Splay pair. Outputs were equal. The 2% wall and 1% RSS caps apply after subtracting each fixture’s A/A upper 95% allowance.

| Fixture | A/B wall | A/B instructions | A/B work RSS | A/A-adjusted wall/RSS loss |
| -- | --: | --: | --: | --: |
| crypto | +1.166% [+0.519, +1.470] | -0.744% | -0.227% [-0.452, +0.000] | 0.724% / 0.000% |
| deltablue | +2.412% [+1.638, +3.333] | +1.859% | -1.381% [-1.468, -0.983] | 1.943% / 0.000% |
| earley-boyer | +1.145% [+0.227, +1.727] | +0.801% | -0.252% [-0.352, +0.051] | 0.239% / 0.000% |
| navier-stokes | -1.596% [-2.125, -0.883] | -1.070% | +0.470% [+0.392, +1.099] | 0.000% / 0.156% |
| raytrace | -0.693% [-2.684, +1.248] | -1.772% | +0.161% [+0.000, +0.481] | 0.000% / 0.080% |
| regexp | +0.678% [-0.626, +1.409] | +0.689% | -0.661% [-0.926, -0.088] | 0.000% / 0.000% |
| richards | +3.021% [+2.302, +3.425] | +1.555% | -0.591% [-0.923, -0.337] | 2.303% / 0.000% |
| splay | -1.252% [-1.901, -0.762] | -1.022% | -3.475% [-3.624, -3.264] | 0.000% / 0.000% |

**M4 regression guard fails on Richards.** Wall is +3.021% [+2.302, +3.425], with instructions +1.555% and cycles +3.003%. After its A/A upper-95 allowance, wall loss is 2.303%, above the 2% cap. The intervals and instruction increase make this a measured guard failure, not just a Deno-contended stock sample. The candidate cannot be integrated under the current cross-fixture policy.

## Stock Splay diagnostic

The 11-round four-engine stock report completed validly with equal outputs but is not a clean distance result: host snapshots show Deno workers, including a Deno process at 153% CPU during round 0 while Quench ran. Keep it out of the scoreboard and retake in a quiet window.

Raw medians: Quench Score 9516, max RSS 73,089,024 B; best reference Score 14937 (node), lowest reference RSS 63,635,456 B (bun). The contaminated raw ratios are 1.570× Score and 1.149× RSS; `qualification_ready` is false. They are diagnostics only, not valid current distances.

## Decision

Keep this candidate isolated. Its Splay fixed-work speed and RSS both improve, but Richards exceeds the M4 guard and the stock campaign was contaminated. The Splay win remains open on M4 and Linux; no other V8-v7 benchmark was started. Raw reports, hashes, and the derivation script are in this directory.

Raw reports: `splay-aa-11.json`, `splay-ab-11.json`, `all-eight-aa-11.json`, `all-eight-ab-11.json`, `regexp-ab-topup-11.json`, `richards-ab-topup-11.json`, `stock-splay-11.json`, `test262-31-32.json`, `test262-60-63.json`, `wasm-full.json`. Derived table: `all-eight-analysis.json`; machine summary: `summary.json`.
