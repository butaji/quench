# 162 — Non-moving generational nursery

Status: planned

Extend the stable-handle region heap from Task 148 with a small, non-moving young
generation. Allocate common fixed-size objects from size-class blocks using an inline
bump range rather than linking every free cell individually. Mark new cells by allocation epoch; an eden collection traces roots
and the remembered old-to-young set, retains survivors in place, and uses sticky mark
bits to distinguish old cells. A full collection remains the uncommon shared kernel.

Use block-local mark/new bitmaps with logical version numbers: starting a collection bumps
the corresponding global epoch rather than eagerly clearing every bitmap. A block is
physically refreshed only when allocation or marking first touches it in the new epoch.
This keeps the mutator allocation leaf to a cursor comparison and bump while sweeping and
bitmap scans remain in shared kernels. `GC_BLOCK_BYTES`, `GC_SIZE_CLASS_COUNT`,
`EDEN_ALLOCATION_BUDGET_BYTES`, and epoch-wrap behavior are named policies.

The mutation barrier is a composable effect stencil: perform the store, test the compact
cell-state byte, and append an old object to the remembered set only when required.
Named constants define block size, size classes, eden budget, promotion policy, and full
collection threshold. There is no per-`Value` retain/release and no moving pointer fixup.

Derive barrier elimination and coalescing from the quoted effect graph before stencil
emission. Stores into a newly allocated destination and stores of a proven non-heap value
need no generational barrier. Repeated pointer stores to the same destination object with
no intervening clobber/effect produce at most one cell-state test and remembered-set
action. Task 173's MemorySSA is the canonical proof source; individual property stencils
must not rediscover or mutate this fact independently.

VM registers and local/frame slots are precise roots, not counted owners. Copying a
heap-tagged `Value` between those locations must be the same one-word move as copying a
number. Collection scans the composable maps from Task 193. This deliberately takes the
performance lesson from deferred reference counting (omit frequent local-root count
updates) while using tracing to reclaim cycles and avoiding a separate zero-count table.

This adapts JavaScriptCore's block allocator and sticky-mark generational design without
its concurrent complexity. It is expected to matter most for Splay and allocation-heavy
object/call paths.

Acceptance: precise stress collection remains correct; cycles are reclaimed; allocation
fast paths disassemble to cursor check + pointer bump for supported cells; remembered-set
tests cover all heap edge kinds; Splay allocation/collection counters and full-suite A/B
improve without component regressions.

Sources: JSC's block directories, bump ranges, and logical mark/new versioning
<https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/> and
<https://webkit.org/blog/7122/introducing-riptide-webkits-retreating-wavefront-concurrent-garbage-collector/>.
