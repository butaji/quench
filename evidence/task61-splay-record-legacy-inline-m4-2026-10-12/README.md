# Splay record candidate: direct legacy Cell slots (M4)

Candidate source commit: `e7a5d9584447c553c4c295b2348036f4d669b0af`, based on rejected record-space candidate `63fc6629bc81c7bc44b2938ae2bc553e3395a016`.

This slice retains direct ObjectRecord Values and returns every legacy Cell payload to its original SlotArena slot. It removes the `SlotEntry` plus `RecordArena<Cell>` indirection. The same slot-owned Cell is used by accessors, marking, weak pruning, sweep, property compaction, and memory profiling.

Correctness checks on Apple M4 arm64 macOS:

- `cargo check -p quench-runtime`: passed.
- `cargo check -p quench-runtime --features profile-memory,profile-aggregate,profile-gc-time`: passed.
- `cargo test -p quench-runtime`: 551 unit tests and 6 integration tests passed.
- Full pinned Wasm testsuite, submodule `67c8b9dc32c47d61c05a1681df349d01c85fa8ff`: 67,124/67,124 passed; inventory complete. Raw report: [`wasm-shared.json`](wasm-shared.json), SHA-256 `6bae5d9f85b94f879ddffdd796e3721a3efbc24e24ec395235b2a77f96ef6c18`.

## Splay fixed-work screen (Apple M4 arm64 macOS)

The schema-5 Splay A/A and A/B ran for 300 `run()` calls per process against the pinned current-trunk production binary. Raw reports:

- [`splay-aa-schema5-11.json`](splay-aa-schema5-11.json): trunk binary against itself, 11/11 clean pairs; work max RSS median 116,375,552 B and setup-only 65,454,080 B.
- [`splay-ab-schema5-11.json`](splay-ab-schema5-11.json): trunk SHA-256 `8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd` versus candidate SHA-256 `e86118d57ad11cfe4d3c2fe919037b51ded3da1af70198d05071d985aae84b7d`; output equal, 10/11 clean pairs. The one excluded pair failed the detector's effective-cycle-rate check (ratio 0.178 versus minimum 0.9).

Using only clean pairs, paired marginal wall time changed −5.425% (95% bootstrap interval −6.506% to −4.353%), cycles −5.522% (−6.175% to −4.939%), and retired instructions −3.819% (−3.837% to −3.802%). Median work max RSS fell from 116,375,552 B to 87,703,552 B (−24.63%); setup-only max RSS fell from 65,437,696 B to 50,429,952 B (−22.93%).

The official M4 distances remain the last trunk values: 1.664× Score and 1.810× max RSS.

## Stock-harness Splay diagnostic (Apple M4 arm64 macOS)

The candidate production binary completed 11/11 valid rounds with equal output against the same-campaign references. Scores: Quench 9,599; Node v26.10.0 `--jitless` 14,872; Bun 1.4.2 no-JIT 9,917; QuickJS 2026-06-04 8,622. RSS: Quench 87,883,776 B; Bun 63,422,464 B (lowest reference); Node 404,619,264 B; QuickJS 151,060,480 B. Candidate diagnostic distances are **1.5493× Score / 1.3857× max RSS**; Splay is still unwon on both bars. Raw report: [`candidate-stock-11.json`](candidate-stock-11.json), SHA-256 `2e623341d23bf618d8c62d47edf4b52638879f248939a490eb266fe81405d0f0`.

This single-fixture report has `qualification_ready=false` because the runner's qualification inventory expects the full fixture set; its Splay observations are diagnostic and do not replace the official trunk row. The preflight was one-minute load 3.68 with no Deno worker; the 5/15-minute averages were still elevated, and the stock runner has no per-round contention detector. Re-take stock Splay after integration on a fully settled host before updating official distances.

## Post-sync stock Splay diagnostic (Apple M4 arm64 macOS)

After merging `origin/v2-merged` at `40b5d420c`, candidate source `e08b4e0cabe756610606264b5db53cfaa5c481c9` was built as production binary SHA-256 `ead2ff81fa08797eee190dcafed5d31f825bf193c4f4a3328dc3af4150f22343`. The 11-round Splay report is [`post-merge-stock-11.json`](post-merge-stock-11.json), SHA-256 `541f21505711890a91bd88c7e5bf10ae87c6e27c226183302151e22b3d765cb7`.

All 11 rounds were valid and output-equal. Medians: Quench Score 8,839 / max RSS 126,205,952 B; Node v26.10.0 `--jitless` Score 14,929 / RSS 407,322,624 B; Bun 1.4.2 no-JIT Score 10,007 / RSS 63,373,312 B; QuickJS 2026-06-04 Score 8,671 / RSS 151,060,480 B. The same-campaign diagnostic ratios are 1.689× Score (Node bar) and 1.991× RSS (Bun bar). This is not a candidate decision: the pre-sync candidate `e7a5d9584` is not a matched baseline for the merged tree, and the report marks `qualification_ready=false` because the launcher checkout was dirty and this was a one-fixture run. Its recorded 10-core host snapshots had one-minute load 2.44–3.03 and no Deno, Bun, Node, or rustc workers in the sampled top CPU processes; 5/15-minute load remained elevated at 5.78–6.75 / 7.87–8.37. The next comparison is candidate `e08b4e0ca` against its synced parent `40b5d420c`, using matched binaries and a clean runner checkout. The official Splay scoreboard remains unchanged.

## All-eight fixed-work gate (Apple M4 arm64 macOS)

Pinned schema-5 runner SHA-256: `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`. Both all-eight campaigns completed with 11/11 clean pairs on every fixture and equal output. The A/A used the pinned trunk binary for both roles; the A/B compared that trunk binary with the candidate binary above. Raw reports: [`all-eight-aa-schema5-11.json`](all-eight-aa-schema5-11.json) (SHA-256 `963a116fccdbec9f320e12c76d73443066dfbcb58a83c0f4afee6925d50fdb26`) and [`all-eight-ab-schema5-11.json`](all-eight-ab-schema5-11.json) (SHA-256 `d49b18da91c7a67d731ccc8084f89a07aa3d454da5a9c90daddc9b1ec9ccd84b`). The schema-5 guard binary SHA-256 `65c7d338250c899622b4fb5851964f7262602fbb09c5e6759f88196b1952a0c1` wrote [`the checkpoint`](all-eight-ab-guard.json) and [`decision`](all-eight-ab-guard.decision.json); the tool's overall result is **rejected** because its cycle rule classifies Crypto as a regression.

The paired wall intervals below use 20,000 percentile-bootstrap replicates and the same fixed seed as the Splay screen. RSS is the paired median change in work max RSS. A/A wall intervals show the measured same-binary band.

| Fixture | A/B wall, % (95% interval) | A/A wall band, % | A/B cycles, % (95% interval) | A/B instructions, % | A/B work max RSS, % |
| -- | --: | --: | --: | --: | --: |
| Crypto | +1.167 [+0.377, +1.574] | [−0.842, +0.980] | +1.683 [+0.296, +1.976] | −0.734 | −1.057 |
| DeltaBlue | −6.700 [−6.859, −6.082] | [−0.296, +0.921] | −6.662 [−6.826, −5.740] | −7.420 | −3.946 |
| EarleyBoyer | −1.330 [−1.587, −0.819] | [−0.476, +0.148] | −1.314 [−1.613, −1.069] | −2.094 | −9.613 |
| NavierStokes | −0.959 [−1.561, −0.053] | [−0.221, +0.663] | −0.843 [−1.331, −0.052] | −0.607 | −1.241 |
| RayTrace | +0.346 [−0.608, +1.243] | [−0.641, +0.151] | +0.406 [−0.153, +1.169] | −0.132 | −3.882 |
| RegExp | +0.663 [+0.043, +1.470] | [−1.391, +4.977] | +0.412 [−0.229, +2.324] | +0.785 | −1.342 |
| Richards | −0.499 [−1.242, −0.135] | [−1.415, +0.532] | −0.360 [−0.899, +0.139] | −0.000 | −2.941 |
| Splay | −4.991 [−5.940, −4.426] | [−1.884, +0.757] | −4.795 [−5.509, −4.328] | −3.835 | −24.609 |

TOD-11's current numbered-package gate allows at most 2% wall loss on another fixture and at most 1% max-RSS loss beyond its A/A band; it also requires a `finding` for a loss above 0.5%. On those terms Crypto's +1.167% wall median is below the 2% limit, with its median only 0.187 percentage points above the A/A interval's upper end; its RSS fell 1.057%. The required Crypto finding is recorded on TOD-16. All eight fixtures' max-RSS medians decreased. The guard's cycle-based `overall: rejected` result is retained verbatim because it differs from TOD-11's wall-based numbered-package rule. The candidate stock diagnostic is now recorded above: 11/11 valid, output-equal Splay rounds, with `qualification_ready=false` because the report contains one fixture. It does not replace the official scoreboard row; a stock retake on integrated trunk is still required after an integration decision.

The runner records launcher checkout `597dd55d…` as dirty; this does not identify either measured engine. The candidate source worktree was clean at `e7a5d9584`, the fixture corpus was clean at `64e1860c`, and both executable SHA-256 values are recorded in the raw A/B report.
