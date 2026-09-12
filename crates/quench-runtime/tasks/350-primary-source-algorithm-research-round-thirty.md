# 350 — Primary-source algorithm research, round thirty

Status: complete

Research additional VM/JIT algorithms against the implementation and the existing task
graph. The standing constraints remain: every function executes stencils/kernels from its
first invocation, runtime execution counts do not trigger a tier, V8v7 is a holdout rather
than a template-training corpus, and rustc/LLVM does expensive code generation only in the
AOT cooker.

## Findings and canonical homes

1. **Compiler-worklist basic-block versioning, not runtime-hotness specialization
   ([[144]], [[152]]).** Lazy BBV can propagate context-dependent representations while
   generating a block and reported eliminating 71% of type tests; Static BBV explores
   versions without execution profiles and reported about 10% average speedup with a
   two-version limit. Typed shapes add shape propagation so one dominating shape check
   feeds later fixed-offset accesses. This is the best fit for the no-hotness constraint.

2. **Single-forward-pass SSA with known facts ([[171]], [[158]], [[330]]).** Maglev's
   branch/loop-assignment/liveness prepass, pre-created loop phis, and known-node table are
   a practical small-compiler pattern. In this VM, static facts and already-existing IC
   facts seed the table; the result stays quoted until tiling and emission. This is more
   promising than adding more opaque per-opcode fragments because values can remain in
   registers across operations.

3. **Expose IC recipes as guard/pure/result operations ([[153]], [[145]], [[176]]).**
   SpiderMonkey CacheIR's restricted `Guard* ; Idempotent* ; Result` form and JSC's IC
   expansion make shape checks and field loads visible to CSE, scalar replacement, and
   allocation sinking. An IC should be one immutable recipe lowered either to shared
   kernel code or a patched inline instance, never a second semantic implementation.

4. **Interprocedural context transfer after call-boundary erasure
   ([[20]], [[144]], [[177]]).** Interprocedural BBV reported eliminating 94.3% of type-tag
   tests and speedups up to 56%. The transferable algorithm is bounded argument/return
   context propagation through inlined or exact direct calls—not its published numbers.
   Task 349 shows that a very narrow same-owner leaf subset is too small to assume this
   payoff; widen through the canonical call graph and call recipe rather than adding
   ad-hoc call opcodes.

5. **Choose `Kernel` sharing versus `StencilInstance` replication by cost ([[157]],
   [[189]], [[291]]).** Classic superinstruction/replication work shows that copying code
   can remove dispatch and improve branch prediction, while shared kernels save executable
   memory. These are two physical realizations of the same typed morphism. Add both to one
   tiling decision, charging call/branch seams and ABI spills against copied bytes and
   instruction-cache pressure; never hard-code one representation globally.

6. **Partial escape plus edge heapification ([[176]], [[191]]).** Modern speculative
   escape work materializes a stack/virtual object only when an edge invalidates the
   assumption. The existing partial-escape task is the correct home: an immutable virtual
   object graph is the fact, while materialization is an explicit side-exit stencil. Do
   not add a second stack-object representation.

7. **Native representation specialization ([[165]], [[265]], [[321]]).** Production JS
   engines separate named properties from indexed elements and specialize arrays as
   packed integers, packed doubles, tagged, and holey. Stable typed fields and dense raw
   numeric backings are prerequisites for C-like loops; fresh allocations also license
   write-barrier removal until publication.

8. **A nursery is a representation/ownership change, not an allocator micro-tweak
   ([[148]], [[162]], [[193]], [[320]]).** Generational collection makes allocation a
   cursor bump and reclaims young garbage in bulk. Precise composable root maps must come
   first. Task 299's neutral bump-first leaf does not falsify the nursery; it falsifies
   changing only the local allocation branch while retaining the same ownership system.

9. **Measure dependency chains, not instruction counts ([[331]]).** A 2025 JavaScript AOT
   study implemented dynamic IC code modification, reduced memory reads, and found no
   wall-time gain on contemporary processors. That matches this project's repeated neutral
   seam experiments. A candidate must shorten a serial load, indirect branch, call/frame,
   allocation, or arithmetic critical path before implementation priority is granted.

10. **Meta-derive the compiler from semantic declarations ([[149]], [[309]], [[347]]).**
    Deegen and Druid both support the one-semantics-source rule; Copy-and-Patch and the
    2025 R implementation support offline-cooked templates with a tiny online linker.
    Rust macros should derive kernel semantics, micro-op quotes, template declarations,
    effect metadata, and cooker-matrix tests from one opcode record.

## Priority produced by this research

The next structural order remains:

`20/317 call recipe + broader inlining -> 171/309 quoted SSA micro-ops ->`
`144/152 bounded context versions -> 158 register residence ->`
`157 total costed cover -> 165/265 native heap representations`.

Tasks 145/153 inline ICs are useful only when their recipe expands into that optimizer-
visible region. Tasks 162/176 are foundational engine work but should not displace the
call/frame and generic-executor boundaries currently dominating the measured profile.

No new execution mechanism is added by this research. Every finding maps to an existing
canonical task; this avoids turning the ledger into parallel names for the same fact.

## Primary sources

- Deegen (18 November 2024): <https://arxiv.org/abs/2411.11469>.
- Copy-and-Patch: <https://arxiv.org/abs/2011.13127>.
- Copy-and-Patch JIT for R (2025):
  <https://d3s.mff.cuni.cz/publications/kocourek_copyandpatch_2025/>.
- Lazy BBV: <https://arxiv.org/abs/1411.0352>.
- Static BBV: <https://doi.org/10.4230/LIPIcs.ECOOP.2024.28>.
- Typed shapes with BBV: <https://arxiv.org/abs/1507.02437>.
- Interprocedural BBV: <https://arxiv.org/abs/1511.02956>.
- V8 Maglev: <https://v8.dev/blog/maglev>.
- V8 fast properties and elements kinds: <https://v8.dev/blog/fast-properties> and
  <https://v8.dev/blog/elements-kinds>.
- SpiderMonkey CacheIR and MIR passes:
  <https://firefox-source-docs.mozilla.org/js/cacheir.html> and
  <https://firefox-source-docs.mozilla.org/js/MIR-optimizations/index.html>.
- JavaScriptCore speculation and structures:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- V8 generational collection: <https://v8.dev/blog/trash-talk>.
- LLVM GC statepoints: <https://llvm.org/docs/Statepoints.html>.
- Efficient interpretation using quickening:
  <https://doi.org/10.1145/1899661.1869633>.
- Superinstructions and replication:
  <https://www.complang.tuwien.ac.at/cd/papers/A73-full.pdf>.
- Meta-compilation with Druid: <https://arxiv.org/abs/2502.20543>.
- False lead of optimizing ICs: <https://arxiv.org/abs/2502.20547>.
- CoSSJIT speculative stack allocation: <https://doi.org/10.1145/3763149>.
