# Splay stock retake after trunk speed head 3d3d (Linux)

## Result

The collector branch merged `origin/v2` at `4a3925c7b` in merge commit
`7008a643d`, including runtime head `3760888a0` from the speed lane. The M4
fixed-work gate for that speed head passed Splay (−17.92% paired marginal
wall/run) and all eight fixtures, but its stock run is still pending.

Two Splay-only Linux stock sessions were run on the merged source. Both were
valid and output-equal in all 11 rounds. The runner marks them
`qualification_ready=false` because the fixture set contains Splay only. Score
varied widely between rounds (per-engine coefficient of variation 19.8–34.1%),
so these sessions do not provide a stable current Score distance. Both medians
remain below Node's `--jitless` median. RSS was stable across sessions and is
still far above Bun no-JIT.

| Session | Quench Score | Node `--jitless` | Bun no-JIT | QuickJS | Quench max RSS | Bun max RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 2,083 | 3,729 | 2,537 | 2,513 | 114,577,408 B | 62,803,968 B |
| 2 | 2,493 | 3,243 | 2,233 | 1,944 | 114,577,408 B | 63,057,920 B |

The pooled 22-round medians are Quench 2,415 vs Node 3,288.5 Score (1.36×
behind), and Quench 114,577,408 B vs Bun 62,869,504 B max RSS (51,707,904 B,
1.82× above). Because of the Score variance, use these only as diagnostics.
Neither bar is closed, and this is not an all-eight qualification run.

No collector change was made. The merged trunk still has the current slot heap;
the accepted record heap needed for lazy block sweep and high-water tuning has
not landed. Task 61 / TOD-8 remains with the trunk lane.

## Provenance

- Source revision: `7008a643d643636ba675a46be3a8d5a871635861` (clean).
- Merged `origin/v2`: `4a3925c7b5ccf0d5c16560ddacdf305ca9ae71ac`.
- Production build: Rust `1.99.0 (b940084d7 2026-09-28)`.
- Host: Linux x86_64, kernel `6.18.44`, 4 logical CPUs, 4-CPU cgroup quota,
  16 GiB memory limit; metrics via `wait4` rusage.
- Quench executable SHA-256:
  `e92ba8bf6f8866d3b048e15660a81a132234124a38e13e9cfa7910c4db023b62`.
- Runner SHA-256:
  `924e62eee00ea5964d140f1cb04a6d1e125a2dfec08271cb0f28d4b933f79deb`.
- QuickJS SHA-256:
  `c798e32aac14495856a4709be67a11b0562a6e2d07fa89ad4f28efe46c7c7955`.
- Bun SHA-256:
  `a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c`.
- Node v26.10.0 SHA-256:
  `ab9c8eecf9f82d6693cdc3accced17034065c8d96213b0aa76a7e803d20ae1da`.
- Pinned fixture corpus revision: `64e1860c736c1b899708f4cd646721bc71e53d8b`.
- Raw report 1 SHA-256: `787f0b55a19ec58970f04eec60a0c410ab2d0ec2af201bb5129dbd87cbfd9220`;
  compressed report SHA-256: `b46dfb4d1b7e425ab5dc8a0253c592f4ccddd4a83ce1af453ceb76b3583f802a`.
- Raw report 2 SHA-256: `120780f684d164da4561b9f6ed49ddd034ac5f8484cf27d5b93bd2d5e0006eb3`;
  compressed report SHA-256: `d5548b6f2870db8da5f3eec5334d16a17972b7604d23ff85c9d2b53292c488b8`.

## Reproduction

```sh
source /workspace/quench-build-env.sh
cargo build --profile production -p quench-node --bin quench-node

target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench target/production/quench-node \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 \
  --out work/task94-v2-cloud-3d3d-splay-stock-11.json

# Repeat with the output path changed to
# work/task94-v2-cloud-3d3d-splay-stock-repeat-11.json.
```

Compressed raw reports are stored beside this README.
