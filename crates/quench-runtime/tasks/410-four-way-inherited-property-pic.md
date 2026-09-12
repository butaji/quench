# 410 — Four-way inherited-property PIC experiment

Status: complete

Replace the single published inherited-property cache entry with a bounded four-way PIC.
This is a general property-site mechanism: keys, source files, benchmark identity, and
execution heat cannot influence admission. Each immutable way publishes
`receiver-shape -> holder identity + holder shape + slot`; the direct GetStatic stencil
tests the finite ways and loads the fixed slot. The total semantic kernel performs prototype
walking and fills/replaces ways at the effect boundary.

Why this is next: the accepted Task 385 checkpoint leaves Richards' sampled time dominated
by semantic block execution. A 20 ms instrumented run reports 101,272 inherited-cache hits,
49,699 misses, and 49,738 fills, while direct calls hit 415,505 of 441,979 attempts. Residual
blocks are overwhelmingly property-load/call shapes (`GetStatic; LoadLocal; Call; Return`
alone re-entered 70,732 times). The evidence points to a monomorphic inherited cache
thrashing across receiver shapes, not to missing stencil execution generally.

The Lisp/category shape is one normalized cache-way value and one fold over it. The source
and raw AOT layouts share named `PROPERTY_IC_WAY_COUNT`; lookup, publishing, GC invalidation,
and tests derive from that representation. The hot stencil remains
`Connector -> Connector`; misses are one symbolic slow edge. No ad-hoc property-name
superinstruction is allowed.

Acceptance: layout assertions and tests cover fill, hit, replacement, shape mutation,
prototype identity/shape mutation, and GC invalidation; cooker disassembly proves the
bounded native way checks; counters show miss/fill reduction; Richards plus the full
randomized exact gate improves without a component-floor violation. Reject and remove if
four comparisons slow monomorphic sites more than reduced misses recover.

Evidence artifacts:

- `reports/task410-richards-profile-pair/profile-delta.tsv`
- `/tmp/task410-richards-call.err`
- `/tmp/task410-richards-res.err`

## Result: rejected physical cover

The four-way semantic and native layout was implemented and verified with 160 release
tests. The shared constant in `stencil-aot/object_layout.rs` drove both `repr(C)` layouts;
source lookup, bounded round-robin replacement, publishing, and selective GC invalidation
were folds over the same normalized way sequence. Cooked `deegen_dyn_get_static` and fused
`deegen_dyn_get_static_call` disassembly proved four direct receiver-shape, holder-identity,
and holder-shape guard chains rather than a dormant metadata cache.

The mechanism worked: the 20 ms Richards run reduced inherited-cache misses from 49,699 to
14,012 and raised its instrumented score from about 746 to 1,377. A short full-suite screen
showed +1.39% aggregate. The randomized nine-pair exact gate in
`reports/task410-four-way-pic-exact-9/comparison.md` rejected it, however: Richards gained
6.90%, but RayTrace lost 4.45%, Earley-Boyer lost 3.68%, and aggregate was -0.49% with a
95% paired interval of [-1.58%, +0.52%]. Copying three extra guard chains into every
property stencil inflated the catalog and instruction footprint even at monomorphic sites.

The four-way inline physical cover is therefore removed. Task 411 retains the proven PIC
policy but tests the smallest polymorphic native cover (two ways). A future overflow-way
design must use one shared immutable kernel; it must not duplicate the cold ways into each
StencilInstance.
