# 333 — Primary-source algorithm research, round twenty-six

Status: complete

Research additional algorithms after [[329]]/[[332]] proved that isolated local-load
elimination has negligible V8v7 coverage. The standing contract remains: stencil execution
from the first invocation, no execution-count hotness threshold, no benchmark identity,
finite rustc/LLVM-cooked templates, immutable shared kernels, and complete-suite A/B.

## Result: optimize regions, not seams

The sources converge on four high-leverage patterns. Three already have canonical work
items, so this review refines them instead of creating duplicate representations.

1. **Single-forward-pass typed SSA plus register residence ([[171]], [[158]], [[330]]).**
   V8 Maglev uses a liveness/loop-assignment prepass, pre-creates loop phis, constructs SSA
   in one forward pass, remembers known shape/type facts, and resolves parallel gap moves
   after allocation. For this VM, bytecode/static/IC facts select the initial variants; no
   hotness counter is required. This is the direct answer to Task 329's sparse payoff:
   remove frame traffic and checks across whole regions, not only duplicate loads inside a
   few blocks.

2. **Expand IC results into optimizer-visible pure operations ([[145]], [[155]], [[176]]).**
   JSC treats ICs as “negative-cost profiling”: they speed baseline execution and expose
   precise structure/offset cases. Once a case is expanded into `CheckShape ; LoadOffset`
   or `StoreOffset`, CSE, scalar replacement, and allocation sinking can see through it.
   Watchpoint/dependency cells make stable facts constants until the exact mutation event.

3. **Plan loop alternatives as immutable data ([[39]], [[157]], [[196]], new [[334]]).**
   LLVM VPlan explicitly separates Legal, Plan, and Execute. Candidate vector widths,
   interleave/unroll factors, predication, scalar remainders, and SESE regions remain cold
   descriptions until one plan is materialized. This exactly matches quote -> rewrite* ->
   eval-once and prevents unsafe vectorization from leaking into emission.

4. **Cost code layout and branch removal against I-cache pressure ([[322]], [[331]]).**
   LLVM machine block placement combines block frequency, fallthrough creation, tail
   duplication, and explicit code-size penalties. Static loop depth and CFG edge weights can
   seed this without runtime heat. A smaller stencil count alone is not a profitability
   proof, as the current neutral measurements demonstrate.

The new bounded algorithm is [[334]]: distribute a loop only when doing so isolates a
vectorizable/hoistable pure component without changing JavaScript effect order. Register-bank
selection is added to [[158]] rather than split into another allocator, and IC-enabled
allocation sinking is added to [[176]] rather than creating a second escape analysis.

## Primary sources

- Deegen, submitted 18 November 2024: <https://arxiv.org/abs/2411.11469>.
- V8 Maglev SSA, known-node information, representation selection, and parallel moves:
  <https://v8.dev/blog/maglev>.
- V8 Sparkplug's baseline frame-layout and shared-builtin design:
  <https://v8.dev/blog/sparkplug>.
- JavaScriptCore speculation, structures, ICs, watchpoints, and local optimization:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- JavaScriptCore IC expansion enabling allocation elimination:
  <https://webkit.org/blog/10298/inline-caching-delete/>.
- LLVM's Legal/Plan/Execute vectorization-plan architecture:
  <https://llvm.org/docs/VectorizationPlan.html>.
- LLVM loop vectorization, runtime alias checks, reductions, SLP, and interleaving:
  <https://llvm.org/docs/Vectorizers.html>.
- LLVM machine block placement and costed tail duplication:
  <https://llvm.org/docs/doxygen/MachineBlockPlacement_8cpp_source.html>.
- LLVM GlobalISel register-bank selection and combiners:
  <https://llvm.org/docs/GlobalISel/Pipeline.html>.

No external speedup is projected onto this VM. These are implementation hypotheses subject
to the same correctness, disassembly, dynamic-coverage, and alternating A/B gates.
