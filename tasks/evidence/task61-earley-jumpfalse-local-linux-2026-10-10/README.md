# Rejected EarleyBoyer `JumpFalse → LoadLocalPlain` fusion

The scoped EarleyBoyer profile ranked `JumpFalse → LoadLocalPlain` at
17,629,574 adjacent executions. I added a size-preserving fused opcode: on the
fallthrough path it evaluates the branch and loads the proven plain local in
one dispatch, while leaving the original local-load instruction as a fallback
for any branch that enters its PC. The fused destination is modeled as
read/write because the taken branch preserves its incoming value, and liveness
uses the actual `pc + 2` fallthrough for the skip form.

The aggregate-profile EarleyBoyer run executed `JumpFalseLoadLocalSkip`
32,371,058 times (741,329,935 dispatches total). Node v24.19.0, the pinned
baseline, and candidate agreed on the focused truthy/falsy, string, null,
undefined, and object cases. The V8 team's [Ignition interpreter article](https://v8.dev/blog/ignition-interpreter)
describes inline bytecode optimization as replacing common patterns and
reducing unnecessary register transfers. That motivates measuring this pair;
it does not predict a Quench win, and the production results reject it.

Three alternating production pairs used the exact `5a3397e86` baseline
(binary SHA-256
`e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32`) and a
candidate built from `v2-cloud` parent `fe0869b35` (binary SHA-256
`c3c776a03216bf8809156798e32d6b527f46b1f702af64ad14363d2803f9a805`). The
materialized EarleyBoyer fixture SHA-256 is
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

| Pair | Baseline Score | Candidate Score | Score delta | Baseline RSS | Candidate RSS | RSS delta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 421 | 410 | -11 | 41,893,888 | 41,791,488 | -102,400 |
| 2 | 427 | 407 | -20 | 41,779,200 | 41,795,584 | +16,384 |
| 3 | 425 | 408 | -17 | 41,725,952 | 41,791,488 | +65,536 |

Score fell in all three pairs. Median Score delta was -17 points and paired
median maximum-RSS delta was +16,384 bytes. Output matched in every pair, but
this is a clear performance regression, so I removed the candidate without an
extended run.

`jumpfalse-loadlocal-screen-3.json` and `.jsonl` record the paired results and
hashes. `raw/` contains Node, baseline, candidate, and benchmark stdout/stderr.
The aggregate-profile census is preserved alongside them. `candidate.patch`
and `jumpfalse-loadlocal-node-oracle.js` preserve the implementation and
focused oracle.
