# Splay free-slab census

Date: 2026-10-11. Branch: `v2-cloud`. Base revision: `5180b8713` with
uncommitted diagnostic-only census changes. Host: Linux x86_64, kernel
`6.18.44`, 4 CPU quota (`400000/100000`), 16 GiB memory limit. Rust:
`1.99.0 (b940084d7 2026-09-28)`.

## Result

The Splay fixture ran to completion and produced `Score: 1265`. This build
enabled `profile-memory`, so its score and RSS are instrumentation-contaminated
and are not compared with the stock gates. The census is taken at existing
before/after-GC memory checkpoints and only reads heap metadata.

Across all 18 after-GC checkpoints, **zero complete slabs were fully free**.
The latest full collection had 781,049 live slots across 762 complete slabs;
all 762 contained live cells. The arena reserved 87,506,944 bytes, or 763
slab allocations at 56 bytes per slot and 2,048 slots per slab. The remaining
allocation is the partially addressed high-water tail, so at most one slab
(114,688 B) can be reclaimed as a tail. The census does not measure fully
empty pages inside mixed slabs. Returning complete empty slabs cannot
materially reduce this run's RSS; page-level return still needs separate
evidence and a safe representation for decommitted slots.

After-GC census snapshots:

| Collection | Complete slabs | Fully free | Partially occupied | Reserved slot bytes |
| ---: | ---: | ---: | ---: | ---: |
| 15 | 525 | 0 | 523 | 60,325,888 |
| 16 | 762 | 0 | 762 | 87,506,944 |
| 17 | 762 | 0 | 761 | 87,506,944 |
| 18 | 762 | 0 | 762 | 87,506,944 |

The implementation also fixes the `profile-memory` accounting path, which
still referenced the retired flat `free` vector. It now counts the slab-local
u16 vectors, their outer Vec storage, and the slab stack. The Rust 1.99
warnings-denied check passed, and the focused profile-memory accounting test
passed 1/1.

## Reproduction

The optimized instrumented binary SHA-256 was
`01ed50746ac4c7b0e94bea9e87531ed24f49db0b86195afe5b623303e99296e0`.
The fixture-plus-runner source is included here (SHA-256
`7fe05d49b019abf02b2a32120525e7e698f1f56b3205fcea6d26d7b0b7132d92`).

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' CARGO_TARGET_DIR=target/profile-memory-diagnostic \
  cargo +1.99.0 build --locked --offline --profile production \
  -p quench-node --features profile-memory --bin quench-node
mkdir -p work/task94
QUENCH_MEMORY_PEAK=1 \
  target/profile-memory-diagnostic/production/quench-node \
  evidence/task94-splay-empty-slab-census-linux-2026-10-11/splay-memory-diagnostic.js \
  > work/task94/splay-memory-diagnostic.stdout \
  2> work/task94/splay-memory-diagnostic.stderr
```

Captured stdout is in `stdout.txt`. The complete memory checkpoint JSONL is
compressed as `profile-memory-stderr.jsonl.gz` (SHA-256
`ca0ec332e7ace71c68da055947b219c77db9f7f092bca5f16fb3a3e99a8b95e8`).
