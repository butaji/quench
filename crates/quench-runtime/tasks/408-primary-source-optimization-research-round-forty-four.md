# 408 — Primary-source optimization research, round forty-four

Status: complete

This round searched for additional algorithms after the accepted Task 368 checkpoint and
the rejected Task 407 activation lease. The accepted score remains **2401.58**, or 24.02%
of the 10000 gate. No new benchmark result was produced by this read-only research pass.

The important result is an ordering correction. Modern optimizing VMs do not get C-like
numeric code merely by keeping tagged words in registers. They first choose a machine
representation, fuse memory loads with validation/unboxing, keep values in that
representation across a region, and reconstruct boxed state only on observable exits.
The current Task 385 scaffolding has the first and last shape of a register ABI, but its
lanes still carry F64 bits in `u64` values and bitwise leaves reconvert every operation.
It is also not linked into execution. That explains why it cannot change V8v7 yet.

## Ranked algorithms and their existing homes

1. **Representation-typed register contexts (`385`, `158`, `165`).** Lower the same quoted
   SSA region to `I32Context`, `U32Context`, `F64Context`, or `TaggedContext`. An entry
   stencil guards/unboxes once; interior templates use `w` or `d` registers; an exit
   materializes once. This is the first implementation target because Crypto's Word32
   loops and Navier-Stokes' F64 loops require different physical categories.
2. **Load-with-unbox and box-with-store fusion (`213`, `316`).** Use one canonical
   `MemoryRef` and cook producer/consumer forms such as `load_local_i32`,
   `load_dense_f64`, and `store_property_f64`. This removes the temporary tagged value,
   virtual-register publication, and cross-bank move. It is a stencil-template family,
   not an ad-hoc peephole over emitted bytes.
3. **Cold-use-aware state reconstruction (`330`, `164`, `172`).** A value required only
   by a slow/deopt exit must remain reconstructible but must not force a hot-path spill.
   Record constants, locations, ownership, and boxing recipes as zero-code frame-state
   hints; the exit adapter performs the effect once. JavaScriptCore explicitly treats
   OSR stackmap inputs as cold register-allocation uses.
4. **Cache-state variants and parallel edge transfers (`158`, `385`).** CPython 3.15's
   copy-and-patch JIT validates the finite-variant pattern: each operation has forms for
   its input/output cached-register count. Generalize that count to this VM's typed
   `ValueLocationMap`; first predecessor fixes a join context, and later predecessors
   emit one parallel-copy recipe. Identity transfers emit no bytes.
5. **Late edge-case analysis (`385`, `175`, `279`).** Delay overflow, NaN, negative-zero,
   and bounds obligations until representation selection and code motion finish. Prove
   which observations require each distinction, then place one guard at the earliest
   dominating boundary. Do not repeat a conservative check inside every arithmetic leaf.
6. **Composite invariant fuses (`155`, `229`, `368`).** Compile a conjunction of realm,
   prototype, property, iterator, and species assumptions into one monotone byte cell or
   one dependency registration. This makes guarded array/builtin/property regions pay a
   single O(1) check; the mutation operation owns the one-way invalidation effect.
7. **Typed mutable binding cells (`204`).** Use the monotone
   `Constant -> I32 -> F64 -> Tagged` state for captured/global numeric slots. Compatible
   updates mutate the owned numeric payload in place; widening invalidates dependent
   code. V8 reports this exact pattern eliminating allocation and enabling integer code.
8. **Uniqueness as an ownership refinement (`172`, `176`).** A proven-unique heap value
   permits refcount elimination and in-place updates. CPython 3.15 now reports both from
   one uniqueness analysis. Keep it as `Ownership { unique }` in the existing SSA facts,
   not a second optimizer.
9. **Semantic builtin macro/kernel duality (`153`, `212`, `309`).** One typed semantic
   recipe lowers either to an in-region macro stencil or to an immutable shared kernel.
   Inline only when it removes a measured call/boxing seam; otherwise reference the one
   kernel. V8's Torque distinction between inlined macros and shared builtins is the
   physical model, while CacheIR supplies the guard/pure/result effect discipline.
10. **Root-relative constants and near-code kernels (`203`, `189`, `406`).** Pin the VM/
    heap/root base once, load immutable roots by named offsets, and select direct/page-
    relative address forms at final link. This reduces indirections without copying a
    shared kernel per function.

## The single data pipeline

Do not build ten optimizers. Extend the canonical quote with facts:

```text
RegionPlan
  -> RepresentationDemand
  -> ValueLocationMap
  -> AssumptionSet
  -> ReconstructionMap
  -> PhysicalCover
  -> link once
```

Each arrow is a pure derivation. The quoted region remains the one semantic fact; register
locations, guards, fuses, ownership, and exit recipes are derived views. Rust macros turn
the finite physical vocabulary into LLVM-cooked `StencilTemplate`s. Final linking is the
only byte mutation. Immutable `Kernel`s and patched `StencilInstance`s expose the same
typed connectors and therefore remain composable at operation, block, loop, function,
and call-region levels.

Categorically, representation selection maps one semantic morphism into a coproduct of
physical covers. A successful guard injects execution into the chosen `I32`, `U32`, or
`F64` subcategory; the total generic kernel is the final coproduct arm. Edge reconstruction
is a natural transformation back to canonical tagged-frame state. Composition must preserve
the representation/location object, not merely the number of machine arguments.

## Next experiments

1. Finish Task 385's link path, but first replace raw-F64 `u64` lanes with actual typed GPR/
   FPR contexts and stop canonical write-through inside the region.
2. Prove on disassembly that a Crypto bitwise chain has one entry conversion, native `w`
   operations, and one exit materialization; prove a Navier-Stokes chain stays in `d`
   registers. Reject before benchmarking if either proof fails.
3. Measure selection, conversions, spills, frame writes, copied bytes, and slow exits, then
   run the full randomized alternating gate against Task 368.
4. If Task 385 exposes memory operations as the next seam, implement Task 213's fused
   load/unbox forms. If exits cause spills, implement Task 330 first. The counters decide.
5. Only after numeric regions are physical, advance composite fuses (`155`/`229`), typed
   binding cells (`204`), and call/builtin recipe lowering (`153`/`309`).

No experiment uses a benchmark name, source offset, property spelling, hotness threshold,
execution counter, runtime LLVM, or interpreter fallback.

## Primary sources

- Deegen, first submitted 18 November 2024: <https://arxiv.org/abs/2411.11469>
- V8 Maglev representation selection and register allocation:
  <https://v8.dev/blog/maglev>
- SpiderMonkey MIR optimization inventory:
  <https://firefox-source-docs.mozilla.org/js/MIR-optimizations/index.html>
- SpiderMonkey CacheIR and composite fuses:
  <https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>
- JavaScriptCore speculation, watchpoints, cold OSR uses, and allocation sinking:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- CPython 3.15 JIT upgrades:
  <https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>
- CPython register-cached stencil variants:
  <https://github.com/python/cpython/issues/135379>
- V8 mutable numeric binding cells: <https://v8.dev/blog/mutable-heap-number>
- V8 generated low-level builtins and custom register ABI: <https://v8.dev/blog/csa>
- V8 Torque macro/builtin split: <https://v8.dev/docs/torque>
- V8 pointer compression/root register: <https://v8.dev/blog/pointer-compression>
- V8 embedded and near-code builtins: <https://v8.dev/blog/embedded-builtins> and
  <https://v8.dev/blog/short-builtin-calls>
- LLVM loop strength reduction and loop transforms: <https://llvm.org/docs/Passes.html>
- LLVM GlobalISel phase separation: <https://llvm.org/docs/GlobalISel/Pipeline.html>
