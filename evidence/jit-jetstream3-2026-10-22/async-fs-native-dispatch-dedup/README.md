# Async-fs native callee lookup deduplication

`lane_native_call` previously looked up the callee heap cell to check for
`Function.prototype.call`, then looked it up again to identify the native
handler. The async-fs Float64 DataView lane executes about 16.4 million get/set
calls through this dispatch. The candidate reads the native kind once and
matches that saved value for `FunctionCall`, `DataViewGetFloat64`,
`DataViewSetFloat64`, `ArrayPush`, and `ArrayPop`. Non-native calls and failed
fast paths still exit to the generic interpreter path.

Six interleaved one-round A/B pairs were run on JetStream 3 `async-fs`; the
order alternated baseline/candidate and candidate/baseline. Every run passed
fixture validation and reported equal output. The baseline executable is the
preserved production build of `6608130`; the candidate contains only this
source change.

| Pair | Order | Baseline score | Candidate score | Score ratio | Concurrent peer geometric ratio | Quench RSS delta |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | B/C | 1.89 | 3.56 | 1.884x | 2.579x | 0 B |
| 2 | C/B | 4.26 | 2.61 | 0.613x | 2.001x | −4,096 B |
| 3 | B/C | 2.46 | 3.85 | 1.565x | 0.471x | 0 B |
| 4 | C/B | 3.56 | 3.82 | 1.073x | 0.950x | +4,096 B |
| 5 | B/C | 2.57 | 2.51 | 0.977x | 1.071x | −36,864 B |
| 6 | C/B | 3.67 | 3.76 | 1.025x | 0.919x | +4,096 B |

The median paired raw score ratio is 1.073x and the geometric mean is 1.117x.
The median RSS delta is zero. Peer scores varied sharply across windows: the
geometric mean of Quench ratios after dividing by the simultaneous Bun/Node
geometric ratio is 0.974x. This is a plausible low-risk dispatch reduction,
but the sample does not establish a causal JetStream score gain or a memory
reduction. Raw reports retain executable hashes, peer scores, timing and
output-validation evidence.

Control executable SHA-256:
`2a7d34b581952756eddc79533a4ff6df11ab76a15c8fa1a7e130f5941a66fe7d`

Candidate executable SHA-256:
`d2f343df215f335e2068efbc94e75e0e3ee20d5948ae35910abf428f6c534645`

The six pairs are `pair-1-baseline.json` through `pair-6-candidate.json`.
