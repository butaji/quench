# Merged-tree Splay screen and heap census

Date: 2026-10-11. Branch: `v2-cloud`. Source revision:
`cd0cacd23c5d0700a71b6c68027aee23dde304bd`, clean. Host: Linux x86_64,
kernel `6.18.44`, four-CPU quota, 16 GiB memory limit. Rust:
`1.99.0 (b940084d7 2026-09-28)`.

## Stock Splay screen

The production four-engine runner completed 11/11 valid and output-equal
rounds. Its report marks the host `qualification_ready=false`, so the result
is a Linux screen rather than a qualification. Medians:

| Engine | Score | Median maximum RSS |
| --- | ---: | ---: |
| Quench | 2,227 | 117,080,064 B |
| Node v26.10.0 `--jitless` | 3,456 | 403,005,440 B |
| Bun 1.4.2 JIT disabled | 1,950 | 62,017,536 B |

Quench is 35.6% below Node's Score and uses 1.89x Bun's RSS. Both bars remain
open. The unusually high Node RSS and non-qualification flag make this host's
RSS comparison diagnostic; the result still confirms the current Linux Splay
screen does not pass either gate.

## Merged-source heap composition

A separate `profile-memory` build ran the same Splay fixture. Its Score of
2,020 is instrumentation-contaminated and is not comparable to stock. Around
collection 18, immediately before sweep, the heap held 1,562,164 occupied
slots. After sweep, 781,055 cells remained: 512,343 objects and 256,084
arrays, plus 12,628 other cells. Each `Slot` is 56 bytes. The slot arena kept
87,506,944 bytes reserved for 1,562,624 slots, almost twice the post-sweep
live set. The live slots alone represent about 43.7 MB at this layout.

After sweep, 4,169 fully empty 4 KiB pages represented 17,076,224 bytes. The
accounted runtime composition was 93,012,014 bytes; this is a structural
estimate, not process RSS. The previous safe page-return screen showed that
returning such pages after sweep did not lower the observed RSS peak. This
census therefore points to pre-sweep slot capacity and the 56-byte slot
representation as the dominant opportunity. Slot representation belongs to
Task 61; this lane will not change it.

## Reproduction

The stock report records full runner arguments and executable hashes in
`stock-splay-11.json.gz`. Its uncompressed SHA-256 is
`813c2d52c654d5438f5989bf1c2f67884064fa33f95422bf65dce3dd73ab0b1c`.
The production Quench binary SHA-256 is
`bcf5dd77df96437bfa3c963be1c17feb41c2590507ad4a517bf5809b5cd37933`.

The profile binary SHA-256 is recorded in `binary.sha256` and is
`83ed2f5af3257b4f31b90f0126d47fe8bbb63a73bce50e90cf550ce7abcf1670`.
The complete profile stderr is compressed in `profile-stderr.jsonl.gz`
(uncompressed SHA-256
`9ed8c5833b2bd30bb4ed84bb177c556ebf9689ef574545880bcd9ee543072e8f`);
selected structural snapshots are in `snapshots.json`.

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' CARGO_TARGET_DIR=target/profile-memory-diagnostic \
  cargo +1.99.0 build --locked --offline --profile production \
  -p quench-node --features profile-memory --bin quench-node
QUENCH_MEMORY=1 QUENCH_MEMORY_PEAK=1 QUENCH_MEMORY_PAGE_SIZE=4096 \
  target/profile-memory-diagnostic/production/quench-node \
  evidence/task94-splay-empty-page-census-linux-2026-10-11/splay-memory-diagnostic.js \
  > work/task94/splay-merged-profile.stdout \
  2> work/task94/splay-merged-profile.stderr
```

The profile runner source SHA-256 is
`7fe05d49b019abf02b2a32120525e7e698f1f56b3205fcea6d26d7b0b7132d92`.
