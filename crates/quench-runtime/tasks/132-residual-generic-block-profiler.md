# 132 — Residual generic-block profiler under stencil selection

Status: complete

Measure only the semantic blocks that remain generic after the normal direct-block and
numeric-region selectors run. The earlier Task 128 frequency artifact deliberately
disabled direct stencils to measure the whole bytecode vocabulary; it cannot rank the
remaining optimization opportunities after Tasks 129–131.

Add an opt-in residual-statistics helper family selected at function-link time. Normal
execution must keep compile-time-false instrumentation and pay no per-block environment
lookup, atomic, or statistics branch. Profiling must leave the ordinary stencil selector
enabled and record the normalized block shape through the existing `JitStats` data model.

Acceptance: release tests, complete short V8v7 artifact from the accepted Task 131 binary,
proof that direct blocks/regions remain selected during measurement, and a ranked list of
residual shapes and semantic families. Use the result to define the next general-purpose
coarse stencil grammar; do not select source names, property names, literal values, or
benchmark identity.

## Result

The residual mode selects distinct monomorphized helper functions at link time. The normal
helpers instantiate `RESIDUAL_STATS = false`, so normal block execution contains no added
statistics branch or environment lookup. The profiling helpers instantiate it as `true`
while direct-block, numeric-region, and Task 131 selectors remain enabled.

All 74 release tests pass. The complete short artifact is
`reports/task132-residual-generic-blocks.jsonl`; its normalized summary is
`reports/task132-residual-generic-blocks-summary.txt`. Every suite retained nonzero direct
selection. A simultaneous Crypto numeric-region probe retained 29 linked regions and
1,796,041 region iterations.

The accepted runtime still makes 9,714,173 residual generic-block entries in the short run:
4,319,715 static-property, 2,736,545 call/construct, 1,106,750 name/environment, and
412,515 generic-operator entries. A particularly actionable general defect is that selected
return stencils fall back for heap-owned values: standalone `Return` contributes 121,459
entries and `LoadLocal,Return` contributes 254,280. Task 133 will first make terminal return
an ownership-transfer morphism; larger property/call blocks still require coarse templates,
not opcode-leaf fragmentation.
