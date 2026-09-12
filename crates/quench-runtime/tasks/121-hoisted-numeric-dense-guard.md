# 121 — Hoisted numeric/dense guard connector

Status: complete

Current work: represent packed-versus-tagged dense storage as an explicit object fact,
derive a compact guard plan from the Task 120 quote, and validate it once at the region
entry edge. The plan must not retain the quoted analysis tree after linking.

Lower the obligations produced by [[120-typed-numeric-dense-region-ir]] into one entry
guard per coarse region. The guard validates numeric loop-carried values plus receiver
identity, dense backing kind, bounds envelope, and backing-storage stability. A success
produces a typed raw region context; failure transfers to the canonical dynamic stencil
at the original bytecode label before any region effect occurs.

The guard is a normal stencil morphism and may be shared as an immutable kernel when it
has no per-site holes. Invalidation facts are explicit effects. No body operation may
repeat a tag, shape, or element-kind proof already represented by the region context.

Acceptance: mutations capable of resizing/replacing the backing store invalidate or
end the region; aliasing and out-of-bounds cases fall through correctly; focused tests
cover packed numeric, holey/tagged, prototype, and resize transitions; the emitted body
contains no repeated entry guards.

## Result: complete

`ArrayStorage` now maintains explicit `non_number_count` and `backing_version` facts,
so packed-numeric validation is O(1) and packed↔holey transitions are derived from the
single canonical mutation API. `GuardPlan` is a compact value derived once from the
quoted loop; the quote is discarded after linking. It records number/dense sources,
deduplicated backing identities, dense-site view indices, carrier-register ownership,
and first-write register/local clobbers.

The entry guard validates all facts before effects, retains each unique array backing,
installs raw pointer/length views, clears prior heap owners that unchecked writes will
replace, and installs owned receiver copies needed by exact computed-access bailout.
Only in-bounds dense writes are allowed inside the region, so no accepted body operation
can resize or replace the guarded backing. A miss executes the canonical bytecode
region and never enters a private optimized label without a new guard.

Focused tests cover packed arrays, holey-to-packed fill, aliases, backing-version
changes, prototype identity, and first-write local ownership. The 61 release tests and
complete V8v7 smoke pass. Diagnostic evidence in
`reports/task123-crypto-region-stats.txt` and
`reports/task123-navier-region-stats.txt` records 155/3 guard failures against
137,219/12,760 successes; exact bounds failures preserve canonical semantics.
