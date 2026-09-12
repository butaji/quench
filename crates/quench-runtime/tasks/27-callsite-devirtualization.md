# 27 — Call-site devirtualization via monomorphic call guards

Status: complete

Apply the `CallTargetGuarded<F>` connector from [[25-generalized-speculative-guards]] to call sites: cache the observed callee identity and compile a direct, unguarded call (or splice the callee body via [[20-inline-via-node-composition]]) when a site has only ever observed one target, falling back to a generic indirect dispatch as the polymorphic/megamorphic case. This is the same specialization structure as [[08-property-inline-caches]] applied to call targets rather than property slots, and is expected to matter disproportionately for the OO-shaped V8v7 benchmarks (Richards, DeltaBlue) targeted by [[15-v8v7-10000-gate]].

Acceptance: a monomorphic call site compiles to a direct call or inline splice with a single receiver-identity guard; a call site observed with more than a small bounded number of distinct targets degrades to the generic dispatch without duplicating semantic call logic; invalidation on reassignment of the called function is covered by a test.

Current experiment: [[31-bounded-polymorphic-guards]] proved that the four-arm form is
fully exercised but regresses the whole suite. Its counters also show the primary arm
dominates: 3.33 million hits in Richards and 4.50 million in DeltaBlue. Retain exactly
one immutable callable target per `Call` site and send every different identity through
the canonical dispatcher without replacement. This removes the multi-arm scan and most
metadata while testing whether direct target selection itself has value.

## Result: rejected and reverted

The one-arm variant retained exactly one callable target per actual `Call` bytecode,
used one identity comparison, and never replaced it after a different callee appeared.
The identity-change/saturation test and all 63 release tests passed. The focused six-run
250 ms comparison in `reports/task27-monomorphic-call-focused-ab-6/comparison.txt`
regressed Richards 1.49%, DeltaBlue 0.71%, and their combined aggregate 1.10%.

Together with [[31-bounded-polymorphic-guards]], this shows that caching a Rust-level
call descriptor is not devirtualization enough: the extra guard and retained metadata
cost more than the small dispatcher it bypasses. Both variants were fully removed. The
source and release executable were restored byte-for-byte to
[[125-borrowed-callee-dispatch]], SHA-256
`9841db36898bc11b1e0cc7df323eb4f2d86ad75c55a2f3a08f004b15dbabd272`.
