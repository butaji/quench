# 416 — Alias-preserving register-location state

Status: complete

Represent numeric `Move` as a quote-stage name-to-location update instead of allocating a
second physical lane and selecting a copied move stencil. Several SSA names may therefore
refer to one `RegisterLocation`; liveness removes names independently and the lane becomes
free only after its final alias dies. Temporary representation conversions are memoized by
physical source location rather than source name, preventing aliases from paying duplicate
cross-bank conversions within one operation.

This is the small category-law slice of Task 158's multi-location state. A value location is
the object being preserved; renaming is the identity morphism. The existing finite move
stencils remain available for future CFG-edge parallel copies, where locations genuinely
differ. No runtime assembler, benchmark identity, hotness policy, or interpreter fallback
is introduced.

Named counters expose zero-byte alias updates and maximum shared-location fanout. A unit
fixture builds eight general Word32 operations with live source/destination aliases and
proves that all moves normalize to identities, maximum fanout is two, and no physical move
step remains. All 160 release tests pass.

## V8v7 result

Retained as infrastructure, not claimed as a performance win. The 50 ms smoke aggregate was
2621.09. A three-pair 100 ms screen against the accepted Task 385 binary is in
`reports/task416-alias-preserving-screen-3/comparison.txt`; its +0.47% aggregate change is
below the evidence threshold and generated function-image byte counts were identical for
all eight suites.

The diagnostic reason is exact: Crypto selected five register regions and reported 24 alias
updates, all 24 coming from the pre-existing forwarded-local path. No surviving numeric
`Move` occurred in a selected V8v7 register region. This implementation therefore removes
a real abstraction defect and protects future CFG work, but it cannot improve the current
suite until register-region coverage expands. The next work item must count the discarded
plans by rejection reason rather than guessing which restriction hides the available aliases.
