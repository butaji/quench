# 299 — Bump-first object allocation leaf

Status: complete

Make the common `ObjectHeap` allocation edge exactly one active-range check, payload
initialization, and cursor increment. The current implementation borrows and probes the
recycled-cell vector before every allocation, even while the active chunk contains never
initialized cells.

Use one explicit state transition:

```text
ActiveTail(cursor < limit) -> initialize cursor -> cursor + 1
Exhausted                  -> recycled/chunk slow kernel -> ActiveTail
```

On exhaustion, consume reclaimed cells before reserving another chunk. Allocation debt,
live-cell accounting, collection requests, and payload initialization occur exactly once
on both arms. The common stencil and slow shared kernel have the same input/output frame
context and are therefore interchangeable categorical morphisms; only their storage and
sharing policies differ.

Keep `OBJECT_HEAP_CHUNK_CELLS`, alignment, minimum collection budget, and all thresholds
as named constants. Do not add a second free list, allocation counter, or object-construction
semantics. Task 191 may later fold several `Fresh` allocations into one range check using
this same primitive.

Acceptance: focused tests cover untouched-tail allocation, reclaimed-cell reuse before
chunk growth, cursor exhaustion, GC debt, and stress collection; disassembly of the fast
leaf contains no `RefCell` borrow or free-vector operation; allocation counters distinguish
bump versus recycled versus new-chunk arms; release and stress-GC suites pass; alternating
Splay and full V8v7 A/B improve without increasing the accepted RSS bound.

Primary sources: MMTk's inline bump allocator
<https://www.mmtk.io/assets/pubs/mmtk-sigmetrics-2004.pdf>, Immix recycled-region bump
allocation <https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf>, and rustc's
arena allocator description <https://doc.rust-lang.org/stable/nightly-rustc/rustc_arena/>.

## 2026-09-10 implementation checkpoint

The experimental `ObjectHeap::allocate` tested the untouched active tail first. The cursor
path was `#[inline(always)]`; the exhausted-tail operation was a `#[cold]`,
`#[inline(never)]` kernel that consumed reclaimed cells before reserving another chunk.
Allocation debt and live-cell accounting remained one common post-transition operation.

A focused state-machine test created a reclaimed cell while the active chunk still had an
untouched tail, proved the next allocation advanced that tail, filled the tail, then proved
the following allocation reused the reclaimed address without growing the chunk set. The
candidate also exposed Task 305's missing host-root boundary instead of immediately
overwriting the reclaimed prototype cell; that independent correctness fix is retained.

## Result: rejected and removed

The frozen candidate
`2c886a607d7978bcc0a8fd1cd0e9a53349e0ef5c4831006bcb74633a01425335`
passed all 109 release tests both normally and with collection requested at every completed
frame. The slow exhausted-tail kernel remained outlined in disassembly. Five alternating
500 ms full-suite pairs measured aggregate V8v7 2,058.87 to 2,058.68 (-0.01%). Splay was
+1.15%, Richards +0.77%, and Navier-Stokes -1.66%. Candidate two-second Splay maximum RSS
was 443,301,888 bytes, effectively identical to the adaptive baseline. Evidence is in
`reports/task299-bump-first-full-ab-5`.

The allocator ordering is therefore performance- and memory-neutral and fails the standing
gain gate. It and its state-machine test were removed; the accepted free-list-first order
is restored. Do not repeat allocator micro-ordering without a profile showing this exact
borrow/probe dominates. The next allocation work remains Task 191's grouped range check or
Task 162's generational policy, both of which remove work at a coarser granularity.
