# 151 — Primary-source algorithm research, round two

Status: complete

Research additional VM algorithms that fit this project's constraints: every function
executes as stencils from first use, rustc/LLVM cooks templates ahead of time, runtime
codegen is copy-and-patch only, and no benchmark identity or hotness threshold may affect
selection. Convert only mechanisms that remove costs visible in profiles into tasks.

## Findings

The next layer should not be another list of exact bytecode patterns in
`select_direct_block_template`. It should be a small quoted algebra that can express
guards, pure loads, results, contexts, and costs; general algorithms then derive concrete
stencil instances from that data.

Ranked by expected benefit and fit with current profiles:

1. **Finish inline IC slabs and the direct VM call ABI (Tasks 145 and 146).** Richards
   still spends most of its residual time in property/call blocks and Rust call-frame
   machinery. JavaScriptCore identifies removal of dispatch plus comprehensive PICs as
   its baseline JIT's two main wins. These two tasks remove boundaries rather than only
   shortening helpers.
2. **Propagate typed shapes through context-versioned regions (Task 152).** The typed
   shapes/shape-propagation paper reports 48% fewer type tests, 17% less code, and 25%
   lower execution time. This directly addresses why Task 138's individually guarded
   property stencil regressed: a region should pay the shape guard once.
3. **Keep live values in registers across continuation-compatible stencils (Task 158).**
   The Copy-and-Patch algorithm explicitly plans register use and selects pass-through or
   spill variants. The accepted Richards profile still counts `LoadLocal` 1,416,455 times
   in a 20 ms diagnostic run, so a fixed frame-only connector leaves a large C-vs-VM gap.
4. **Make IC specialization a quoted recipe algebra, then fold equivalent stubs (Tasks
   153 and 154).** SpiderMonkey's CacheIR restricts recipes to guards, idempotent pure
   operations, and one final result; the same recipe can drive multiple tiers and share
   code. Its stub-folding pass merges cases that differ only in shape into a multi-shape
   guard. This is the Lisp/data-first replacement for hand-written selector branches.
5. **Use monotone runtime fuses for stable global/prototype facts (Task 155).** A single
   byte can represent a conjunction of invariants; mutation irreversibly pops it and
   invalidates dependent instances. This supports safe direct array/builtin/property
   stencils without repeating a prototype-chain proof inside each loop iteration.
6. **Allocate common fixed-layout objects with in-object slots (Task 156).** Both V8 and
   JavaScriptCore put common properties at fixed offsets inside the object and spill only
   overflow properties. The current `PropertySlots` always owns an independent vector,
   so even tiny objects require allocation plus an extra pointer load.
7. **Select across all stencil granularities with a costed tiler (Task 157).** The catalog
   already contains leaves, fused blocks, traced loops, and shared kernels, but a growing
   sequence of exact `if let` patterns is not a general selector. A bounded dynamic
   program over quoted regions can choose a minimum-cost cover while preserving the same
   `StencilExpr` output type.
8. **Add a shared megamorphic fallback cache only after slabs/PIC folding (Task 159).**
   Sites that exceed the bounded shape/target set should not grow code forever or return
   immediately to string-keyed lookup. A shared `(shape, atom)` lookup kernel bounds code
   and metadata while retaining a faster general case.

## Important constraints and negative evidence

- None of these tasks adds an interpreter, warmup counter, hot-loop detector, or runtime
  LLVM dependency. First observation may patch an IC or choose a context version, but
  every execution remains inside a stencil instance or shared kernel.
- Stub state changes are driven by **distinct semantic cases observed**, not execution
  frequency: `Empty -> Mono -> FoldedShapeSet -> Mega` is a finite data transition.
- Vmgen reports only modest benefit for static superinstructions when their main effect
  is removing dispatch. Since this VM already concatenates native code, Task 157 must
  value removal of loads, guards, calls, spills, or ownership operations—not stencil
  length by itself.
- A 2025 negative study of dynamic IC code modification found that reducing memory
  accesses alone did not reduce execution time on its tested modern hardware. Task 145
  therefore requires disassembly and profiles proving that an unpredictable/helper
  boundary disappears, not just a smaller instruction count.
- Allocation sinking, scalar replacement, broad equality saturation, polyhedral loops,
  and SIMD remain valid later work. They should follow the stable heap, direct call ABI,
  and context propagation because those are prerequisites or larger measured costs.

## Implementation order

`145 -> 146 -> 152 -> 158 -> 156 -> 153 -> 154 -> 155 -> 157 -> 159`

Task 144 is the common prerequisite for Tasks 152 and 158. Task 148 should be developed
before or alongside Task 156 so inline allocation targets a VM heap rather than deepening
the temporary `Rc` object model.

Every experiment uses the standing protocol: structural selection counters, semantic
tests, complete V8v7 smoke, alternating A/B, component floors, and exact binary identity.
Named constants define all version, arm, code-size, and register budgets.

## Primary sources

- Typed shapes and shape propagation: <https://arxiv.org/abs/1507.02437>
- Interprocedural BBV and continuation specialization:
  <https://arxiv.org/abs/1511.02956>
- Copy-and-Patch register planning and stencil variants:
  <https://arxiv.org/abs/2011.13127>
- JavaScriptCore structures, minimorphism, PICs, and allocation sinking:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- SpiderMonkey CacheIR, stub folding, and shared stub code:
  <https://firefox-source-docs.mozilla.org/js/cacheir.html>
- SpiderMonkey fuses and allocation sites:
  <https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>
- V8 in-object slots and inline allocation: <https://v8.dev/blog/slack-tracking>
- V8 element-kind lattice: <https://v8.dev/blog/elements-kinds>
- Vmgen and superinstructions: <https://doi.org/10.1002/spe.434>
- Dynamic IC modification negative result: <https://arxiv.org/abs/2502.20547>

