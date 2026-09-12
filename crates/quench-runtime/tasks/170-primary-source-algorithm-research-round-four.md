# 170 — Primary-source algorithm research, round four

Status: complete

Research compiler and VM algorithms not already represented by the stencil, IC,
context-versioning, call-ABI, and heap tasks. The constraint remains unchanged: every
function executes as a stencil image from first use; runtime code generation is only
copy-and-patch; selection never depends on benchmark identity or a hotness threshold.

## Result

The next useful abstraction is not another opcode family. It is one compact, immutable
SSA/CFG region value from which value flow, effects, ownership, loop recurrences, and
side-exit state are derived. V8's Maglev demonstrates that a liveness prepass followed by
a single forward SSA build can remain a fast compiler, while V8's move from Sea of Nodes
to Turboshaft is strong evidence that a conventional CFG is the simpler substrate for
control-sensitive facts and low-cost rewrites.

Ranked additions:

1. **Quoted SSA/CFG region normal form (Task 171).** This refines Task 68 away from a
   general Sea-of-Nodes migration. Keep block arguments/phis, explicit control edges,
   typed values, effect tokens, ownership, and canonical exits in one cold data value;
   discard it after the final stencil image is emitted.
2. **Ownership SSA (Task 172).** `Value` drop glue remains a visible accepted-profile
   cost. Swift's ownership SSA makes every nontrivial owned value exactly-once consumed,
   and makes copies explicit. Apply that discipline to derive last-use moves, borrows,
   and one destroy schedule before stencil selection instead of discovering ownership
   independently inside individual opcode helpers.
3. **Effect-token MemorySSA (Task 173).** Model heap reads as uses of a location and heap
   mutations/calls as explicit definitions. A clobber query can then reuse a previous
   property/element load or forward a store without pretending all calls alias all heap
   state. V8 reports its CFG load-elimination implementation as dramatically cheaper to
   compile than the former Sea-of-Nodes pass; LLVM's MemorySSA supplies the sparse
   clobber-query model.
4. **Sparse conditional value propagation (Task 174).** Run one bounded SCCP-style
   fixed point over constants, reachability, JS value representations, and known shapes.
   This removes dead blocks and selects cheaper stencil morphisms before tiling. It is a
   static first-use analysis, not execution-frequency profiling.
5. **Scalar evolution plus loop predication (Task 175).** Recognize affine induction
   recurrences and widen per-iteration bounds checks into one loop-entry envelope.
   ABCD reports removing 45% of dynamic bounds checks on average; LLVM's loop-predication
   implementation shows the direct `i < n` / `n - 1 < length` transformation. A failed
   envelope selects the general stencil loop, never an interpreter.
6. **Partial escape with edge materialization (Task 176).** Task 24 is currently
   all-or-nothing. Control-flow-sensitive partial escape keeps an allocation scalar on
   nonescaping paths and materializes only on escaping edges; published results report
   allocation reductions up to 58.5% and performance improvements up to 33%.
7. **Interprocedural semantic summaries (Task 177).** Compute bounded fixpoint summaries
   for capture, heap effects, throwing, allocation, argument ownership, and return
   representation. This lets direct calls preserve MemorySSA and ownership facts without
   inlining every callee. LLVM's Attributor is the relevant small-kernel pattern: several
   interacting attributes converge through one fixpoint driver.

## Order

Do not interrupt Task 146's direct-continuation call work: it attacks the largest current
boundary. Build Task 171 as the common analysis substrate, then implement the smallest
profile-relevant slices in this order:

`146 -> 171 -> 172 -> 177 -> 173 -> 174 -> 175 -> 176`

Task 172 comes first because retain/release/drop cost is already visible and its laws can
be verified locally. Task 177 precedes heap-load elimination so ordinary direct calls do
not unnecessarily kill every effect fact. Loop predication follows the value/effect
analyses needed to prove its envelope. Partial escape comes last because correct
materialization depends on canonical side-exit state and ownership.

## Rejections and guardrails

- Do not build a broad Sea-of-Nodes or retain an optimization graph at runtime. The graph
  is quoted compile-time data and is consumed by the final stencil linker.
- Do not reward a rewrite for fewer IR nodes. Cost measures dynamic loads, stores,
  guards, branches, calls, ownership operations, and copied code bytes.
- Do not use ABCD's optional hot-check selection. Run a bounded static query for every
  eligible dense access in a selected loop.
- Do not treat `Rc` operations as implicit compiler details. `copy`, `borrow`, `move`,
  and `destroy` are explicit recipe operations until Task 148 replaces `Rc` with a heap.
- Every iteration, graph-size, context, alias-walk, and materialization limit is a named
  policy constant. No unexplained numeric threshold is permitted.

## Primary sources

- V8 Maglev SSA, liveness, known-node information, and register allocation:
  <https://v8.dev/blog/maglev>
- V8's CFG/Turboshaft rationale and load-elimination experience:
  <https://v8.dev/blog/leaving-the-sea-of-nodes>
- Sparse conditional constant propagation:
  <https://doi.org/10.1145/318593.318659>
- LLVM MemorySSA: <https://www.llvm.org/docs/MemorySSA.html>
- Swift ownership SSA: <https://github.com/swiftlang/swift/blob/main/docs/SIL/Ownership.md>
- ABCD bounds-check elimination:
  <https://research.ibm.com/publications/abcd-eliminating-array-bounds-checks-on-demand>
- LLVM loop predication:
  <https://www.llvm.org/docs/doxygen/LoopPredication_8cpp_source.html>
- LLVM scalar evolution:
  <https://llvm.org/doxygen/classllvm_1_1ScalarEvolution.html>
- Partial escape analysis:
  <https://ssw.jku.at/Research/Papers/Stadler14/Stadler2014-CGO-PEA.pdf>
- LLVM Attributor fixpoint framework:
  <https://llvm.org/docs/doxygen/structllvm_1_1Attributor.html>

