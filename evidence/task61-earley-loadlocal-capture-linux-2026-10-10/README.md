# Rejected EarleyBoyer `LoadLocalPlain → LoadCapture` fusion

The scoped EarleyBoyer profile counted 17,844,019 adjacent
`LoadLocalPlain → LoadCapture` dispatches. A late rewrite pass after plain
local specialization fused the pair while preserving both result writes and
their order. Protected suffix entries retain a standalone `LoadCapture`
fallback. The focused `var` binding probe executed the fused opcode four times;
Node v24.19.0, the pinned baseline, and candidate produced identical output.

Three alternating production pairs used the exact `5a3397e86` baseline
(binary SHA-256
`e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32`) and a
candidate built from `v2-cloud` parent `7a3d85808` (binary SHA-256
`58c9ce67f325386a500e65481c80130356968efe863c8da3e062a449766230ea`). The
materialized EarleyBoyer fixture SHA-256 is
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

| Pair | Baseline Score | Candidate Score | Score delta | Baseline RSS | Candidate RSS | RSS delta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 420 | 424 | +4 | 41,787,392 | 41,652,224 | -135,168 |
| 2 | 426 | 422 | -4 | 41,590,784 | 41,709,568 | +118,784 |
| 3 | 427 | 413 | -14 | 41,766,912 | 41,783,296 | +16,384 |

The paired median Score delta was -4 points. The paired median maximum-RSS
delta was +16,384 bytes. Candidate output matched in all pairs, but Score fell
in two pairs and RSS rose in two. This is not a joint win; the fusion was
removed without an extended run.

`loadlocal-capture-screen-3.json` and `.jsonl` record the rows, hashes, and
oracle result. `raw/` contains the Node, baseline, candidate, and benchmark
stdout/stderr. `candidate.patch` preserves the rejected implementation, and
`loadlocal-capture-node-oracle.js` preserves the focused semantic probe.
