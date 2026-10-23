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

## Clean production-control baseline

After pinning the source-matching production binary at SHA-256
`ac88cd0bf2b3822441f8cfd2fb1a57b59dc926b7dbe60ccd7fc295606f73eab0`, the
existing runner completed 11 rounds at 5,000 rounds and 250 warmups per engine.
The checksum was 120,000 in both modes. Linux sampled process-tree RSS makes
these diagnostic screens, not platform qualification:

| Mode | Quench ops/s / RSS | Node ops/s / RSS | Bun ops/s / RSS |
|---|---:|---:|---:|
| no-JIT | 27,027 / 31.04 MB | 86,207 / 50.21 MB | 70,922 / 38.24 MB |
| JIT | 29,070 / 23.60 MB | 357,143 / 62.80 MB | 465,116 / 52.26 MB |

Quench currently misses the 20% throughput margin by a wide amount in both
modes. No-JIT RSS is 18.8% below Bun, just short of a 20% reduction; JIT RSS
clears 20% against both. Reports are `target/perf-reports/zod-post-census-{nojit,jit}-11.json` (ignored).

## Exact Zod field paths

An opt-in `profile-lane-exits` build counted each field-cache probe and the
secondary path taken after a miss. I ran the existing `zod-parse` runner in
both modes at 1,000 rounds and 50 warmups (one sample per engine). The output
checksums matched. In `operations()` (`program=17`, `function=2`), each listed
count is 1,050, including warmups:

| Bytecode PC | Mode | Cache miss → shaped data read | Cache miss → getter call |
|---:|---|---:|---:|
| 38 | no-JIT / JIT | 1,050 | 1,050 |
| 43 | no-JIT / JIT | 1,050 | 1,050 |
| 52 | no-JIT / JIT | 1,050 | 1,050 |
| 77 | no-JIT / JIT | — | 1,050 |
| 78 | no-JIT / JIT | 1,050 | — |

This confirms several hot fields fall through the primary cache and reach
effectful getter calls on every iteration; treating all ordinary misses as
plain data reads would be incorrect. The counters aggregate identical PC,
atom, and outcome rows across cache indices because the first-level probe
records its local cache index while the secondary path records its global
index. They identify the semantic path, not a consistent cache-index identity.
The fourth tuple value (`field_site`) is the no-source-field sentinel for
these static bytecodes.

The field-path census is instrumentation-only, not a performance comparison.
Reports are `target/profile-diagnostic/zod-field-paths-{nojit,jit}.json`
(ignored local artifacts). The diagnostic binary was built from the current
source with the existing production profile and `profile-lane-exits`; its
build completed successfully.
