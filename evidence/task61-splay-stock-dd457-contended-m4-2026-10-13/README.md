# Excluded Splay stock retake on `dd457a54f` (M4)

This is an output-equal, 11-round Splay-only stock report for the current
`v2-merged` source, preserved for audit. It is **excluded from qualification**
because host contention began during round 2 and persisted through the end.
It does not update either official Splay distance.

## Identity

- Platform: Apple M4 arm64 macOS 26.5, model Mac16,10, 10 logical CPUs.
- Source revision: `dd457a54f858a9ef25cb97276468525aab3a8cd5`; source clean.
- Quench binary SHA-256: `01c22fa3d6c323159dbad38047b8cecb0ccd38edb77773821df17671c8257331`.
- `quench-bench` runner SHA-256: `101aa4b65250b8bdcc41095d10f2c5eeba22d151e25413c299fe9c15da8af964`.
- Pinned V8-v7 corpus: `64e1860c736c1b899708f4cd646721bc71e53d8b`.
- Raw report SHA-256: `a13c992c35703c7b56cf19692ef80389549a05cd313d6d438de61a314993d5e3`.
- All four engines returned valid, output-equal results in all 11 rounds.

## Why this is excluded

The first two rounds had one-minute load no higher than 2.61 and no Deno worker
in the sampled top CPU consumers. Deno first appeared in the snapshots in
round 2; the burst remained through rounds 3–10 and one-minute load reached
7.32. Scores fell across all engines when the burst began. This is engine-
sensitive host contention, so the medians below are audit data only.

| Engine | Median Score | Median max RSS |
| -- | --: | --: |
| Quench | 5,359 | 102,891,520 B |
| Node `--jitless` v26.10.0 | 9,111 | 403,554,304 B |
| Bun 1.4.2 no-JIT | 6,242 | 66,338,816 B |
| QuickJS 2026-06-04 | 5,501 | 151,060,480 B |

These ratios are deliberately not reported as distances: the campaign is
contended. The official M4 row remains unchanged until a clean 11-round stock
retake completes.

## Reproduction

```sh
target/pinned/101aa4b65250b8bdcc41095d10f2c5eeba22d151e25413c299fe9c15da8af964/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --runs 11 --timeout-ms 300000 \
  --quench target/pinned/01c22fa3d6c323159dbad38047b8cecb0ccd38edb77773821df17671c8257331/quench-node \
  --qjs /opt/homebrew/Cellar/quickjs/2026-06-04/bin/qjs \
  --node /opt/homebrew/Cellar/node/26.10.0_1/bin/node \
  --bun /opt/homebrew/Cellar/bun/1.4.2/bin/bun \
  --out target/splay-stock-dd457-m4-2026-10-13.json
```
