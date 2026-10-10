# EarleyBoyer large-heap GC headroom 3/4 recheck

This rechecks the earlier 3/4 large-heap allocation-headroom setting on top
of the current EarleyBoyer capture optimizations. The candidate lowers memory
use but loses Score, so the production setting remains 1/1.

## Paired result

Eleven alternating Linux production pairs used the pinned EarleyBoyer fixture
and matched output in every pair.

- Median Score: 422 baseline, 408 candidate; paired median delta −13 points
  (bootstrap 95% interval −16 to −11).
- Median maximum RSS: 41,746,432 baseline, 39,256,064 candidate bytes; paired
  median delta −2,433,024 bytes (95% interval −2,564,096 to −2,322,432).
- Candidate Score was lower in all 11 pairs; candidate RSS was lower in all
  11 pairs.

The output match and repeatable RSS reduction do not offset the repeatable
Score loss. I reverted the threshold change.

The baseline binary SHA-256 is
`c00031392bbe312d5d1261c869ee1a221de987a59e5df2d7eb8e38967365a4e7`; the
candidate SHA-256 is
`4ded213432923b1dc2707a3d9a4860cf696b325e7c57238992f63ecdb5c3e56f`. The
fixture SHA-256 is
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.
`pairs.jsonl` contains ordered raw samples; `candidate.patch` captures the
single threshold change; `runner.py` records the paired procedure.

The next EarleyBoyer lead is the register-window `Construct` path: the
residual-scoped profile counted 8,866,509 constructions, and the path copies
the already-inline `CallArguments` slice into a new `Vec` before each call.
