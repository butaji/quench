# 266 — Primary-source algorithm research, round fourteen

Status: complete

This pass looked specifically for low-compilation-cost algorithms that fit the existing
stencil-only, rustc/LLVM-AOT architecture. It did not create alternative interpreters,
runtime assemblers, hotness gates, or benchmark-shaped whole-loop kernels.

## Ranked findings

1. **Register-cache states plus explicit edge transfer recipes.** V8 Liftoff keeps
   values in registers during its one-pass baseline compile, snapshots a canonical
   cache state at a merge, and emits a parallel transfer recipe for later predecessors.
   This is the closest production analogue to Task 158's categorical connector plan.
   Keep-source-register, reuse-an-equivalent-register, use-a-free-register, then spill is
   a concrete low-cost ordering. This becomes the first implementation slice of Task
   158; joins are handled by its existing post-allocation parallel-copy edge stencils.

2. **Lazy/static basic-block versioning.** Higgs reports that context-specialized block
   versions remove most repeated dynamic type tests without a costly global inference
   pass. The useful mechanism is not runtime hotness: `(BlockId, Gamma)` is interned,
   successor contexts are refined by guards, and a named per-block cap forces a lattice
   merge. This sharpens Tasks 144 and 152 and matches this VM's first-execution policy.

3. **Typed-shape propagation, not merely faster IC misses.** Shape guards should refine
   the region context so dominated same-object accesses consume the proof. The Higgs
   typed-shape extension reports lower type-test count, code size, and execution time.
   The direct next step is therefore Task 152 before adding more isolated property
   templates.

4. **Shared linear IC recipes.** SpiderMonkey CacheIR uses the normal form
   `Guard* ; Idempotent* ; Result`, separates per-stub fields from shared code, and folds
   recipes that differ only in shape fields. This confirms Tasks 153 and 154: one recipe
   is the immutable Kernel identity; shape/slot/callee data is instance state. It also
   gives a structural rule that no guard can follow an observable result.

5. **FPR-native value residence.** Current JavaScriptCore work explicitly avoids costly
   GPR/FPR round trips by loading numeric property/global/closure fields directly into
   FPRs. Task 165's raw F64 fields therefore need Task 158 connector variants whose
   categorical context records an FPR, rather than immediately boxing into a general
   register-array word.

6. **Allocation sinking requires exact materialization semantics.** JavaScriptCore uses
   must-points-to allocation sinking, including allocation graphs, but a recent fixed
   bug demonstrates the key hazard: an internal double-array hole sentinel must not be
   materialized as observable `NaN`. Task 176 must carry hole state as a distinct sum
   case through virtual objects and edge materialization.

7. **Bounded tail duplication comes after state specialization.** JSC/B3 duplicates
   small tails to flatten diamonds and expose optimization, while LLVM's link graph runs
   layout-dependent relaxation before fixups. This project already owns these concerns
   in Tasks 28, 157, 205, and 209. Task 205 showed that deleting adjacent branches alone
   loses; duplication should be reconsidered only after Tasks 144/158 can preserve the
   specialized register context through the duplicated tail.

## Resulting execution order

`171 canonical RegionPlan -> 158 register/cache-state planner -> 144/152 bounded typed
block versions -> 165 FPR-native fields -> 153/154 shared IC recipes -> 176 partial
escape -> 28/157 layout and bounded duplication`.

This order follows the Lisp staging rule: establish one quoted fact representation,
derive contexts and rewrite it purely, select finite cooked templates, then emit once.
It also attacks the currently measured memory traffic before more surface-level opcode
coverage.

Primary sources:

- V8 Liftoff design: <https://v8.dev/blog/liftoff>
- Liftoff cache-state and merge API:
  <https://chromium.googlesource.com/v8/v8.git/+/7b2b9233d6e1981781a1572e1cf935049ef06b0f/src/wasm/baseline/liftoff-assembler.h>
- Lazy basic-block versioning: <https://arxiv.org/abs/1411.0352>
- Typed shapes with basic-block versioning: <https://arxiv.org/abs/1507.02437>
- SpiderMonkey CacheIR: <https://firefox-source-docs.mozilla.org/js/cacheir.html>
- JavaScriptCore speculation and allocation sinking:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- JavaScriptCore direct-double field lowering:
  <https://commits.webkit.org/298092@main>
- Copy-and-Patch: <https://arxiv.org/abs/2011.13127>
- Deegen: <https://arxiv.org/abs/2411.11469>
- LLVM JITLink phases and fixups: <https://llvm.org/docs/JITLink.html>

