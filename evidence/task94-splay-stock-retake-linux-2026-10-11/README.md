# Splay stock retake on the collector branch (Linux)

## Result

Two fresh 11-round stock screens ran the exact `a125dc95f` Quench executable.
Both were valid and output-equal in all 11 rounds. The second session reproduced
the earlier Linux baseline closely. This is a Splay-only Linux diagnostic, not
a qualification report: `qualification_ready=false` because the selected
fixture set is only Splay (`fixture_set_matches=false`). The source tree and
pinned corpus were clean.

| Session | Quench median Score | Node `--jitless` median Score | Bun no-JIT median Score | Quench max RSS | Bun max RSS | RSS ratio |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| First | 1,865 | 2,836 | 1,987 | 117,551,104 B | 62,537,728 B | 1.88x |
| Repeat | 2,776 | 5,484 | 3,700 | 117,587,968 B | 62,148,608 B | 1.89x |

The first session had wide score movement across every engine (Quench 1,441–
2,817; Node 1,304–4,851; Bun 1,100–3,814), so it is kept as a noise diagnostic.
The repeat medians agree with the previous Linux screen on the same binary:
Quench 2,773 vs Node 5,142, and RSS 117,227,520 B vs Bun 62,410,752 B. The
latest repeat still reaches only 50.6% of Node's score and uses 1.89x Bun's
RSS. Both Splay bars remain open.

## Provenance

- Source revision: `cbd6046a7e4c918980b10b07a7dea22ac75a661f`.
- Quench executable SHA-256:
  `6aaf73b79e0e71419a1ed9ebfe0e705e8c755c72e6ac0f1a4ea9b68e61e27bbd`.
- Benchmark runner SHA-256:
  `0c553c518dcdd9d57ed0900ad4eb494804eb844675ba0942907cca4cf15c7d20`.
- QuickJS SHA-256:
  `c798e32aac14495856a4709be67a11b0562a6e2d07fa89ad4f28efe46c7c7955`.
- Bun SHA-256:
  `a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c`.
- Node SHA-256:
  `ab9c8eecf9f82d6693cdc3accced17034065c8d96213b0aa76a7e803d20ae1da`.
- Host: Linux x86_64, kernel `6.18.44`, 4 logical CPUs, cgroup CPU quota 4,
  16 GiB memory limit; benchmark collection used Linux `wait4` rusage.
- Rust: `1.99.0 (b940084d7 2026-09-28)`.
- Raw reports are ignored under `work/task109/`:
  `splay-current-stock-a125-11.json` and
  `splay-current-stock-a125-repeat-11.json`.

## Reproduction

```sh
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench work/task109/splay-baseline-a125-node \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 \
  --out work/task109/splay-current-stock-a125-11.json
```

The second run used the same command and an output path ending in
`splay-current-stock-a125-repeat-11.json`.

## Current M4 reference

The integrated `origin/v2` candidate-only M4 stock report remains the more
relevant platform reference: Quench Score 7,043 vs Node 14,897 and maximum RSS
111.41 MiB vs Bun 59.03 MiB (2.12x speed gap, 1.89x RSS). It is documented in
[`task61-splay-speed-lane-b72-m4-2026-10-11`](../task61-splay-speed-lane-b72-m4-2026-10-11/README.md).
That run is also diagnostic; Stage B remains open.
