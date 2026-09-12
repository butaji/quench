# 414 — Primary-source optimization research, round forty-six

Status: complete

This round searched current primary sources for general algorithms that can improve a
stencil-only JavaScript VM without a runtime LLVM dependency, an interpreter fallback,
execution-count hotness, or V8v7-shaped code. The accepted checkpoint remains Task 385's
**2501.00** exact aggregate. Research alone does not change that score.

The main conclusion is sharper than “add more stencils.” Copy-and-patch is an emission
mechanism. C-like execution requires a small compile-time optimizer that derives a typed,
effect-aware region from the bytecode and then chooses precompiled physical forms. The
stencil catalog should grow from semantic/binding-time structure, while the linked image
should shrink through register equivalence, guard propagation, allocation sinking, and
strict code-size budgets.

## Ranked algorithms and patterns to try

1. **Alias-preserving multi-location register state (`158`, `385`).** Track an SSA value
   independently from the VM locals that name it. Several locals may denote one value and
   therefore share one machine register until an assignment separates them. A move updates
   the abstract name-to-value map and emits zero bytes; canonical frame stores occur only
   on an observing exit. Titzer's baseline-compiler study found both constant tracking and
   “multiple register allocation” to have large code-quality impact. This is the smallest
   new extension of the accepted Task 385 physical context and should be tried first.
2. **Binding-time-generated semantic templates (`149`, `309`, `394`).** Classify every
   semantic input as static-at-link (opcode, literal, local index, property key, CFG target,
   proved representation/shape/effect) or dynamic-at-execution. Generate reusable AOT graph
   templates from that classification, normalize them, and let rustc/LLVM cook their finite
   physical covers. The 2026 partial-evaluation-template work reports lower partial-
   evaluation and warmup time without peak loss; we borrow the binding-time/template
   algorithm, not GraalVM or a runtime compiler.
3. **Budgeted loop unswitching over assumption products (new `415`).** After LICM proves a
   tag, shape, element-kind, callee, or protector condition loop invariant, clone at most one
   small loop version so the successful body contains no repeated condition. The other arm
   is the total generic stencil/kernel loop. LLVM requires LICM first and applies explicit
   growth thresholds because unrestricted unswitching is exponential. This is a loop-level
   stencil composition rule, not a benchmark-specific whole-loop kernel.
4. **Typed-shape propagation with a hard two-shape context budget (`152`, `154`, `159`).**
   A successful property guard refines both receiver shape and field representation, so
   later property loads and calls can erase repeated guards and know the callee. The Higgs
   study measured its best bounded configuration at two propagated shapes; unlimited shape
   propagation caused pathological Splay code growth. Tasks 410–412 independently show the
   same local tradeoff: more copied PIC ways help Richards but lose globally. Keep one or two
   inline facts and send polymorphic overflow to a shared kernel.
5. **Must-points-to virtual allocation graphs (`176`, `191`).** Do not scalar-replace only
   isolated acyclic objects. Represent a nonescaping graph, forward fields through exact IC
   effects, and emit a graph materialization recipe only on an escaping side edge.
   JavaScriptCore documents production allocation sinking capable of eliminating cyclic
   graphs. This is the general form needed for temporary object networks and closure data.
6. **Bounded offline equality saturation/superoptimization (`394`).** Run it only over pure,
   typed semantic micro-op islands at build time. Extract Pareto-minimal recipes by target
   cost, bytes, register pressure, patches, and side-exit obligations; prove each candidate
   against the generic semantics before admitting it to the catalog. `egg` addresses rewrite
   phase-ordering, while Souper demonstrates synthesis of missing LLVM peepholes. Neither
   belongs in runtime or callable images.
7. **Function-image fitness and fragment-collapse budget (`157`, `291`, `322`).** Cost the
   whole selected cover, not each stencil independently. Reject a cover whose copied bytes,
   edge transfers, or continuation fragments exceed the shared-kernel alternative. A 2026
   CPython JIT issue records a real 18 MB pathological image from about 22 linked traces for
   one synthetic function. This validates a per-function budget and adjacent-fragment
   coalescing before adding more template variants.
8. **Flat, ID-indexed CFG plus copying reducers (`171`).** Keep blocks, values, uses, and
   effects in contiguous arenas and run forward copying reducers. V8 reports its CFG load-
   elimination implementation can be up to 190 times faster than the prior Sea-of-Nodes
   phase on large graphs, with materially better cache behavior. This is compile-time
   infrastructure, but it is what makes the analyses above cheap enough to run for every
   function from first execution.

## Execution order

1. Finish the general alias-preserving register context over Task 385 and prove zero-byte
   moves plus on-demand frame materialization in disassembly.
2. Finish Task 401's native property-load/callee/call/return region so propagated callee
   identity can remove the Rust call boundary.
3. Add two-shape propagation and field/callee facts; do not copy wider inline PICs.
4. Implement the first `415` loop-unswitch case using an invariant representation or shape
   guard and a named code-growth budget.
5. Add virtual allocation graphs once exact property effects are exposed.
6. Use binding-time generation and offline equality saturation to expand the catalog only
   after measurements identify a missing physical cover.

Every experiment first proves the intended erased operation in disassembly/counters, then
runs correctness and randomized alternating full-suite A/B. Source names, benchmark names,
source offsets, execution counts, and literal fingerprints are forbidden selection inputs.

## Primary sources

- Deegen's optimization inventory and single-semantics generation:
  <https://arxiv.org/abs/2411.11469>
- Copy-and-Patch variants and composition: <https://arxiv.org/abs/2011.13127>
- Baseline abstract state, constant tracking, multiple-register allocation, and tag
  materialization measurements: <https://arxiv.org/abs/2305.13241>
- Partial-Evaluation Templates (CGO 2026):
  <https://doi.org/10.1109/CGO68049.2026.11395215>
- `weval` whole-program partial evaluation: <https://arxiv.org/abs/2411.10559>
- Typed shapes, shape propagation, and the bounded `maxshapes=2` result:
  <https://arxiv.org/abs/1507.02437>
- V8's CFG/reducer rationale and measurements:
  <https://v8.dev/blog/leaving-the-sea-of-nodes>
- JavaScriptCore allocation sinking and cyclic allocation graphs:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- LLVM loop unswitching and its growth budget:
  <https://llvm.org/doxygen/classllvm_1_1SimpleLoopUnswitchPass.html>
- `egg` equality saturation and Souper synthesis:
  <https://arxiv.org/abs/2004.03082> and <https://arxiv.org/abs/1711.04422>
- CPython 3.15 copy-and-patch upgrades and the fragment-size failure report:
  <https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst> and
  <https://github.com/python/cpython/issues/149212>

