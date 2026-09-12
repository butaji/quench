# 275 — Primary-source algorithm research, round sixteen

Status: complete

This pass searched current V8, JavaScriptCore, CPython JIT, Higgs/BBV, and
Copy-and-Patch sources for algorithms that fit the project's constraints: compile every
eligible function on first load, use no runtime hotness gate, keep one semantic source,
and emit only rustc/LLVM-cooked kernels plus copied-and-patched stencils.

## Ranked result

1. **Build Task 171's compact CFG/SSA quote stage now.** V8 reports that its current CFG
   representation halves compilation time relative to Sea of Nodes, has roughly one
   third the L1 data-cache misses during compilation, and makes forward state tracking
   dramatically cheaper. The relevant lesson is representation, not another tier:
   `DynCode -> immutable RegionPlan -> pure reducers -> StencilExpr -> emit once`.

2. **Compose Tasks 144, 152, 158, and 214 over that one representation.** Lazy basic
   block versioning eliminated 71% of executed type tests in Higgs; typed shapes plus
   shape propagation eliminated 48% of tests, reduced code size 17%, and reduced
   execution time 25%. The project's eager/no-hotness variant must enumerate the finite
   reachable `(Block, Context)` product at load time, with named widening/version limits.
   Register locations, shapes, and ranges are fields of one context—not independent
   passes or a Cartesian stencil explosion.

3. **Treat side exits as terminal analysis edges (Task 164 refinement).** V8 and JSC
   exploit deoptimization because an exit block contributes no loop-carried data-flow
   facts. A failed assumption materializes canonical stencil state and terminates that
   specialized morphism; it must not poison the successor context. This is what makes
   checked-to-unchecked arithmetic, hoisted shape guards, and loop bounds elimination
   tractable.

4. **Use forward effect-state reduction and demand-driven load placement (Task 173
   refinement).** V8 reports its CFG load elimination can be up to 190× faster than the
   former graph algorithm. Keep location-keyed state in block order, forward stores to
   loads, remove dead stores, and sink a load into the only successor that consumes it
   when no intervening effect aliases it. This avoids doing both branch-only loads on
   every path.

5. **Complete Task 146's custom VM call stack before retrying preserve-none.** Task 270
   proved the whole-function ABI adapter loses 0.75% because the current recursive Rust
   call architecture pays it at every JS call. V8's compatible-frame and adaptor-frame
   removal designs, plus the project's own Richards result, point to direct guest
   continuations and one variable-sized frame as the high-impact prerequisite.

6. **Physically split hot template bodies from cold obligations.** CPython reports an
   early 1–2% gain from a separate cold stencil section. Task 209 gives LLVM branch
   weights, but does not by itself deduplicate copied error/coercion/IC-miss bodies.
   Task 276 adds the missing two-segment linker representation.

7. **Keep static roots and near-code shared trampolines in the implementation queue.**
   V8's static-root offsets and semantic address clustering eliminate root-table loads;
   CPython's AArch64 work reuses trampolines and improves C-call layout. These are already
   represented by Tasks 189 and 198 and should not become duplicate abstractions.

## Recommended implementation order

`171 minimal RegionPlan -> 214 range/tag facts -> 144/152 bounded context versions ->
158 register locations -> 164 terminal materializers -> 173 load/store reduction ->
146 direct call stack -> 276 hot/cold physical layout`

Every stage remains quoted immutable data. Reducers compose as ordinary functions and
the sole effect remains final stencil linking. No item selects by source name, benchmark,
execution count, or runtime temperature.

## Primary sources

- V8, *Land ahoy: leaving the Sea of Nodes*:
  <https://v8.dev/blog/leaving-the-sea-of-nodes>.
- V8, *Maglev — V8's Fastest Optimizing JIT*:
  <https://v8.dev/blog/maglev>.
- V8, speculative inlining and terminal deoptimization edges:
  <https://v8.dev/blog/wasm-speculative-optimizations>.
- JavaScriptCore speculation, typed checks, watchpoints, and side exits:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- Higgs lazy basic block versioning: <https://arxiv.org/abs/1411.0352>.
- Higgs typed shapes and shape propagation: <https://arxiv.org/abs/1507.02437>.
- CPython Copy-and-Patch code-quality plan:
  <https://github.com/python/cpython/issues/115802>.
- CPython 3.15 JIT results and mechanisms:
  <https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>.
- Copy-and-Patch stencil variants and supernodes:
  <https://arxiv.org/abs/2011.13127>.
