# Negative in-loop call eligibility census

This is a diagnostic-only census built with `profile-lane-exits`; its timings and
RSS are instrumented and must not be used as qualification numbers. The census
tests whether negative eligibility memoization deserves an implementation
screen. The feature records exact cache hits, positive resolutions, and
negative resolution categories, while the ordinary call dispatcher still
executes every declined call.

| Workload | Phase | Positive cache hit | Positive resolution | Negative resolution | Negative share |
|---|---:|---:|---:|---:|---:|
| Zod parse (1,000 rounds, 50 warmups) | no-JIT | 40,554 | 17,455 | 3,433 | 5.6% |
| Zod parse (1,000 rounds, 50 warmups) | JIT | 42,862 | 18,964 | 3,433 | 5.3% |
| Pino JSON logger (1,000 rounds, 50 warmups) | no-JIT | 10,080 | 2,395 | 1,646 | 11.7% |

The negative share does not support a 20% call-path win: even removing every
negative probe's resolution work would affect only a small part of all
in-loop-call lookups, while increasing cache state and invalidation complexity.
Do not add negative memoization based on this evidence. The larger JIT ceiling is
in hot functions that currently fall back instead of entering native code; the
Zod `operations()` wrapper had 1,050 fallbacks and zero native entries in the
earlier 1,000-round profile.

Reports: `target/profile-diagnostic/zod-negative-eligibility-{nojit,jit}.json`
and `target/profile-diagnostic/pino-negative-eligibility-nojit.json` (ignored
local artifacts). These were single-round diagnostic runs, not paired
performance comparisons. All outputs/checksums were validated by the existing
runner. No runtime candidate was implemented from this census.
