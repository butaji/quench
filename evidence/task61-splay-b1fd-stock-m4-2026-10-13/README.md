# Splay diagnostic on `b1fd4907b` — M4 arm64 macOS

This is a Splay-only candidate measurement. It does not change the official
scoreboard row.

## Fixed-work comparison

The 11-pair comparison is output-equal and has 11/11 clean pairs. It compares
the pinned parent binary `01c22fa3…` with candidate `b1fd4907b`, binary
`0600ef24…`, using runner `101aa4b6…`.

| Metric | Candidate vs parent | 95% paired interval |
| --- | ---: | ---: |
| Marginal wall/run | +0.637% | −0.217% to +1.601% |
| Marginal cycles/run | +0.368% | −0.143% to +0.934% |
| Marginal instructions/run | −1.245% | −1.298% to −1.203% |
| Work max RSS | −0.390% | not calculated |

Wall and cycles are statistically flat; instructions are lower.

## Stock comparison

The latest 11-round Splay stock diagnostic is valid and output-equal. Maximum
sampled one-minute load was 2.62 on 10 logical CPUs; the five-minute maximum
was 4.46. One low Quench score and one low Node score occur in adjacent rounds.
The report is a single-fixture diagnostic (`qualification_ready=false`), so it
does not update the official trunk row. SHA-256: `f8466aa8a51bb6a4fd0069d950425530b2241e895eaa847e7c95646138e644a4`.

| Engine | Median Score | Median max RSS |
| --- | ---: | ---: |
| Quench candidate | 8,392 | 100,515,840 B |
| Node `--jitless` | 14,954 | 407,322,624 B |
| Bun no-JIT | 9,999 | 65,961,984 B |
| QuickJS | 8,630 | 151,044,096 B |

The bars are Node for Score and Bun for RSS. Candidate distances are **1.782×
Score / 1.524× max RSS**; neither bar is won. The previous ArrayRecord
snapshot measured 1.518× / 1.322×; the matched fixed-work screen below confirms
that the current candidate is slower and larger than that snapshot. The
official M4 trunk remains 1.648× / 1.781× on runtime `17077748a`.

Two earlier 11-round stock attempts are excluded for host interference; their
raw reports remain `stock-attempt-excluded-11.json` and
`stock-attempt-excluded-11-retry.json`.

## Matched fixed-work Splay screens

All screens used 300 `run()` calls, output-equal inputs, 11/11 clean pairs,
and deterministic 20,000-sample paired bootstrap intervals. Candidate binary
SHA-256 is `0600ef24633c8df6da83999a50a22e6a6554c49334eb9efb482059ea71bc48d7`.

Against the pinned M4 trunk binary for runtime `17077748a`
(`8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd`),
marginal wall/run was +7.369% [−0.857%, +23.973%], cycles +6.698%
[+2.353%, +11.527%], instructions +1.236% [+1.179%, +1.338%], and work
max RSS −11.983% [−12.707%, −10.977%]. The candidate trades lower RSS for a
measured cycle and instruction increase; it does not qualify as a Splay win.

Against the older ArrayRecord snapshot binary
`61373063e8d1ed13b1aafdc959e3b831b331db33fabb056a89129630eab8879a`, the
candidate was +14.836% wall/run [+13.912%, +17.817%], +17.015% cycles
[+15.614%, +17.433%], +7.177% instructions [+7.082%, +7.252%], and +17.588%
work max RSS [+17.531%, +17.700%]. This is a cross-revision comparison, not a
single-change attribution. Raw reports and derived summaries are
`fixed-work-vs-trunk-8432-11.json` / `.analysis.json` and
`fixed-work-vs-6186-11.json` / `.analysis.json`; their raw report SHA-256 values
are `28f5c5cb29fb3f3d1a3fb5495bafbc8f60dfbec576638f2a076a661f2681c73e` and
`8cbd67935c04bf60d0f6c8cb7f8d3f7d10f75a912328267f778602784e16efac`.


## Reproduction

```sh
target/pinned/101aa4b65250b8bdcc41095d10f2c5eeba22d151e25413c299fe9c15da8af964/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench /Users/admin/Code/GitHub/quench-v2/target/pinned/8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd/quench-node \
  --quench-peer target/pinned/0600ef24633c8df6da83999a50a22e6a6554c49334eb9efb482059ea71bc48d7/quench-node \
  --runs 11 --timeout-ms 300000 \
  --out fixed-work-vs-trunk-8432-11.json
```

Stock reproduction:

```sh
target/pinned/101aa4b65250b8bdcc41095d10f2c5eeba22d151e25413c299fe9c15da8af964/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench target/pinned/0600ef24633c8df6da83999a50a22e6a6554c49334eb9efb482059ea71bc48d7/quench-node \
  --qjs /opt/homebrew/Cellar/quickjs/2026-06-04/bin/qjs \
  --bun /opt/homebrew/Cellar/bun/1.4.2/bin/bun \
  --node /opt/homebrew/Cellar/node/26.10.0_1/bin/node \
  --runs 11 --timeout-ms 300000 \
  --out evidence/task61-splay-b1fd-stock-m4-2026-10-13/stock-retake-11.json
```

SHA-256 of the stock report is recorded in the evidence commit.
