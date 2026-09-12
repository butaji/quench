# 403 — Primary-source optimization research, round forty-two

Status: complete

This round expands the algorithm inventory without creating a second optimizer or a
benchmark-shaped stencil catalog. The accepted Task 397/399 profiles remain the source of
priority: `dyn_block_step_impl`, call-containing residual blocks, repeated type/shape
guards, and frame cleanup dominate. Every candidate below is therefore expressed as a
pure rewrite or a compatible cover over the existing quoted `RegionPlan`, followed by one
copy-and-patch link effect.

## Algorithms worth trying

1. **CacheIR-style semantic recipes (`153`, `309`).** Firefox's CacheIR makes the IC
   generator the single source of truth and lets later tiers transpile the same small guard
   and effect vocabulary. Define property, call, dense-element, and coercion behavior once
   as immutable micro-ops; interpret it in shared kernels or tile it into stencils. This is
   the strongest defense against interpreter/JIT semantic drift and gives the larger
   residual-region selector composable pieces without one stencil per source pattern.
2. **Minimorphic shape equivalence (`154`, then `159`).** JavaScriptCore groups different
   shapes that place a property at the same offset, performs one set-membership guard, and
   emits one load. Normalize observed IC cases by `(prototype dependency, slot, access
   effect)` rather than by shape identity alone. Different offsets remain a bounded PIC;
   overflow uses one shared megamorphic kernel. Shape observation is cache population, not
   a heat threshold.
3. **Bounded context-driven block versions (`144`, `171`).** Lazy BBV specializes a block
   for facts already known on the incoming edge and reports eliminating 71% of type tests;
   its interprocedural extension reports 94.3%. For this always-stencil VM, enumerate only
   statically demanded `Representation × Shape × Location × EffectVersion` contexts under
   named caps. The generic kernel is the total final cover.
4. **Typed-shape propagation (`152`, `368`).** Higgs reports 48% fewer type tests, 17%
   smaller code, and 25% lower execution time. Carry shape and property representation as
   context facts through effect-safe edges, using explicit prototype/shape epochs. This
   turns repeated property ICs in Richards/DeltaBlue into one guard followed by direct
   offset operations.
5. **Interprocedural context continuation (`401`, enabled by `402`).** Interprocedural BBV
   shows why a call must not erase every known fact. Compose a whole
   `prefix ; call ; continuation` morphism, materializing only the values required by the
   callee recipe or an exceptional exit. Do not revive rejected call-only block splitting.
6. **MemorySSA-lite, forwarding, and LICM (`173`, `329`, `195`).** Give each exact JS heap
   location an effect-token version. A cached clobber walk licenses store-to-load
   forwarding, redundant-load elimination, dead stores, and loop-invariant guard/load
   hoisting. Unknown calls conservatively advance affected domains; immutable kernels and
   pure arithmetic preserve them.
7. **Allocation and store sinking (`176`).** LuaJIT combines store-to-load forwarding with
   side-exit materialization: a nonescaping object remains virtual fields on the fast path
   and is allocated only on an edge where identity becomes observable. This is a better
   match for dynamic-language fallback edges than whole-function escape analysis.
8. **Register-resident connector planning (`158`, `184`, `385`).** Sparkplug benefits from
   bytecode-level register allocation but still materializes frame state around builtins.
   Use liveness to select pre-cooked stencil variants whose categorical contexts keep a
   bounded set of values in physical registers; color the remaining VM frame slots. This
   remains copy-and-patch: rustc/LLVM generates every variant at build time.
9. **Costed multi-granularity tiling (`157`).** Superinstructions reduce dispatch, but this
   VM already copies native code. Use a dynamic-programming cover over primitive, fused,
   block, loop, and kernel bricks whose cost includes helper calls, guards, ownership
   operations, spills, branches, code bytes, and patch count. A larger brick wins only when
   LLVM cooked it as one unit or it removes a measured seam.
10. **Static loop-aware layout (`209`, `241`, `28`).** Use CFG loop depth and exceptional
    edge class—not runtime heat—to make loop continuations and common structural arms
    fall-through, outline semantic failures, and keep shared kernels out of copied code.
    Category laws define safe reassociations; layout chooses only law-equivalent forms.

## Recommended experiment order

Finish the bounded activation-pool gate, because it is required state ownership for native
recursive calls. Then implement `401` and measure whether complete call-containing regions
remove the dominant Richards/Crypto semantic-kernel entries. Next implement the smallest
common recipe slice of `153/309` plus `154`; this makes shape facts first-class data usable
by `144/152`. Only after region coverage materially grows should `173/329`, `158/385`, and
`157` be attempted, because their benefit is proportional to the amount of code that stays
inside one composable region. Allocation sinking follows once snapshots and effect tokens
can materialize virtual objects soundly.

No candidate uses a benchmark name, source offset, property spelling, execution counter,
or hotness threshold. Named caps bound code size and compile work. Rejected experiments
remain recorded so the same low-granularity shapes are not retried under new names.

## Primary sources

- Copy-and-Patch Compilation: <https://arxiv.org/abs/2011.13127>
- Deegen: <https://arxiv.org/abs/2411.11469>
- V8 Sparkplug: <https://v8.dev/blog/sparkplug>
- Firefox CacheIR: <https://firefox-source-docs.mozilla.org/js/cacheir.html>
- JavaScriptCore speculation and minimorphic ICs:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- V8 fast properties: <https://v8.dev/blog/fast-properties>
- Lazy BBV: <https://arxiv.org/abs/1411.0352>
- Interprocedural BBV: <https://arxiv.org/abs/1511.02956>
- Typed object shapes: <https://arxiv.org/abs/1507.02437>
- LuaJIT allocation sinking:
  <https://github.com/tarantool/tarantool/wiki/LuaJIT-Allocation-Sinking-Optimization>
- LLVM MemorySSA and loop passes: <https://llvm.org/docs/MemorySSA.html> and
  <https://llvm.org/docs/Passes.html#licm-loop-invariant-code-motion>

