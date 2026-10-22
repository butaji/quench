# Rejected EarleyBoyer `LoadConst → Return` fusion (2026-10-10)

The residual profile showed 16.9 million adjacent `LoadConst → Return`
executions. I added `ReturnConst` to return a program constant directly,
avoiding the temporary register write. A focused Node v24.19.0 oracle matched
baseline and candidate for returns of undefined, null, booleans, numbers,
NaN, infinity, and strings; returns through `finally` and `catch`; arrow
returns; and constructor primitive returns. EarleyBoyer output matched in all
11 production pairs.

The candidate executed `ReturnConst` 10,690,580 times in the profile run, so
the rewrite did reach this fixture's hot path. However, the 11-pair production
sample did not establish a Score or RSS win: median Score delta was +2 points
(95% bootstrap interval [−3, +7]), with higher Score in 8/11 pairs; median
maximum RSS delta was zero bytes (interval [−143,360, +167,936]), with lower
RSS in only 4/11 pairs. I removed the candidate. The three-pair directional
screen and full 11-pair report, Node oracle, source patch, profiler output,
runner, and per-run outputs are preserved here.

Baseline binary SHA-256:
`e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32`.

Candidate binary SHA-256:
`b5e48540e41d50373cca5e2c5988f9d140ef1f6d24ceb48a286e1e894addd9e7`.

Fixture SHA-256:
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

Node oracle SHA-256:
`2d34d3a74fd1f8c4cbb975be8bbf3d89804a8d9ceba12c5847ef4171f10ada4e`.
