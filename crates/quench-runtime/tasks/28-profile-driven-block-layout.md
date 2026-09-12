# 28 — Profile-driven branch composition order

Status: planned

Lay out the quoted stencil CFG with an ext-TSP-style cost model. Direct-success arms and
loop backedges should fall through; slow exits should be outlined. Static semantic
probabilities provide a first-execution order, while feedback records from
[[26-pgo-morphism-selection]] may refine it without gating compilation. Permit bounded
tail duplication only for small tails when the removed branch/misfetch cost exceeds the
added code-size and I-cache cost. This changes composition order and controlled sharing,
not the connector type.

Acceptance: a branch with strongly skewed static or observed likelihood compiles with the
likely arm as fallthrough; loop slow exits are outlined; every duplicated tail is below a
named byte/instruction budget and has a recorded cost justification; an unprofiled
function still receives deterministic static layout; full-suite A/B does not regress.

Note on framing: this is standard profile-guided block layout (the same technique LLVM's block-layout pass and every optimizing JIT use); reordering `+` operands uses the category's existing composition operator but does not depend on any proven law to be correct — swapping composition order is safe regardless of category structure since both orders are semantically equivalent by definition. Do not cite this task as evidence of categorical payoff.

Source model: LLVM machine block placement exposes explicit costs for jumps, misfetches,
fallthroughs, tail duplication, and I-cache growth:
<https://llvm.org/docs/doxygen/MachineBlockPlacement_8cpp.html>.
