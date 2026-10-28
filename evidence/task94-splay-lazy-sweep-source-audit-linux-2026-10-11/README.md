# Splay record-heap lazy-sweep source audit

Date: 2026-10-11  
Lane: `v2-cloud`, collector  
Scope: primary-source research for TOD-98; no runtime code changed and no
Quench performance claim is made.

## Findings

V8's current `PagedSpaceAllocatorPolicy` gives a concrete allocation-side
fallback order. After linear allocation fails, it tries the free list; while
sweeping is in progress it refills and retries, contributes at most one page
of sweep work and retries again. It then checks old-generation expansion policy
before its later full-sweep fallback. The allocator marks this work
`kLazyOrConcurrent` outside the GC pause. This supports bounded work on an
allocation slow path, but does not imply that one page is the right block size
or work quantum for Quench.

V8's Oilpan implementation describes the safety invariant for concurrent
sweeping: the sweeper processes dead memory while allocation uses already
swept pages. Its incremental mode likewise uses a free list from swept pages,
helps sweep on allocation slow paths, and seeks more memory from the OS only
after that reclaimed supply is exhausted. This is a correctness and ordering
reference; Quench currently uses stop-the-world collection and should start
with a single-threaded state transition.

## Quench application

On the accepted record heap, the first collector prototype should keep sweep
state per record block and follow this allocation path:

1. Allocate from the already-swept free bitmap/free list.
2. On miss, sweep one bounded block, publish its free slots, then retry.
3. Only after reusable swept supply is insufficient, consult the trunk-owned
   named headroom policy before growing the heap.
4. Keep a full-sweep completion path for verifier mode, shutdown and forced
   collection.

Do not publish free slots while a block is in `sweeping`; that prevents a
mutator allocation from racing with liveness processing. In Quench's current
single-threaded lane the state is still useful for explicit ownership,
verification and a later parallel sweep path. The block's mark/occupancy
metadata must be independent from `Rc` destruction so sweep work can be bounded
and charged to the requesting allocation.

This is a hypothesis for the record heap, not a current-heap change: the latest
TOD-10 queue says lazy sweeping waits for record spaces because today's `Rc`
drops make sweep serial. It also cannot by itself guarantee a lower peak RSS.
The V8 source expands old space before its full-sweep fallback; Quench should
compare that order with finishing a bounded sweep budget before growth, since
the latter may avoid touching fresh high-water pages at a score cost. The Splay
run must show that allocations reuse swept slots before expansion and that
fewer high-water pages are touched. Page return after the peak is not a
substitute for that evidence.

## Measurement plan after layout integration

Keep Splay as the only active fixture. For each 2.0x, 1.75x and 1.5x
high-water point, record paired fixed-work wall/cycles/instructions, setup and
work max RSS, blocks swept per collection, bytes available before and after
allocation-side sweeping, and allocations served from swept blocks versus heap
expansion. Require output equality and the lane's clean-cycle rule. Then run
stock Splay twice on Linux against Node `--jitless` and Bun no-JIT; the M4 trunk
owner runs the same active-fixture and all-eight gates. Lazy sweep is useful
only if it preserves Score while lowering maximum RSS; a decrease in sweep
pause or post-GC resident bytes alone does not close either bar.

## Primary sources

- V8 [`main-allocator.cc`](https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/heap/main-allocator.cc), current source, especially
  `PagedSpaceAllocatorPolicy::EnsureAllocation` and `ContributeToSweeping`.
- V8 [`High-performance garbage collection for C++`](https://v8.dev/blog/high-performance-cpp-gc), the Oilpan page-sweep states, allocation fallback and safety invariants.
- V8 [`Garbage Collection in V8`](https://chromium.googlesource.com/v8/v8/+/refs/heads/main/docs/heap/garbage-collection.md), old-generation sweep and free-list overview.
- Hans-J. Boehm, [`Reducing Garbage Collector Cache Misses`](https://www.hboehm.info/pubs.html), ISMM 2000, pp. 59–64. The paper evaluates lazy sweep as part of its cache-miss analysis; its findings motivate a Quench hypothesis, not a Quench performance claim.
- Chen-Yong Cher, Antony L. Hosking and T. N. Vijaykumar, [`Software Prefetching for Mark-Sweep Garbage Collection`](https://engineering.purdue.edu/~vijay/papers/2004/gc.pdf), ASPLOS 2004. This is background for mark-queue prefetch, which the Splay lane already screened and did not retain.

Research checked 2026-10-11. These references describe V8 and Oilpan designs;
they do not prove Quench will improve without matched measurements.
