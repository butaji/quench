# 107 — Reuse name-snapshot value storage

Status: complete

Functions with effect-refreshed name snapshots allocate an `undefined` value vector up
to the last snapshot bytecode PC on every call, then drop the whole vector on return.
This is the remaining temporary value store outside
[[106-cleared-slot-pool-invariant]].

Acquire nonempty snapshot storage from the same cleared-value pool as registers and
return it after native execution. The existing snapshot refresh and AOT ABI remain
unchanged; heap-valued snapshots are released exactly once when the vector returns to
the pool. Empty snapshot sets allocate and pool nothing.

Acceptance: the name-snapshot semantic tests, all release tests, and full V8v7 smoke
pass; an exact alternating full-suite A/B against [[106-cleared-slot-pool-invariant]]
must clear aggregate and component floors. Revert and record rejection otherwise.

Result: rejected. The implementation passed all 44 tests and the complete-suite smoke,
but the exact six-repetition alternating comparison in
`reports/name-snapshot-pool-ab-6/comparison.txt` changed the aggregate from 835.883 to
827.031 (-1.06%) and reduced Splay from 2010 to 1896.5 (-5.65%), crossing the component
floor. Pooling this sparse temporary store also competes with ordinary register vectors
in one size-agnostic LIFO pool, so reuse is not predictably local. The experiment was
reverted; [[106-cleared-slot-pool-invariant]] remains the accepted checkpoint.

If this storage is revisited, use a distinct size-classed pool or eliminate the sparse
PC-indexed vector entirely. Do not mix it into the register pool again without evidence.
