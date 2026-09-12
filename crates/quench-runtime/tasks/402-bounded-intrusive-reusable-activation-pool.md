# 402 — Bounded intrusive reusable-activation pool

Status: complete

Outcome: rejected; runtime change reverted.

Replace each monomorphic call site's single reusable `DynFrame` with a bounded intrusive
free list. Recursive and re-entrant calls currently take the only cached frame, allocate
another frame at every nested invocation, then discard all but one as recursion unwinds.
The exact Task 397 binary's 20 ms call diagnostic records 130,256 activation allocations in
Earley-Boyer and 288,143 in Splay despite 1,536,901 and 479,772 respective reuse events.

The call-site field remains `Option<Box<DynFrame>>`, so the array containing one `CallIcSite`
per bytecode does not grow. Each allocated frame carries its next free frame and the length
of the list it heads. Taking a frame exposes its immutable successor as the new site head;
returning a frame conses it onto the head. This is the free-list monoid represented as a
Lisp list rather than a separately allocated `Vec`:

```text
empty = Nil
recycle(frame, pool) = Cons(frame, pool)
take(Cons(frame, rest)) = (frame, rest)
```

`MAX_REUSABLE_ACTIVATIONS_PER_CALL_SITE` bounds retained high-water memory independently of
runtime heat. Overflow frames are dropped after canonical cleanup. `InlineCallTarget` keeps
publishing the current head pointer for Task 401's future native consumer. GC root tracing
walks every retained frame because sparse validated region contexts may retain guarded
objects even though ordinary value slots are cleared.

Acceptance: tests prove LIFO reuse, distinct simultaneous frames, bound enforcement, head
publication, canonical clearing, and full root traversal; all release and forced-GC tests
pass. Runtime counters must show allocation collapse on recursive suites. Retain only if a
complete alternating A/B improves without a component-floor violation; otherwise revert
the runtime representation and record the rejection.

## Result

The implementation worked mechanically. With a 20 ms diagnostic window,
Earley-Boyer activation allocations fell from 130,256 to 2,165 and Splay allocations fell
from 288,143 to 92. Both the 153-test release suite and the forced-GC suite passed. A test
fixture lifetime bug discovered during stress validation was fixed by retaining its
`CodeArena` for as long as the copied entry pointer can execute.

The first `usize` list-length representation enlarged hot frames too much. Packing the
length to `u8` improved the focused screen to +2.65% across Richards, Earley-Boyer, and
Splay, so that form entered the exact gate. The randomized nine-pair upstream-equivalent
comparison in
`reports/task402-bounded-intrusive-reusable-activation-pool/exact-vs-accepted/comparison.md`
measured **2368.27 -> 2363.02 (-0.22%)**, with a 95% paired bootstrap interval of
**[-0.83%, +0.60%]**. Richards regressed 5.00% and DeltaBlue 6.32%, while Splay improved
10.14% and Earley-Boyer 1.48%. The aggregate confidence interval includes no change and
DeltaBlue violates the component floor, so the runtime pool was reverted.

The result rejects globally enlarging every `DynFrame` to optimize recursive high-water
reuse. Task 401 should instead make recursive activation ownership part of the coarse
call-region/call-stack representation, where only call-containing covers pay for it. The
single reusable activation from Task 336 remains the accepted representation.
