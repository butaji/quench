# 335 — Primary-source algorithm research, round twenty-seven

Status: complete

Research algorithms that can move the current 2133.61 V8v7 checkpoint materially while
preserving the project's fixed constraints: stencil execution from the first invocation,
no execution-count hotness tier, OXC plus Rust, rustc/LLVM-cooked templates, shared
immutable kernels, general bytecode-driven selection, and no third-party VM.

## Evidence before selection

Task 253's native samples attribute roughly 18--29% of several suites to
`dyn_block_step_impl`; call/frame construction, completion, and `Value` destruction add
another large fraction. Task 332 proved that removing 16 isolated local-load sites changes
the full aggregate by only 0.49%. The next algorithm must erase a region or function
boundary, not polish another seam.

## Ranked algorithms to try

### 1. SCC-bottom-up quote-level inlining

Build the immutable guest call graph, compute strongly connected components, and rewrite
eligible calls bottom-up. Within a recursive SCC, process original edges once before newly
exposed transitive edges. This is LLVM's production strategy for gaining bottom-up
simplification without superlinear recursive inlining growth.

For this VM, inlining is alpha-renaming and composition of `StencilExpr` data. Callee
locals become a disjoint caller-frame suffix, parameters/receiver become explicit moves or
aliases, returns become result moves plus symbolic jumps to the caller continuation, and
exceptions retain their explicit continuation. Kernels stay shared references;
templates stay cold until the final copy-and-patch link. Named byte, frame-slot, depth,
context, and SCC-iteration budgets make termination and code growth explicit.

First experiment: exact-target, noncapturing, exact-arity leaf calls. It is accepted only
if disassembly proves the successful edge contains no Rust `make_frame` or
`complete_dyn_frame`, affected call-heavy suites improve, and complete-suite A/B passes.
This refines Tasks 20, 288, 301, and 317 rather than creating another inliner.

### 2. Interprocedural bounded block versioning

After an edge is inlined, carry the caller's representation, exact-shape, and call-target
context through the callee and carry return facts back to the continuation. Intern
compiler demands by `(FunctionId, BlockId, ContextId)` and widen deterministically when a
named per-callee context budget is exhausted. For non-inlined calls, Task 177's summary is
the conservative transfer function.

The published interprocedural BBV evaluation eliminated 94.3% of type-tag tests on average
and reported speedups up to 56%. Those results establish plausibility, not an expected
speedup here. This refines Tasks 144, 163, and 177.

### 3. Recipe-first IC expansion and caller-local specialization

Normalize every attached IC arm into the same small quoted operation algebra before
optimization. Keep mutable shape, offset, and target words in a compact per-site data
plane; share cooked code for structurally equal recipes. A caller-local recipe can remain
monomorphic even when the callee's global behavior is polymorphic, allowing the inliner
and region optimizer to specialize the right context without source-specific rules.

CacheIR demonstrates both reusable native code through recipe matching and Trial Inlining,
which associates distinct IC sets with distinct caller contexts. In this VM the presence
of a semantic IC case may select the stencil immediately; there is no hotness threshold.
This refines Tasks 145, 153, 163, and 176.

### 4. Cold-use-aware allocation plus lazy edge reconstruction

Treat frame values required only by guard failure, exception, or general re-entry as cold
metadata uses, not hot live-range uses. Normal execution carries values in selected GPR/FPR
connectors. A side exit interprets persistent `FrameHint` deltas to materialize the
canonical frame once. Virtual objects similarly materialize only on escaping edges.

JavaScriptCore documents OSR-exit-aware register allocation and allocation sinking; V8
Maglev attaches frame states to deoptimizing nodes and separates tagged from untagged spill
regions. This is the required complement to inlining: otherwise the larger graph can keep
the removed frame logically live through spills. This refines Tasks 158, 176, and 330.

### 5. Preserve-none continuation ABI across every internal edge

Deegen uses LLVM's GHC calling convention for guaranteed tail calls, no callee-saved
registers, register-pinned VM state, and a custom guest stack. Its paper also identifies C
calling-convention register shuffling as an inherent remaining limitation. Therefore a
stencil path that still crosses a Rust/C ABI frame/helper boundary is not “Deegen-style”
merely because its outer bytes were copied from a template.

Audit the first experiment at machine-code level: internal stencil continuations must use
the preserve-none contract; only explicit kernel edges may adapt to the host ABI. This
reinforces Tasks 43, 146, 158, 181, 203, and 270.

## Lisp/category shape

There is one quoted program representation and one effectful emission edge:

`FunctionGraph -> SCC inline rewrite -> context-version rewrite -> IC expansion ->`
`SSA reduction/allocation -> stencil tiling -> copy+patch once`.

Each level is the same compositional idea at a different scale: instruction morphisms form
blocks, blocks form regions, regions form functions, and functions form a call graph.
Analyses derive immutable plans; they do not mutate executable bytes. Shared kernels are
references in the quote, while patched stencil instances are emitted only after the
rewrite fixed point. This preserves closure and hierarchy without pretending that a
thousand leaf stencils alone constitute whole-program optimization.

## Primary sources

- Deegen's copy-and-patch, GHCcc continuation, register-pinning, and custom-stack design:
  <https://arxiv.org/abs/2411.11469>.
- LLVM's bottom-up CGSCC inliner and within-SCC growth discipline:
  <https://llvm.org/docs/doxygen/Inliner_8cpp_source.html>.
- Interprocedural basic-block versioning:
  <https://arxiv.org/abs/1511.02956>.
- CacheIR code sharing and Trial Inlining:
  <https://doi.org/10.1145/3617651.3622979>.
- V8 Maglev's frame state, representation selection, and forward register allocation:
  <https://v8.dev/blog/maglev>.
- JavaScriptCore speculation, cold OSR uses, and allocation sinking:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- JavaScriptCore B3/Air register allocation and patchpoint location contracts:
  <https://webkit.org/blog/5852/introducing-the-b3-jit-compiler/>.

No external benchmark result is projected onto this VM. Every algorithm remains subject
to correctness, machine-code inspection, dynamic coverage, code-size, and alternating
complete-suite A/B gates.
