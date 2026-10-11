# Splay empty-page census

Date: 2026-10-11. Branch: `v2-cloud`. Base revision: `75c7bab41` with
uncommitted profile-only page census changes. Host: Linux x86_64, kernel
`6.18.44`, 4 CPU quota, 16 GiB memory limit. Rust:
`1.99.0 (b940084d7 2026-09-28)`. Host page size from `getconf PAGESIZE`:
4,096 bytes.

## Result

The Splay fixture completed and printed `Score: 1417`. This optimized build
enabled `profile-memory`; its score and RSS are instrumentation-contaminated
and are not stock results.

The census counts a page only when it lies fully within one slot-slab
allocation and every slot whose bytes overlap the page is empty. It excludes
slab-edge pages and non-slot allocations. There were no fully free complete
slabs, but the mixed slabs did contain many empty pages:

| Post-GC snapshot | Fully empty 4 KiB pages | Bytes represented |
| --- | ---: | ---: |
| Collection 14 | 0 | 0 |
| Collection 15 | 1,340 | 5,488,640 |
| Collection 16 | 5,014 | 20,537,344 |
| Collection 17 | 4,915 | 20,131,840 |
| Collection 18 | 4,201 | 17,207,296 |

The maximum post-GC count is 20.5 MB of candidate pages, which is meaningful
headroom for a page-return experiment but still smaller than the 54.8 MB
Linux Splay RSS gap to Bun. This census does not establish safe decommitment:
discarding page contents would invalidate Rust `Slot` values unless the heap
tracks decommitted pages and restores every affected slot before access. It
also does not show that page return lowers the run's maximum RSS; the
measurement must be paired with a collector trigger early enough to affect the
observed peak.

## Reproduction

The binary SHA-256 was
`a8439ee1f7e71dd8c6f49d81f8bdd869ec7c52ca77ae8687598a50fa6c9e5a6f`.
The fixture-plus-runner is included (SHA-256
`7fe05d49b019abf02b2a32120525e7e698f1f56b3205fcea6d26d7b0b7132d92`).

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' CARGO_TARGET_DIR=target/profile-memory-diagnostic \
  cargo +1.99.0 build --locked --offline --profile production \
  -p quench-node --features profile-memory --bin quench-node
mkdir -p work/task94
QUENCH_MEMORY_PEAK=1 QUENCH_MEMORY_PAGE_SIZE="$(getconf PAGESIZE)" \
  target/profile-memory-diagnostic/production/quench-node \
  evidence/task94-splay-empty-page-census-linux-2026-10-11/splay-memory-diagnostic.js \
  > work/task94/splay-page-diagnostic.stdout \
  2> work/task94/splay-page-diagnostic.stderr
```

The full checkpoint JSONL is compressed as `profile-memory-stderr.jsonl.gz`
(SHA-256
`f39ee52c59d13c6d732685d835d8d7f893a896440204c795ff7080207ce04e0f`).
Captured stdout is in `stdout.txt`.
