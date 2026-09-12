# 225 — Amortized O(1) Array.prototype.shift via front-offset deque representation

Status: planned

`src/main.rs:4741` (`native_array_shift`) implements `Array.prototype.shift` as
`b.array.as_mut().unwrap().remove(0)` — `Vec::remove(0)` shifts every remaining element
down by one slot, so a single `shift()` call is O(n) in the array's current length. The
classic and extremely common JS idiom `while (arr.length) { process(arr.shift()) }`
(a work-queue drain — exactly the shape of a task scheduler, which is what richards.js
*is*) turns this into an O(n²) total-cost loop for what every engine treats as an O(n)
operation. `native_array_unshift` (`src/main.rs:4752`) has the analogous cost via
`Vec::insert(i, v)` in a loop, but unshift's O(n) cost is closer to unavoidable for any
contiguous-array backing store and is not this task's target — every major engine pays
a real cost for unshift too, whereas shift is the one operation with a well-known O(1)
amortized fix.

This is not covered by [[12-array-fast-path]] or [[32-element-kind-guarded-arrays]] —
both address which representation an array's *elements* use, not the cost of removing
from the front of the backing store, which is an orthogonal representation question
(deque-style front offset vs. a plain `Vec`, exactly the difference between `Vec` and
`VecDeque` in the Rust standard library, or between a plain array and V8's own internal
handling of small-element-removal-from-front cases).

Corpus honesty check: grepping `/private/tmp/js-engine-benchmark/v8-v7/*.js` for
`.shift(`/`.unshift(` finds zero call sites — richards.js implements its own explicit
linked-list queue (a `Packet`-style `link` field) rather than using native `Array`
methods for its scheduler queue. This task is therefore **not** motivated by measured
V8v7 evidence the way [[220]]/[[221]]/[[222]] are, and should not claim a V8v7 score
gain until a suite is found that actually exercises it. It remains worth doing as a
general correctness-of-algorithmic-complexity fix (any JS program using the standard
queue-drain idiom would otherwise silently get O(n²) instead of the O(n) every major
engine provides), but rank it below corpus-evidenced work and do not expect it to move
[[15]]'s score.

Concrete design: give the dense-array backing store a front offset (a logical start
index into the backing storage, the same idea `VecDeque` or a ring buffer uses), so
`shift()` becomes "advance the front offset and return the vacated element" — O(1) — and
periodically (once the wasted prefix crosses a size threshold relative to the live
length) compact the backing store in one O(n) pass, amortizing to O(1) per shift over
any sequence of operations. This interacts with [[32]]'s element-kind-guarded array
connector and [[209]]'s access-stencil work, since every direct-indexed read/write
through those connectors would need to account for the offset (or the offset must be
folded into the base pointer computation those connectors already do, adding no new
runtime cost to indexed access itself).

Concrete steps:
1. Add a front-offset field to the array backing representation, defaulting to zero for
   every array that never calls `shift()` (so arrays that only ever `push`/index-access
   pay no new cost, verified by measuring that unaffected suites show no regression).
2. Implement `shift()` as an offset advance plus element take, with periodic compaction
   when the offset exceeds a stated fraction of backing capacity (name the threshold as
   an explicit policy constant, consistent with this project's standing "no unexplained
   numeric threshold" discipline).
3. Fold the offset into every existing direct-indexed array access path
   ([[12]]/[[32]]/[[99]]/[[105]]) so indexed reads/writes remain a single computed
   address with the offset pre-added, not a separate runtime check per access.

Acceptance: `shift()` is O(1) amortized, verified by a benchmark draining a large array
via repeated `shift()` showing linear (not quadratic) total time as array size grows;
every existing array correctness test passes unchanged, including mixed
push/pop/shift/unshift/index-access sequences and sparse/holey arrays; arrays that never
call `shift()` show no measured regression on indexed access (the offset must be free
when it is always zero); alternating A/B on the full V8v7 suite shows no regression,
consistent with the corpus honesty check above finding no `shift()`-driven suite to
expect a gain from.
