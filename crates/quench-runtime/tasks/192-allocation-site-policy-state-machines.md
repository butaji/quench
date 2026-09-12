# 192 — Allocation-site representation, capacity, and lifetime policies

Status: planned

Attach a bounded `AllocationPolicy` state machine to allocation-site stencil instances.
Slow allocation/GC events update only semantic facts: initial element representation,
observed growth capacity, and survival generation. Stable policies patch future
allocations toward presized/pretransitioned storage or old-generation allocation; this is
allocation feedback, not execution-count hotness.

Policy is immutable snapshot data read by a shared allocation kernel. Transitions and
confidence/survival budgets are named constants, saturate monotonically, and are never
selected by source or benchmark identity. A wrong prediction remains a correct slow
transition, not deoptimization to an interpreter.

Acceptance: policy transition tests cover oscillating element kinds, capacity waste, and
young/old survival; DeltaBlue transition counts and Splay collections are measured;
metadata cost is included; complete V8v7 alternating A/B improves.

Primary source: V8's Memento Mori allocation-site optimizations,
<https://static.googleusercontent.com/media/research.google.com/en//pubs/archive/43823.pdf>.

