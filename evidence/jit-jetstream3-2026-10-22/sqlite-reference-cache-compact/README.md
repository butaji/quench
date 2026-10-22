# Wasm indirect-call cache compaction screen

The experiment removed the body pointer from each default `FastReferenceCache`
entry and moved it to an optional per-site array used only by
`QUENCH_WASM_JIT_REFERENCE_BODY_CACHE=1`. The default cache entry shrank by one
word (8 bytes); generated default dispatch was otherwise intended to remain
unchanged. `cargo check --locked -p quench-runtime` passed. Static review found
no pointer-layout or ownership issue in the optional path.

The two production executables were screened on the JetStream 3
`sqlite3-wasm` fixture with three rounds each. Both reports passed output
validation and reported equal outputs.

| Arm | Executable SHA-256 | Quench median score | Quench median max RSS | Bun score / RSS | Node score / RSS |
| --- | --- | ---: | ---: | ---: | ---: |
| Baseline | `2a7d34b581952756eddc79533a4ff6df11ab76a15c8fa1a7e130f5941a66fe7d` | 0.43 | 170,295,296 B | 46.83 / 105,439,232 B | 41.20 / 76,427,264 B |
| Compacted | `13e9d5bd9a21f0126aba3ffa9d4ba21b576a03dc5066a411142fffdcc98d391f` | 0.50 | 170,586,112 B | 32.13 / 99,864,576 B | 43.98 / 73,273,344 B |

The candidate used 290,816 more bytes of peak RSS (0.17%). The score and peer
scores varied substantially across the two windows, so the score difference is
not treated as a causal speed change. The RSS result does not establish a
reduction; the 8-byte-per-site saving was below useful peak-RSS resolution for
this fixture. This patch was dropped rather than retaining extra optional
pointer plumbing without a measured memory benefit.

Raw reports: `baseline-3round.json` and `candidate-3round.json`.
