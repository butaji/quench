# Splay full-growth and page-return policy screen

Date: 2026-10-11. Branch: `v2-cloud`. Base revision:
`c3d6a3be82293e7e077c1d5392609249ec86252e`, clean. Host: Linux x86_64,
kernel `6.18.44`, four-CPU quota, 16 GiB memory limit. Rust:
`1.99.0 (b940084d7 2026-09-28)`.

## Decision

No policy change is retained. Earlier full collections lower Splay work RSS,
but neither tested policy reaches the Bun RSS bar. Fixed-work output matched
11/11, while Linux counters were unavailable (0/11 clean timing rounds). The
raw wall deltas are directional only. Page return added no measured peak-RSS
reduction after full collections moved earlier. Splay remains open.

## Stock Splay screens

All candidate stock runs completed 11/11 valid and output-equal rounds; all
reports mark `qualification_ready=false`. Scores drifted substantially by
session, so use each report's same-session reference. RSS and Score are Linux
screening data, not M4 qualification.

| Candidate | Quench Score | Node `--jitless` Score | Quench max RSS | Bun no-JIT max RSS |
| --- | ---: | ---: | ---: | ---: |
| 1/2 headroom + 1/2 full-growth, no page return | 2,262 | 5,297 | 92,180,480 B | 62,144,512 B |
| 1/4 headroom + 1/4 full-growth, no page return | 1,696 | 4,800 | 80,744,448 B | 62,148,608 B |
| 1/2 headroom + 1/2 full-growth + page return | 1,633 | 5,460 | 92,147,712 B | 61,886,464 B |

Even the 1/4 policy remains 18,595,840 B above Bun and reaches only 35.3% of
the same-session Node Score. Its fixed-work setup-only RSS was 62,988,288 B,
slightly above that stock session's Bun RSS. The observed setup footprint and
the much larger live slot set point to Task 61's heap layout as a required
dependency for clearing RSS.

## Paired fixed-work attribution

Each candidate was paired against the clean `c3d6a3be8` Quench control for 11
rounds of 300 Splay `run()` calls. Outputs matched in every round. The runner
found no clean timing rounds because Linux hardware counters were unavailable.

| Candidate | Control work RSS | Candidate work RSS | Work RSS change | Setup RSS change | Raw paired wall median |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1/2 growth, no page return | 116,981,760 B | 91,844,608 B | −25,137,152 B | −393,216 B | +19.36% |
| 1/4 growth, no page return | 117,080,064 B | 80,609,280 B | −36,470,784 B | −671,744 B | +29.91% |
| 1/2 growth + page return | 116,965,376 B | 92,028,928 B | −24,936,448 B | −303,104 B | +56.06% |

The three fixed-work runs occurred in separate sessions, so their wall deltas
must not be compared against one another. Within each paired run, raw median
wall increased and is unqualified. The two 1/2 RSS reductions are close; page
return did not improve peak RSS in the candidate comparison.

## Reproduction

All raw stock and fixed-work JSON reports are compressed in this directory.
Candidate binary hashes are in `binary-*.sha256`. Policy patches apply to the
base revision above:

```sh
git apply evidence/task94-splay-growth-policy-linux-2026-10-11/growth-half.patch
# Or use growth-quarter.patch for the 1/4 policy.
```

The combined 1/2 plus page-return candidate patch is
`half-growth-page-return.patch.gz`:

```sh
gzip -dc evidence/task94-splay-growth-policy-linux-2026-10-11/half-growth-page-return.patch.gz | git apply
```

Build and stock-run commands:

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' cargo +1.99.0 build --locked --offline --profile production \
  -p quench-node --bin quench-node -p quench-bench --bin quench-bench
target/production/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench target/production/quench-node \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 --out work/task94/splay-stock-11.json
```
