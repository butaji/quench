# Smaller young generation with time-paced page releases: rejected

The idea came from the eager-collector bound (`tod137-eager-collector-bound-…`). With releases
off, frequent collection cost Richards only 5% of its score, so collecting more often while
releasing pages at most every N µs might keep the garbage saving without the refault cost.

The probe used one gc-tuning static binary (v2-gc `3df28ee231`) with a temporary switch,
`QX_RELEASE_US`, that skips a collection's page release unless N µs have passed since the last
one. Six alternating runs, MemoryFirst, no JIT; a test build ran on the host meanwhile, so the
scores are noisy. Each cell is the KiB change against default, with the score change in
brackets (`pace-6-runs.txt`).

| Row | young 2^10 | 2^10, releases ≥ 2 ms apart | 2^10, ≥ 10 ms | 2^12, ≥ 2 ms |
| -- | --: | --: | --: | --: |
| Richards | −46 (0%) | +146 (+4%) | +204 (+3%) | −162 (+4%) |
| RegExp | −126 (−16%) | −224 (−7%) | −134 (+5%) | −30 (+1%) |
| Crypto | +128 (−5%) | +154 (−8%) | +84 (−3%) | +22 (−1%) |
| RayTrace | 0 (−38%) | +70 (−4%) | +52 (+12%) | +82 (+11%) |
| DeltaBlue | +256 (−1%) | +238 (−2%) | +256 (0%) | 0 (−4%) |
| EarleyBoyer | −34 (−26%) | −40 (−11%) | −104 (−4%) | −4 (+8%) |

Decision: rejected. No arm lowers the peak on most rows. A peak release has to land at the peak,
so pacing the releases by time misses peaks that releasing at every collection catches.
