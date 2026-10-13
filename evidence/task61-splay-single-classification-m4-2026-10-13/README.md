# Splay object-record access attribution (M4)

Host: Apple M4, macOS arm64. This work attributes the all-eight guard regression while Splay remains the active V8-v7 target. It does not change the official stock-harness distance or qualify a candidate.

## Current Splay distances

The latest clean stock result is still trunk `17077748a`: 1.64× Score distance and 1.81× max-RSS distance. The isolated record candidate has no clean stock qualification result. Its trunk-relative schema-5 fixed-work report is `all-eight-trunk-vs-candidate-11.json` (SHA-256 `1e2370e379c6a872c2303d8396b7336b716b1cd1b8bc3eaf323a1c499e568763`): Splay cycles −7.994%, instructions −5.683%, and work max RSS −37.504%, with 11/11 clean pairs. The same report rejects the candidate on Richards cycles (+2.249%, 11/11 clean) and instructions (+1.299%); RegExp instructions rise +0.807%.

## Setup-subtracted counters

The profile inputs are materialized CJS forms of the pinned `base.js` plus Richards or RegExp and the fixed-work runner. K=0 and work inputs have matching fixture content across binaries. `run_profile_inputs.py` retained raw stdout/stderr; `compare_aggregate_counters.py` subtracts setup counters and divides by K. The resulting report is `setup-subtracted-counters.json` (SHA-256 `fc5158c30131e68ab95b7821b475aa477cda7781b38ec238b34fadf46af50808`).

| M4 profile | Trunk | Record candidate | Finding |
| -- | --: | --: | -- |
| Richards field-cache hits/run | 252,566.06 | 252,566.05 | Same work |
| Richards field-cache misses/run | 1.94 | 1.95 | Same work within counter rounding |
| Richards indexed gets/run | 4,252 | 4,252 | Same work |
| Richards indexed sets/run | 928 | 928 | Same work |
| Richards allocations/run | 33 | 33 | Same work |
| Richards collections/run | 0.02 | 0.02 | Same work |
| RegExp GetIndex/run | 92,090 | 92,090 | Same work |
| RegExp allocations/run | 335,000 | 335,000 | Same work |
| RegExp collections/run | 30 | 30 | Same work |

The guest opcode totals also match for both fixtures. Therefore the measured instruction increase is not caused by executing extra guest operations or allocations; it remains in per-access implementation/codegen or runtime helper work. GC nanosecond counters vary substantially between these separately run profile binaries and are not treated as an attribution result.

Profile binaries: trunk source `17077748a`, SHA-256 `836f46f72e94a55df8c9c3e56d46474cf210e2b7915dae0c31ecad9b83bdedd9`; candidate source `9034436c`, SHA-256 `211cba431565ec4d3a684d50ec4b2e56a3dccca773f294ad07370552037541b0`. Aggregate report hashes: trunk `55c522999ddde2ea7ec4f5e9c8df08cf878956de611aa5039a0301ff3e1c923e`, candidate `056c3292b79b9c9cebee9d3d0c378eae65de7acd572e30e6481088a174379909`.

## Controlled Richards probes

`richards-array-record-disabled-3.json` (SHA-256 `30b08aa8bbb8078a33613af04405d2597041b64643768d3a317a2548f7243081`) uses production binary `7aa06b7f51394e0c710e0bb583f3b02355c972d637b74758d8166ef434c044d6`, which keeps ObjectRecords but routes Array allocation back to legacy `Cell`. It produced valid, output-equal runs but 0/3 clean timing pairs due an external Node process during the campaign. Retired instructions are still useful for attribution: setup-subtracted Richards marginal instructions were +1.365% against trunk (three rounds), close to the full candidate’s +1.299% across 11 clean rounds. This rules out the ArrayRecord allocation/layout alone; the probe still uses the generic `Heap::array` decoder, so it does not rule out accessor decode cost.

`richards-object-record-disabled-3.json` (SHA-256 `3119f7daf58c10a46d2a07cb15079cbc455208fd2612c1b2f25fda40162cf190`) is excluded: routing object allocation back to legacy `Cell` makes the fixture fail with `value is not callable`, so it is not a valid performance comparison.

## Direct-object classification candidate

Source base `9034436cce8e9a5892d21a082823c5627bf5ff11`; production binary SHA-256 `578c7f1c1e3b1fedb6ea47413d5ad275413d655880bd5fddaf556fc50a2a0bf4`; source diff SHA-256 `b1ed45a59c7680a14da4eea0d0801717d900a033ee348f3f2d9fa259cf0bb180`. The direct ObjectRecord tag is recognized before the general three-kind `HeapReference` decode on cached field hits. The direct-object and direct-array index helpers compare their named masks directly, preserving the generic fallback for other Values.

On Splay versus the immediate parent binary `14c20b22603fdd69c07910feb762d5ee2e6964d1933b19d6dcc739d5f2782db5`, the 11/11-clean fixed-work incremental result is wall/run −1.373% [−4.076%, −0.163%] and instructions −0.146% [−0.158%, −0.128%]. Against the current M4 trunk binary `8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd`, the complete all-eight fixed-work report is 11/11 clean per fixture with equal output. Splay improves marginal wall/run −8.979% [−10.053%, −7.745%], cycles −8.911%, instructions −5.825%, and work max RSS −37.546% [−37.567%, −37.467%].

The written WP6 guard passes after subtracting each fixture's same-binary A/A upper-95 allowance. The largest non-Splay adjusted wall losses are Crypto +0.094% and NavierStokes +0.047%; the other fixtures have no adjusted wall loss, and every work max-RSS median is lower. RegExp's raw wall median is +2.186%, below its A/A upper endpoint (+4.977%). Derived values are reproducible with `analyze_gate.py`; `all-eight-analysis-direct-object.json` records report hashes, exact per-fixture bootstrap intervals, and the decision. Raw paired reports are `all-eight-direct-object-classification-vs-trunk-11.json`, `splay-direct-object-classification-11.json`, and copied same-binary control `all-eight-aa-schema5-11.json`.

Correctness passed on this candidate: runtime 556 unit and 6 integration tests; production `cargo check --locked -p quench-node` with repository warning denial; Test262 stages 31–32 (3,302/3,302; report SHA-256 `8cf57968e7c2c1eb0a267ff0b741ceddad2d74afde1074865d6278a501d34472`) and 60–63 (4,604/4,604; `e07c164749eddbcf86c71ce54c489deb474209bb9b7dfc35327616ce92976c2e`); full Wasm suite (67,124/67,124; inventory complete). The WAST JSON is gzip-compressed at `wasm-full-current.json.gz` (compressed artifact SHA-256 `8bc7ba12ff64adfc29ca6e3e4425b3685bf36b609ba191351f67fdb0b36daa3d`; decompress to recover the report SHA-256 `acbdd5414a78909be982c68d149eeb5fb8107bf0885d0d4398c86d2792ef63af`).

## Stock Splay diagnostic

The candidate's first 11-round stock retake is not accepted as a distance. The report is `stock-splay-direct-object-candidate-11.json` (SHA-256 `b9bcc914b712e068d686cf3fa4ab50a16c7b98d728a0d54d87ce799bf523f728`): all engine outputs match and all rounds are valid, but Deno workers enter the host snapshots starting in round 6, with Bun and Deno workers overlapping rounds 9–10. The contaminated all-round medians are excluded. For context only, the first six rounds without Deno/Bun in the recorded process snapshots have median Quench Score/RSS 9,892 / 72,933,376 B, Node `--jitless` Score 14,957.5, and Bun no-JIT Score/RSS 10,014.5 / 63,750,144 B. The partial subset implies 1.512× Score distance and 1.144× RSS distance; it is not a decision or qualification result. A full clean 11-round Splay retake remains pending.

The candidate remains isolated. The Linear scoreboard still records the last clean M4 trunk distance, 1.648× Score and 1.781× RSS. A clean 11-round stock Splay retake on this candidate and its Linux own-host row are still required; no other V8-v7 fixture has been started.
