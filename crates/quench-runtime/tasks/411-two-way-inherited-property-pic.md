# 411 — Two-way inherited-property PIC physical-cover experiment

Status: complete

Retain the general bounded inherited-property PIC from Task 410, but publish exactly two
native ways. This is the smallest polymorphic cover and copies one additional guard chain,
not three, into each property stencil. Admission and replacement remain independent of
property name, source, benchmark identity, and execution heat.

This experiment separates the semantic algebra from its physical cover. The cache remains
one normalized finite sequence with source lookup, native projection, replacement, and GC
invalidation derived as folds. The copied stencil is the two-way prefix. Any later overflow
ways belong in one shared immutable Kernel so their code memory is paid once, preserving
the same `Connector -> Connector` category boundary.

Acceptance: 160 release tests pass; cooked disassembly proves exactly two native inherited
guard chains; Richards counters improve over Task 385; a short alternating full-suite screen
has no component-floor violation; and the randomized nine-pair exact gate has a strictly
positive aggregate confidence interval. Reject and restore Task 385 if exact evidence does
not clear that bar.

Baseline: `/tmp/deegen-task385-mixed-register-candidate`.

## Result: rejected before the exact gate

The two-way cover passed the focused source/native tests and reduced the copied expansion
to one additional inherited guard chain. In a 20 ms instrumented Richards run it recorded
57,340 inherited hits, 15,476 misses, and 15,515 fills over 230,355 direct-call attempts.
The alternating three-pair 100 ms screen is in
`reports/task411-two-way-pic-screen-3/comparison.txt`: aggregate was +0.48%, but Richards
was -0.89%. Two ways therefore removed Task 410's broad instruction-footprint regressions
but also failed to cover the polymorphism responsible for the target workload. It did not
justify the expensive exact gate and is removed in favor of the only remaining bounded
inline arity, Task 412's three-way experiment.
