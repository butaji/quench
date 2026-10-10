# EarleyBoyer large-heap GC headroom 3/4 screen

I changed the general large-heap collector threshold from one live set of
allocation headroom to three quarters of the live set. Three alternating
production pairs on the pinned EarleyBoyer fixture preserved benchmark output.

| Pair | Score baseline → candidate | Max RSS baseline → candidate |
| --- | ---: | ---: |
| 1 | 421 → 420 | 41,816,064 → 39,862,272 bytes |
| 2 | 424 → 420 | 41,721,856 → 39,940,096 bytes |
| 3 | 427 → 422 | 41,721,856 → 39,493,632 bytes |

The candidate lowered RSS in every pair (median delta −1,781,760 bytes) but
lowered Score in every pair (median delta −4). This fails the joint Score/RSS
criterion, so I restored the 1/1 production policy. The three-pair sample is
directional only; I did not extend it to 11 pairs. `pairs.jsonl` records the
raw ordered runs and output, and `candidate.patch` records the rejected
threshold change. Baseline binary SHA-256: `e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32`;
candidate binary SHA-256: `0d38f5bdeec7337a3e15e4a97a1d486fe8e8b502e7899979a4cf75be745bc317`.
