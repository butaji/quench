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

## Stock attempts

Both 11-round stock attempts are excluded: although each produced valid,
output-equal samples, host interference and a cross-engine score collapse
occurred during the runs. The raw reports are retained as
`stock-attempt-excluded-11.json` and
`stock-attempt-excluded-11-retry.json`; no distances are derived from them.
No official M4 score or RSS result changes.

The official M4 Splay row remains 1.648× Score / 1.781× max RSS on runtime
`17077748a`. A clean stock retake on the current source is still required.

## Reproduction

```sh
target/pinned/101aa4b65250b8bdcc41095d10f2c5eeba22d151e25413c299fe9c15da8af964/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench target/pinned/01c22fa3d6c323159dbad38047b8cecb0ccd38edb77773821df17671c8257331/quench-node \
  --quench-peer target/pinned/0600ef24633c8df6da83999a50a22e6a6554c49334eb9efb482059ea71bc48d7/quench-node \
  --runs 11 --timeout-ms 300000 \
  --out fixed-work-parent-comparison-11.json
```

The report files are `fixed-work-parent-comparison-11.json` and the two
excluded stock reports. Their SHA-256 values are recorded in the commit that
adds this directory.
