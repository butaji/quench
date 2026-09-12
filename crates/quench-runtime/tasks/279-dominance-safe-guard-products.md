# 279 — Dominance-safe guard products and implication elimination

Status: planned

Normalize compatible speculative checks into one immutable `GuardProduct`. A guard atom
names its predicate, required facts, pure operands, success refinement, canonical failure
`FrameState`, and failure continuation. Sort/deduplicate atoms by semantic identity and
derive implication from the canonical shape/tag/range/protector lattices; never maintain
an independent table of redundant opcode combinations.

Within one dominance region, replace compatible repeated guards with the conjunction at
the earliest safe point and one failure branch. A later atom is erased when the path
facts already imply it. Movement is legal only when every required calculation is pure,
no Task 173 effect token is crossed, and all merged failures materialize the same Task
164 state and enter the same semantic continuation. Otherwise the original guards remain
separate. This avoids changing which JS effect or exception is observed.

Lower the normalized product through rustc-cooked tag/shape/range comparison templates
and Task 271 predicate continuations. The product is an ordinary morphism from a weaker
context to a refined context; composition remains associative because normalization is a
pure quotient rewrite before final emission.

Use named `MAX_GUARD_PRODUCT_ATOMS`, `MAX_GUARD_HOIST_BLOCKS`, and
`MAX_GUARD_PRODUCT_CODE_BYTES` constants. The cost model charges extra predicate work on
paths that previously exited early, so fewer branches alone is not sufficient evidence.

Acceptance: law/property tests cover permutation normalization, implication, identical
and distinct exit states, effects between guards, exceptions, NaN/minus-zero, shape
invalidation, and loop backedges; disassembly shows fewer conditional branches and no
duplicate tag/shape loads in accepted regions; counters reconcile atoms merged/erased;
full V8v7 alternating A/B improves.

Primary sources:

- LLVM GuardWidening's dominance, safety, profitability, and range-check machinery:
  <https://llvm.org/doxygen/GuardWidening_8cpp_source.html>
- V8's path-condition branch/deopt elimination:
  <https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/compiler/branch-elimination.cc>

