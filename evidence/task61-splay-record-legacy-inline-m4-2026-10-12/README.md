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

## Matched fixed-work Splay comparison on the synced base (Apple M4 arm64 macOS)

The comparison parent is `40b5d420c591ba53c34252ea88dccad141635e93`; candidate is `e08b4e0cabe756610606264b5db53cfaa5c481c9`. The parent does not build unchanged on this M4: warning denial catches two Linux-only dead items, and its Node credential syscalls pass `u32`/`usize` to macOS `c_int` APIs. The detached comparison worktree applied only two `#[allow(dead_code)]` attributes and the two macOS argument casts; the exact patch is [`synced-parent-m4-build.patch`](synced-parent-m4-build.patch), SHA-256 `95f55fa827136cc5fa9045d9f9dbea30a0f128f57af6aee5ea5aaab10d5b1214`. The candidate binary SHA-256 is `ead2ff81fa08797eee190dcafed5d31f825bf193c4f4a3328dc3af4150f22343`; the patched synced-parent binary is `dd63c09a142648041daab393fcab66d5f2e4a85d9347a23d0c3830504c93cffe`. Both use the M4 production profile, rustc 1.99.0 / LLVM 23.1.2, 64-byte function alignment, and interpreter mode.

The pinned schema-5 runner SHA-256 is `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`; its source checkout is clean at `597dd55d…` and the corpus submodule is clean at `64e1860c…`. Work samples each execute the pinned Splay plan of 300 `run()` calls; the report contains 11 paired rounds and 11/11 clean pairs with equal output. Raw reports: same-binary parent A/A [`synced-parent-splay-aa-fixed-work-11.json`](synced-parent-splay-aa-fixed-work-11.json), SHA-256 `5ad59b05d3217f2c6068cedc10937ca63d0d2018c963076e0d85d800d97cba72`; parent-versus-candidate A/B [`synced-parent-vs-candidate-splay-fixed-work-11.json`](synced-parent-vs-candidate-splay-fixed-work-11.json), SHA-256 `7b299c438ffc687f705022493e56c1e2a13836a49569fb6ee08addee27cdd89b`.

Using the schema-5 guard's paired marginal formula and deterministic 20,000-replicate bootstrap (the guard CLI itself requires the full eight-fixture set and therefore will not create a decision from these Splay-only reports), candidate changes versus the synced parent are: marginal wall/run −11.312% (95% interval −11.996% to −10.420%), cycles/run −20.427% (−20.949% to −19.934%), instructions/run −15.793% (−15.833% to −15.761%), and work max RSS −20.662% (−20.786% to −20.613%). The paired medians are 1.0111 → 0.8967 ms, 4.6525 → 3.7000 M cycles, 24.3643 → 20.5165 M instructions, and 156,106,752 → 123,846,656 B work max RSS. Setup-only max RSS is 94,846,976 → 78,217,216 B.

The A/A reports an order-associated shift despite flat retired instructions: marginal wall −2.943% (95% interval −7.840% to −1.168%), cycles −2.183% (−4.746% to −0.194%), instructions −0.042% (−0.255% to +0.018%), and work max RSS −0.032% (−0.854% to +0.010%). The candidate's instruction reduction is far outside this A/A band; the wall/cycle result is recorded with that A/A behavior visible.

A clean-runner stock Splay attempt is retained as rejected evidence, [`rejected-candidate-stock-deno-burst-11.json`](rejected-candidate-stock-deno-burst-11.json), SHA-256 `785be80b925efb359603696d55dd64dadb0532be8b5c953d1de082eebedeb179`. It was valid and output-equal, but Deno workers appeared in the host snapshots during rounds 0 and 2–4 (up to 229.7% CPU); discard its Score/RSS medians. The report also has `qualification_ready=false` because this was Splay-only. No stock distance or scoreboard update is claimed from this attempt.

This fixed-work result is a positive Splay mechanism result against the synced parent, not a fixture qualification or a trunk decision. Run the required all-eight fixed-work guard on this base and get a worker-free stock Splay retake before updating the scoreboard.

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

## Synced-parent all-eight fixed-work guard (Apple M4 arm64 macOS)

The schema-5 runner was pinned at SHA-256 `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`; the V8-v7 corpus was clean at `64e1860c736c1b899708f4cd646721bc71e53d8b`. The baseline is synced parent `40b5d420c591ba53c34252ea88dccad141635e93`, production binary SHA-256 `dd63c09a142648041daab393fcab66d5f2e4a85d9347a23d0c3830504c93cffe`. The candidate is `e08b4e0cabe756610606264b5db53cfaa5c481c9`, production binary SHA-256 `ead2ff81fa08797eee190dcafed5d31f825bf193c4f4a3328dc3af4150f22343`. The runner source was clean at `597dd55d41a79584e73d8965a16d5243a784349b`; the report records interpreter mode, equal output, and 11/11 clean paired rounds on every fixture.

Raw report [`synced-parent-vs-candidate-all-eight-fixed-work-11.json`](synced-parent-vs-candidate-all-eight-fixed-work-11.json), SHA-256 `fc790d4a62bf9611a4e9a2316f25bb464590423e8fd83ca2cec360af8166fcf9`. Guard state [`synced-parent-vs-candidate-all-eight.guard.json`](synced-parent-vs-candidate-all-eight.guard.json), SHA-256 `b94bd7bd1f641fb8a7abb4d97335a0987127ce86e5578c1c1265e15652648926`; decision [`synced-parent-vs-candidate-all-eight.guard.decision.json`](synced-parent-vs-candidate-all-eight.guard.decision.json), SHA-256 `814e4e1503df3d93717a75aaec44e7ab449f56a77ca2dc4223881c0053f70297`. The fixed-work guard's overall decision is **rejected**.

The paired wall and cycle percentages below use the guard's per-round K/K=0 marginal formula and its deterministic 20,000-replicate bootstrap (95% percentile interval); instructions and RSS are paired medians. All fixtures have 11/11 clean pairs and equal output.

| Fixture | Marginal wall/run, % (95% interval) | Cycles/run, % (95% interval) | Instructions/run, % | Work max RSS, % |
| -- | --: | --: | --: | --: |
| Crypto | −0.086 [−0.681, +0.585] | +0.498 [−0.351, +0.722] | −1.275 | −0.577 |
| DeltaBlue | −4.572 [−5.224, −4.270] | −4.611 [−5.025, −4.051] | −7.628 | −1.275 |
| EarleyBoyer | −0.407 [−0.823, +0.276] | −3.751 [−4.623, −3.595] | −1.329 | −4.666 |
| NavierStokes | −5.395 [−6.382, −4.628] | −4.976 [−6.505, −4.513] | −3.019 | −0.348 |
| RayTrace | +8.850 [+7.482, +12.221] | +8.612 [+8.164, +10.660] | +8.155 | +1.342 |
| RegExp | +3.211 [+1.127, +4.844] | +3.309 [+1.153, +4.667] | +1.726 | −0.456 |
| Richards | −0.700 [−1.232, −0.003] | −0.556 [−0.904, −0.048] | −1.280 | +0.288 |
| Splay | −11.803 [−12.634, −10.469] | −21.047 [−22.559, −20.706] | −15.872 | −20.744 |

The candidate's Splay fixed-work improvement is clear against the synced parent, but RayTrace regresses beyond the guard on wall, cycles and instructions, and its work max RSS rises 1.342% against the 0.5% budget. RegExp also has a clean +3.211% wall / +3.309% cycles regression. This layout candidate is therefore not eligible to merge. The official M4 stock Splay distances remain **1.664× Score / 1.810× max RSS**; no stock result or scoreboard update follows from this fixed-work guard.

An earlier attempt reached all fixtures but failed to write its report because the output path was relative to the clean runner checkout. It produced no retained raw data and is excluded; the report above is the successful retry with an absolute output path.

## Counter-only attribution of RayTrace and RegExp (Apple M4 arm64 macOS)

To locate the two regressions, I ran one schema-5 `profile-aggregate` fixed-work round per fixture, each with its setup-only control and output comparison. The run counts come from the raw report and runner source: RayTrace executes 10 `run()` calls; RegExp executes 1. The earlier note calling these captures “K=300” was incorrect. Both reports have 0/1 clean timing rounds, so their wall/cycle/RSS fields are excluded from decisions; only runtime counters are used here. The parent and pre-fix candidate profile binaries are SHA-256 `83cac26ff6e4a20a606d5e5a6e9524d32353fe6eafb624ea1f11bb26c7e90ea5` and `9b30259ad5216caa168302471dc7eea7d04c0f3d1eb226f58bde2ce081817840`. They were built from parent `40b5d420c` and pre-fix candidate `e08b4e0ca`, respectively, with the measurement-only compatibility patch [`profile-aggregate-build-compatibility.patch`](profile-aggregate-build-compatibility.patch), SHA-256 `b7554ef8d15ff034d9d6aca6691684c7747cfcb233ea1c081a130a288aa60c06`. The patch only supplies `Default` for the feature-gated 35-entry mark-counter array and updates the feature-gated Wasm profiler call signature. Runner SHA-256 is `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`; corpus revision is `64e1860c736c1b899708f4cd646721bc71e53d8b`.

For RayTrace, work minus setup-only field-cache counters are:

| Counter | Synced parent | Candidate |
| -- | --: | --: |
| Hits | 12,024,787 | 11,294,151 |
| Misses | 12,323 | 742,959 |
| Tier 0 / tier 2 hits | 11,108,370 / 916,417 | 11,108,370 / 185,781 |
| Depth 1 hits | 1,865,269 | 1,134,633 |

The candidate lost exactly 730,636 tier-2 holder hits and gained exactly 730,636 misses. Allocations (1,332,950), indexed gets (115,230), indexed dispatches, call counts, and output were equal. The source explains the counter delta: in `vm/object.rs`, `get_field_polymorphic`'s direct-record branch at lines 696–700 tried `cached_field_value` and then `cached_ordinary_holder_field_value`, which checks only the monomorphic immediate-holder entry. It then called `get_field_miss`. The equivalent fallback in `get_field_uncached` used `cached_holder_field_value`, which also probes the megamorphic holder table. Thus direct-record receivers bypassed an existing cache tier.

For RegExp, work minus setup-only cache counters match exactly: 123,706 hits, one miss, 123,706 tier-0 hits, and depth counts `[38,781, 84,925, 0]`; indexed gets are 92,090 and allocations are 335,000 in both builds. This rules out cache-miss inflation as the explanation for RegExp. Its instrumented marginal retired-instruction count is 10,478,445,360 on the parent and 10,645,924,111 on the candidate (+1.599%); this is attribution only, not a qualified performance measurement. The remaining RegExp cost needs separate source or instruction attribution.

Raw reports: [`raytrace-profile-aggregate-parent-vs-candidate-1.json`](raytrace-profile-aggregate-parent-vs-candidate-1.json), SHA-256 `869f9bc2bb36942c0aa1c36b37591b9de43b71efecaa46394a73d8ccdfbeeaa1`; and [`regexp-profile-aggregate-parent-vs-candidate-1.json`](regexp-profile-aggregate-parent-vs-candidate-1.json), SHA-256 `3252d2a0b855f43a9308ed06eb960fdef50c7af408e870b24fc5d2cd158e0be5`.

After the fix in candidate commit `24d9f77f20b2f1dc81c6b461b7face09e71a7c71`, I repeated the RayTrace counter capture against the same parent. The corrected candidate profile binary SHA-256 is `19c8e00199d9bd3d173c20b5abed4c5fccf8967f8651a48ec864ece890d82738`; its only source-tree difference from the commit is the same measurement-only compatibility patch above. The capture is valid and output-equal, with 0/1 clean timing rounds. Parent and fixed candidate now match exactly: 12,024,787 field-cache hits, 12,323 misses, tiers `[11,108,370, 0, 916,417]`, depths `[10,159,518, 1,865,269, 0]`, 1,332,950 allocations, and 115,230 indexed gets. This confirms the cache-hit loss is removed; the capture supplies no timing verdict. Raw report [`raytrace-profile-aggregate-parent-vs-fixed-candidate-1.json`](raytrace-profile-aggregate-parent-vs-fixed-candidate-1.json), SHA-256 `557332fcc0954e9c915f69164c8e0aa03a1477da3d93b2049dce0288bad08931`.

Correctness after the fix: `cargo test -p quench-runtime --lib --locked` passes 571/571. With `profile-aggregate`, both direct-object holder tests pass when the Rust test thread uses `RUST_MIN_STACK=16777216`; the new test asserts a tier-2 hit, and the old-path control fails that assertion. The full profile-enabled library suite is 574/575 because the existing module-evaluation test trips the aggregate dispatch-matrix assertion (11 physical executions vs 13 opcode counts); the normal suite passes.
