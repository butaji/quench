# 300 — Logical mark epochs and lazy block sweeping

Status: planned

Evaluate a block-local metadata representation for Task 148 after its adaptive collector
has complete timing and work counters. Replace per-cell physical mark clearing plus eager
whole-heap sweeping with compact mark/allocation bitmaps, a logical collection epoch, and
lazy free-list reconstruction for one allocation block at a time.

The canonical heap state is one block record:

```text
BlockMeta = { heap_id, mark_epoch, allocation_bits, mark_bits, sweep_state, free_cursor }
```

Object handles remain one-word stable pointers. Use a named power-of-two block size and
alignment so a cell address derives its block metadata by masking, then derives its slot
index from the block-relative offset. This makes marking O(1) without a chunk-list search
or an extra handle word. If the required platform allocation/alignment contract cannot be
proved, reject this representation rather than adding an unbounded side-table lookup.

A collection advances the logical epoch and marks roots/edges. It computes the live count
from successful first marks, invalidates identity-bearing IC facts, and queues blocks for
sweeping; it does not walk every cell merely to clear marks or build one global free list.
Allocation first consumes Task 299's untouched active tail. When it needs recycled space,
the slow kernel claims one queued block, drops its unmarked payloads, and builds that
block's compact free list. Optional bounded incremental sweep work may run at completed
frame safepoints, but the mutator's ordinary property/move/call stencils never participate.

This is a representation experiment inside the one stable-handle heap. It must not coexist
as a second collector. `OBJECT_HEAP_BLOCK_BYTES`, `OBJECT_HEAP_BLOCK_ALIGNMENT`, bitmap
word width, sweep quantum, and collection-growth policy are named constants derived from
cell size and measurements.

Risks: deferred sweeping temporarily retains Rust-owned payloads; custom aligned allocation
must pair exactly with deallocation; block masking must reject foreign handles; a reused
address must never satisfy stale property or `instanceof` caches. Task 193 root maps and
Task 162 remembered edges remain orthogonal inputs.

Acceptance: law tests prove logical epoch rollover handling, O(1) address-to-block and slot
recovery, foreign-handle rejection, exactly-once destruction, ABA invalidation, and bounded
reclamation under stress; diagnostics record mark, queued-block, sweep, reclaimed-cell,
and committed-byte work; release/stress suites pass; compare GC wall time, Splay score, and
RSS against the frozen adaptive Task 148 implementation before accepting.

Primary source: JavaScriptCore's block-directory allocator, outlined bitmaps, logical
versioning, active-block free list, and deferred/incremental destruction
<https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>. Immix supplies the
block/line locality model <https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf>.

