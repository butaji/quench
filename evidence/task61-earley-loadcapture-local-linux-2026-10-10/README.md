# Rejected EarleyBoyer `LoadCapture → LoadLocalPlain` fusion

The first fusion pass runs before plain-local specialization and could not see
this pair. A targeted late pass after numeric-local rewrites made the pattern
visible without rerunning unrelated fusion recipes. The aggregate-profile
candidate executed `LoadCaptureLocal` 1,350,445 times; the protected-entry skip
variant did not execute. The focused Node v24.19.0 oracle agreed with both the
pinned baseline and candidate on nested and mutated captures, null, negative
zero, numbers, and strings.

Three alternating production pairs used the same-path `5a3397e86` baseline
(binary SHA-256
`e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32`) and the
candidate built from `v2-cloud` parent `94d7665717e0031b2b92f764753a1d399dfb20b8`
(binary SHA-256
`e4fb2e4309b184ad52857bbdd938f86ff67dc272907df91a0f8e22056a64f0b3`). The
materialized EarleyBoyer fixture SHA-256 is
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

| Pair | Baseline Score | Candidate Score | Score delta | Baseline RSS | Candidate RSS | RSS delta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 430 | 422 | -8 | 41,725,952 | 41,787,392 | +61,440 |
| 2 | 400 | 414 | +14 | 41,697,280 | 41,717,760 | +20,480 |
| 3 | 424 | 415 | -9 | 41,832,448 | 41,652,224 | -180,224 |

Median Score delta was -8 points. Score fell in two of three pairs. Median
maximum-RSS delta was +20,480 bytes, with candidate RSS higher in two pairs.
Outputs matched in all pairs. This screen does not establish a 95% interval;
the negative Score direction and lack of an RSS reduction reject the candidate
without an 11-pair extension. I removed the source change and continued
profiling EarleyBoyer.

`screen-3.json`, `pairs-3.jsonl`, and `raw/` preserve the measured rows and
stdout/stderr. `candidate.patch` records the removed implementation; the
profile output, materialized input, Node oracle, and build provenance are also
included. The binary files remain in the ignored `work/task61-v2-merge-earley/`
directory and are identified by the hashes above.

I fetched `v2` at `7dfe2e1f1` and reviewed its rewrite/runtime changes. It
removes several separate local and branch fusions after their own negative
measurements, but it contains no result for this capture/local pair. I did not
copy those unrelated changes into `v2-cloud`; this candidate was rejected on
its own paired EarleyBoyer evidence.
