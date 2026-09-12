# 269 — Primary-source algorithm research, round fifteen

Status: complete

This pass searched current production VM, compiler, and copy-and-patch sources for
mechanisms missing from the 268-item ledger. It deliberately rejected benchmark-shaped
recognizers, runtime hotness gates, a second interpreter, and runtime LLVM. The useful
findings all fit the existing Lisp staging boundary:

`quoted RegionPlan -> pure typed selection/rewrite -> StencilExpr -> one emit/link effect`

## Ranked findings

1. **The internal continuation ABI must preserve nothing.** Every stencil tail-calls and
   never returns to its predecessor, so the C ABI's callee-save contract creates work
   with no semantic consumer. LLVM's `preserve_nonecc` exists specifically for chained
   tail calls and uses more general registers for arguments. CPython identifies this as
   an important part of Copy-and-Patch; Clang exposes it on AArch64 and x86-64. Task 270
   isolates the ABI conversion. It is a prerequisite for the full Task 158 register-
   resident planner: selecting register-valued connectors while retaining a C ABI would
   encode contradictory facts.

2. **Predicate results are a representation, not necessarily JS values.** CPython's
   current JIT work reports that producing a one-bit predicate and letting a following
   guard consume it substantially shrinks compare/branch stencils. V8's instruction
   selector carries a `FlagsContinuation` whose mode decides whether the comparison
   branches, selects, deoptimizes, traps, or materializes a result. Task 268 correctly
   fuses one numeric family, but the general solution is Task 271: a typed predicate or
   flags context composed with branch/select/materialize consumers. Materialize a JS
   Boolean only when it is observably used as a value.

3. **Generate the selector from the same pattern data as the catalog.** LLVM/MLIR use
   declarative typed DAG patterns and compile static pattern benefits into efficient
   matchers; BURS encodes minimum-cost tree covers in a bottom-up automaton. Task 157
   already asks for a minimum-cost cover, but a hand-grown sequence of Rust matchers
   would duplicate semantic knowledge and make candidate-order bugs inevitable. Task
   272 makes stencil recipes the one fact and derives both the AOT macro expansion and
   the typed selection automaton from it.

4. **`-O3` is not automatically the right stencil-cooking pipeline.** CPython's current
   AOT stencil builder uses `-Os` because `-O2/-O3` include transformations appropriate
   to standalone functions—such as target alignment and tail duplication—that can be
   harmful when snippets are later laid out end-to-end. This is not permission to
   switch blindly. Task 273 measures an isolated `O2/O3/Os/Oz` matrix using extracted
   code shape, semantic tests, and alternating V8v7 A/B, then pins one named pipeline.

5. **Optimize loop iteration two, then split entry from steady state.** LuaJIT/PyPy's
   loop-aware algorithm unrolls a trace twice, runs ordinary forward optimizations, and
   emits the first optimized iteration as a preamble and the second as the steady-state
   loop. This lets ordinary guard elimination, virtual-object propagation, and load
   elimination see backedge facts without implementing bespoke loop variants of every
   pass. Task 196 already contains the correct general first-iteration/steady-state
   transform; this source sharpens its implementation to self-compose once and compare
   the resulting abstract backedge context.

6. **Make lifetime cleanup a separate, optimizable effect.** CPython now leaves inputs
   for explicit `POP_TOP` operations so its optimizer can remove unused cleanup; it is
   also moving instruction-pointer/invalidation work inside the rare deallocation arm.
   Task 172 already has explicit `DestroyValue`; the refinement is to keep semantic
   producers non-consuming, derive cleanup from ownership SSA, and attach observable
   state synchronization only to a destroy that can actually invoke a finalizer or GC.

## Revised execution order

The research does not overturn round fourteen. It inserts two necessary foundations and
one cheap measurement:

`171 RegionPlan -> 203 pinned context -> 270 preserve-none ABI -> 158 register planner`

In parallel, run Task 273 before generating a much larger catalog. Then implement
`271 predicate contexts -> 272 generated typed selector -> 157 general tiling`. Task
196's two-iteration steady-state construction and Task 172's explicit cleanup become
ordinary RegionPlan rewrites after that foundation exists.

## Primary sources

- LLVM `preserve_nonecc` and `musttail` rules:
  <https://llvm.org/docs/LangRef.html#calling-conventions>.
- Clang `preserve_none` attribute:
  <https://clang.llvm.org/docs/AttributeReference.html#preserve-none>.
- Rust's current `extern "custom"` RFC permits naked assembly shims but does not give a
  Rust function body a compiler-known preserve-none convention:
  <https://rust-lang.github.io/rfcs/3980-extern-custom.html>.
- CPython Copy-and-Patch code-quality plan:
  <https://github.com/python/cpython/issues/115802>.
- CPython 3.15 register-allocation and optimizer results:
  <https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>.
- CPython one-bit predicate/branch design:
  <https://github.com/python/cpython/issues/149238>.
- V8 `FlagsContinuation` representation:
  <https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/compiler/backend/instruction-selector.h>.
- CPython's current stencil cooker and its `-Os` rationale:
  <https://github.com/python/cpython/blob/main/Tools/jit/_targets.py>.
- LLVM generated DAG instruction-selection patterns:
  <https://llvm.org/docs/CodeGenerator.html>.
- MLIR declarative DAG rewrites and statically compiled benefits:
  <https://mlir.llvm.org/docs/DeclarativeRewrites/> and
  <https://mlir.llvm.org/docs/PatternRewriter/>.
- BURS optimal bottom-up tree matching:
  <https://doi.org/10.1016/0096-0551(90)90006-B>.
- PyPy/LuaJIT two-iteration loop optimization:
  <https://pypy.org/posts/2011/01/loop-invariant-code-motion-1998392217676829154.html>.
- CPython explicit cleanup optimization:
  <https://github.com/python/cpython/issues/145866> and
  <https://github.com/python/cpython/issues/152106>.
