# 322 — Static ExtTSP stencil-block layout and bounded tail duplication

Status: planned

After [[157-costed-multi-granularity-stencil-tiling]] selects code atoms but before final
linking, order the closed CFG with an ExtTSP-style greedy chain algorithm. Inputs are
block byte sizes and deterministic semantic edge weights from [[209]]: lexical next,
loop backedge/body/exit, return, throw, guard failure, and generic slow continuation.
There are no runtime counters, training data, source names, or benchmark identities.

Start with one chain per block. Repeatedly choose the chain split/merge with the largest
positive distance-sensitive score increase. Prefer conditional common successors and
unconditional edges as fallthrough. Then perform bounded tail duplication only for
small, effect-equivalent blocks when the saved edge score exceeds copied-byte and
i-cache penalties. [[276]] owns hot/cold segmentation; this task orders within and
across those annotated segments without inventing another code representation.

All weights, maximum blocks, maximum split positions, maximum duplicated bytes, branch
range, and score coefficients are named constants in one architecture policy record.
Identity/fallthrough edges remain zero-byte categorical morphisms. Symbolic holes are
resolved only after the chosen order is final.

Acceptance: deterministic/property tests preserve CFG semantics and label resolution;
small CFGs compare against exhaustive layout; emitted branch/fallthrough counts and
distance score improve; copied bytes remain within budget; i-cache samples and complete
V8v7 A/B improve before default enablement.

Primary sources:
<https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Utils/CodeLayout.cpp>,
<https://arxiv.org/abs/1809.04676>, and
<https://www.llvm.org/docs/doxygen/MachineBlockPlacement_8cpp_source.html>.

