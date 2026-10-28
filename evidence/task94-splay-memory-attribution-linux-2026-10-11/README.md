# Splay peak memory attribution — Linux, 2026-10-11

## Result

This is the Linux half of the open TOD-102 memory attribution. It uses the
per-slab bitmap candidate at source `bcf972683` and a `profile-memory` build
that reports the bitmap/free-slab metadata and Linux allocator state at each
memory snapshot. It does not close the M4 half, and the profile build is
instrumented rather than a qualification binary.

The current stock candidate's separate 11-round Splay report measured
114,819,072 B max RSS. At the pre-sweep snapshot around GC18, the slot arena
reserved 87,506,944 B and free-slot metadata reserved 205,824 B. The structural
runtime composition accounted for 95,056,920 B total:

| Peak component | Bytes |
| --- | ---: |
| Slot arena high-water capacity | 87,506,944 |
| Free-slot bitmap and free-slab capacity | 205,824 |
| Other accounted runtime memory | 7,344,152 |
| Total accounted runtime composition | 95,056,920 |
| Stock max RSS minus accounted composition | 19,762,152 |

So the stock peak has 27,312,128 B outside the slot arena. About 7.55 MB of
that is in the profiled runtime composition (including 0.206 MB of collector
metadata); 19.76 MB remains outside those structural counters. This subtraction
combines an uninstrumented stock peak with a separate instrumented structural
snapshot, so it is a cross-run estimate rather than an exact simultaneous
reconciliation.

At `gc_18_before`, the profile process itself reported 160,280,576 B RSS and
149,417,984 B anonymous/private-dirty memory. Its structural runtime composition
plus the memory profiler's own 60,856,976 B instrumentation was 155,913,896 B,
leaving 4,366,680 B outside the counters during that instrumented snapshot.
glibc `mallinfo2` at the same point reported 106,786,816 B arena capacity,
99,660,832 B in use, 7,125,984 B free across chunks, and 233,104 B releasable
from the arena top. Seven mapped allocations accounted for another 59,207,680 B.
Because the profiler changes allocator pressure and resident memory, these
allocator counters characterize the instrumented process; they cannot be
subtracted from the stock process's RSS.

The stock result still misses both Splay bars: Score 2,736 vs Node 5,433 and
max RSS 114,819,072 B vs Bun 62,672,896 B. The slot representation and its
2.00x capacity-to-live ratio remain the dominant structural term. The prior
safe page-return screen did not lower peak RSS. Task 61 owns changing that slot
layout; this collector lane has not changed it.

## Profile gate

- Rust 1.99.0, production profile, repository warnings denied.
- Profile binary SHA-256:
  `c393736fab60f48d7f0fb04e1c1fbf3f74070fe8734af2a9747a23958a24e22f`.
- Splay memory diagnostic fixture SHA-256:
  `7fe05d49b019abf02b2a32120525e7e698f1f56b3205fcea6d26d7b0b7132d92`.
- `profile-memory` instrumentation bytes at GC18: 60,856,976 B.
- Selected snapshots and the compressed raw JSONL are included. The allocator
  reader is active only for `profile-memory` on Linux GNU; ordinary Linux
  production builds keep the no-op reporter.

SHA-256:

- `allocator-profile.jsonl.gz`:
  `c2d925cb6aeb226bcbf01409fa1f3439058c289b1a645da544df736962169785`
- `census-fields.patch.gz`:
  `b4ebcb527dcc4b0f241cc1f6c160732b1c687fae3aa8a842d1aa28dac191ed31`
- `snapshots.json`:
  `6973785e8a123acfcac7495a294d7d25e2766de3febe805ef5ed1b852f7a2ef0`

## Reproduction

Build with the added census fields and Linux allocator reader:

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' CARGO_TARGET_DIR=target/profile-memory-diagnostic \
  cargo +1.99.0 build --locked --offline --profile production \
  -p quench-node --features profile-memory --bin quench-node
```

Run the Splay diagnostic with allocator snapshots:

```sh
QUENCH_MEMORY=1 QUENCH_MEMORY_PEAK=1 QUENCH_MEMORY_PAGE_SIZE=4096 \
  target/profile-memory-diagnostic/production/quench-node \
  evidence/task94-splay-empty-page-census-linux-2026-10-11/splay-memory-diagnostic.js \
  > work/task94-free-bitmap-allocator.stdout \
  2> work/task94-free-bitmap-allocator.jsonl
```

The recorded profile process max RSS used Python `resource.getrusage` on the
child process. The normal Splay Score/RSS bars come from the uninstrumented
stock report in [the free-bitmap screen](../task94-splay-free-bitmap-linux-2026-10-11/README.md).
