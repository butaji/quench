# 286 — Primary-source algorithm research, round eighteen

Status: complete

This pass searched Deegen, V8, JavaScriptCore, SpiderMonkey, LLVM, Higgs, and garbage-
collection sources for additional general algorithms under the standing contract: every
eligible function uses stencils from first execution, runtime code production is bounded
copy/patch/share, rustc/LLVM cooks the finite catalog AOT, and no source name, benchmark
identity, execution count, or hotness threshold selects an optimization.

## Deduplicated findings

1. **Replace object ownership before adding more allocation leaves (Task 148).** V8's
   mutator allocation primitive is only an aligned cursor-limit check plus cursor bump.
   A copied `NewObject` stencil over the current `Rc<ObjectCell> + Vec<Value>` design
   would still call Rust allocation and ownership machinery. The smallest sound cutover
   is the complete object-tag domain to stable non-moving handles and a precise STW heap;
   nursery/logical-version refinements remain Task 162.

2. **Fuse allocation with initialization only after the heap (Tasks 191 and 156).** The
   current residual corpus has repeated `New*; SetStatic*` construction blocks. Quote an
   immutable construction recipe, allocate its final storage once, then initialize in
   evaluation order. A self-source write (`src == allocation dst`) terminates freshness.
   This is a general freshness/effect pattern, not an object-literal source matcher.

3. **Treat shared semantic kernels as valid predicate producers (Task 271).** Task 285
   empirically found that copying a 176-byte complete `instanceof` cache checker regressed
   Earley-Boyer 18.44%, while a shared observation kernel plus 108/112-byte branch
   stencils improved it 1.53% focused and 2.60% in the full run. The reusable abstraction
   is `Kernel -> Predicate -> Branch`, compatible with copied predicate producers and
   avoiding dead Boolean materialization.

4. **Use a bounded two-level megamorphic cache (Task 159).** V8's stub cache maps
   `(map, name, access type)` to handlers with direct primary and secondary probes; primary
   victims move to the differently hashed secondary table. This bounds hit latency and
   code size. SpiderMonkey's `Specialized -> Megamorphic -> Generic` state machine gives
   the site transition discipline. Adopt stable shape/atom IDs and shared recipes, not
   owned values.

5. **Propagate minimorphic/typed shape facts (Tasks 152 and 154).** JSC distinguishes the
   useful case where multiple structures agree on one property offset. Higgs reports that
   typed shapes plus shape propagation eliminate 48% of type tests, reduce code size 17%,
   and reduce execution time 25%. One dominating shape-set/offset proof should feed a
   complete region; do not copy one guard per property bytecode.

6. **Pin the existing metadata plane (Task 203).** Sparkplug caches the feedback-vector
   pointer in its compatible frame because it is used by most operations. This VM's
   `InlineSite` array is already that single immutable metadata plane. Keep its base in the
   connector context and derive site fields; adding another per-op metadata representation
   would violate one-fact/one-representation and worsen memory traffic.

7. **Prioritize cross-operation state over more isolated leaves (Tasks 144, 152, 158).**
   Lazy basic-block versioning reports 71% of executed type tests removed and speedups up
   to 50%; its five-version limit grew mean code size only 0.19%. The implementation here
   remains compile-time worklist specialization—never runtime hot-path detection—and must
   lower each `(BlockId, ContextId)` to ordinary composable stencils.

## Ranked implementation order

`148 object-only heap -> 156 inline slots -> 191 construction recipes/grouped bumps ->
146 direct guest call/frame -> 171/144/152 region contexts and shape propagation ->
158 register residence -> 271 general kernel/stencil predicates -> 159 megamorphic cache`

Tasks 285 and 271 supply a safe small-step pattern while the heap/call/region work lands:
keep immutable expensive semantics in one kernel, keep mutable per-site facts in a compact
instance record, and copy only the control/data fragment whose specialization repays its
memory. This follows the Lisp staging rule: one quoted fact graph, pure pattern rewrites,
finite template selection, then one effectful link.

## Primary sources

- Deegen optimization inventory: <https://arxiv.org/abs/2411.11469>.
- V8 Sparkplug frame and feedback-vector design: <https://v8.dev/blog/sparkplug>.
- V8 linear allocation path:
  <https://chromium.googlesource.com/v8/v8.git/+/refs/heads/13.1.57/src/heap/main-allocator-inl.h>.
- V8 generational collector overview: <https://v8.dev/blog/trash-talk>.
- V8 megamorphic stub cache:
  <https://chromium.googlesource.com/v8/v8/+/0f581e4b99ee923e7ebae72e64ee58999ff74b5d/src/ic/stub-cache.h>.
- SpiderMonkey CacheIR and IC states: <https://firefox-source-docs.mozilla.org/js/cacheir.html>.
- JavaScriptCore shapes, minimorphism, ICs, and value layout:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- JavaScriptCore GC blocks and logical versions:
  <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>.
- Higgs lazy basic-block versioning: <https://arxiv.org/abs/1411.0352>.
- Higgs typed shapes and shape propagation: <https://arxiv.org/abs/1507.02437>.
- LLVM JITLink graph/fixup phases: <https://llvm.org/docs/JITLink.html>.
