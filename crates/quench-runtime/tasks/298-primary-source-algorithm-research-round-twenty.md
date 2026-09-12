# 298 — Primary-source algorithm research, round twenty

Status: complete

This pass searched primary papers, production-engine documentation, and engine source for
additional algorithms compatible with the standing constraints: stencil execution from
the first invocation, no interpreter fallback or execution-count hotness gate, finite
rustc/LLVM-cooked templates, immutable shared kernels, copy/patch/share instances, and no
benchmark identity in selection.

In Lisp terms, the result is a deduplicated set of data transformations over the existing
semantic, context, frame, heap, and relocation records. A finding was assigned to an
existing task whenever that task already owned the fact. Only two missing bounded work
items were added.

## Ranked experiments

1. **Make allocation a literal bump leaf (Task 299).** The current object allocator probes
   `free_cells` through `RefCell` on every allocation even when the active chunk has an
   untouched linear tail. Use `cursor != limit` as the only common-path condition; enter a
   cold recycled-cell/chunk kernel only after exhaustion. MMTk and Immix identify the
   increment-and-limit-test allocation path as the important mutator primitive. This is
   the smallest immediately testable change for allocation-heavy Splay.

2. **Finish the POD guest stack and direct continuations (Tasks 146, 163, and 181).** The
   current native profiles attribute 13.3–31.7% of self samples in the object-heavy suites
   to call/frame work. V8's single-frame actual-argument-count design and Sparkplug's
   compatible frame demonstrate the relevant pattern: one pointer-bump frame, fixed slots,
   and direct call/return edges. The first useful coarse morphism is an inherited-method
   dependency guard followed by a direct guest call; prior bytecode fusion was neutral
   because it retained the Rust call boundary.

3. **If collection remains visible, use block-local marks and lazy sweeping (Task 300).**
   JavaScriptCore clusters mark/new bits in block metadata, advances logical versions
   instead of clearing every block, and reconstructs a free list for one active block when
   allocation needs it. Adapt that algorithm to stable non-moving handles: O(1)
   address-to-block marking, one mark epoch, and allocation-driven block sweeping. This is
   one alternative representation of Task 148's collector state, not a second heap.

4. **Then add a non-moving sticky nursery (Task 162).** A young-allocation log, survivor
   bit, and remembered old-to-young edges let minor collections visit young objects rather
   than retracing a large stable graph. Sticky Immix supplies the in-place algorithm, which
   preserves embedded stable handles. This becomes worthwhile only after Task 148's full
   collector is correct and measured counters show marking/sweeping dominates.

5. **Propagate contexts across whole blocks (Tasks 144, 152, 171, and 279).** Lazy
   basic-block versioning reports removing 71% of executed type tests in its JS VM, and
   typed-shape propagation reports a 25% execution-time reduction in the same research
   line. Here, every version remains a `StencilInstance`; the first semantic context may
   select a bounded version immediately, with monotone lattice merging at the fixed
   budget. No counter, hot tier, or interpreter is introduced.

6. **Put fields in the stable object cell and fold construction (Tasks 156 and 191).** V8
   and JavaScriptCore use in-object slots to avoid a backing-store load. Once layout size
   is known, dominance-based allocation folding can reserve several fresh cells with one
   check and omit barriers until publication. Both transformations derive from the same
   immutable allocation recipe and freshness state.

7. **Wire true IC slabs, bounded PICs, and minimorphic folding (Tasks 145, 153, 154, and
   159).** A hit should execute a shape/dependency guard and fixed-offset load/call inside
   the composed image. Multiple shapes that prove the same offset share one result arm;
   budget overflow enters a shared megamorphic kernel. Deegen and JavaScriptCore support
   this pattern. Merely patching metadata loads is not sufficient evidence of a win.

8. **Generate LLVM-visible supernodes, rather than concatenating more tiny leaves (Tasks
   149, 157, and 272).** Enumerate bounded legal semantic compositions from the one macro
   source, normalize operand alias patterns, cook the whole body through rustc/LLVM, and
   select a minimum-cost cover. Dynamic-superinstruction work reports substantial gains
   when whole instruction bodies are combined; this repository's Task 284 is complementary
   negative evidence that isolated atoms surrounded by generic boundaries do not pay.

9. **Complete canonical key and data representations (Tasks 11, 190, 200, 204, and 265).**
   Syntax/static property keys should be compact atoms; shape-churning objects should
   monotonically enter a dictionary representation; arrays should widen through packed
   numeric/tagged and holey/sparse states; typed mutable environment cells should preserve
   raw numeric recurrence values. These are representation state machines, not scattered
   opcode cases.

10. **Keep relocation and layout refinements behind boundary removal (Tasks 189, 205,
    209, 274, 276, 289, and 290).** Near-code kernel islands, fallthrough relaxation,
    hot/cold segments, tail merging, and partial prefix inlining can reduce branch and
    instruction-cache cost. They do not erase expensive Rust semantic or call boundaries,
    so measure them after the direct allocation/call/property paths exist.

## New work versus existing work

Only Tasks 299 and 300 are new. Direct calls, IC slabs, static context versions, shape
propagation, a sticky nursery, precise root maps, in-object slots, allocation folding,
atoms, array kinds, and generated supernodes already had canonical owners. This pass
refines their order rather than forking those facts into parallel designs.

The recommended next measurement order is:

`148 adaptive-GC A/B -> 299 bump leaf -> 146 direct guest frame -> 163 inherited direct`
`call -> 145/153 IC slab -> 144/152 context regions -> 156/191 allocation layout`.

Task 300 replaces the collector metadata/sweep experiment only if Task 148 counters show
full sweep work remains material. Task 162 follows if repeated traversal of old live
objects, rather than eager sweeping, is the dominant GC cost.

No score claim changed during this research pass.

## Deliberately deferred or rejected priorities

- Do not redo NaN boxing. `RawValue` is already one 64-bit word and numbers are immediate;
  pointer compression would add a heap cage/base-register contract before the larger
  call/property boundaries are removed.
- Do not add an interpreter, runtime LLVM optimizer, or execution-count tier. Bounded
  semantic-case IC transitions and context versions still execute stencils on every arm.
- Do not begin with concurrent collection. This VM is single-threaded and throughput-bound;
  a concurrent marker would add synchronization and a barrier to the mutations that the
  current work is trying to make cheap.
- Do not treat fewer IC metadata loads, fewer copied bytes, or a cleaner disassembly as an
  accepted optimization. The existing negative IC and property-tiling experiments require
  complete alternating A/B evidence.

## Primary sources

- Deegen: <https://arxiv.org/abs/2411.11469>.
- Copy-and-Patch: <https://arxiv.org/abs/2011.13127>.
- MMTk allocator design: <https://www.mmtk.io/assets/pubs/mmtk-sigmetrics-2004.pdf>.
- Immix: <https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf>.
- Sticky Immix: <https://www.steveblackburn.org/pubs/papers/consrc-oopsla-2014.pdf>.
- JavaScriptCore GC block metadata, logical versions, and sweeping:
  <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>.
- V8 Sparkplug frames: <https://v8.dev/blog/sparkplug>.
- V8 argument-adaptor-frame removal: <https://v8.dev/blog/adaptor-frame>.
- JavaScriptCore structures, PICs, minimorphism, and method calls:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- Lazy basic-block versioning: <https://arxiv.org/abs/1411.0352>.
- Typed shapes and shape propagation: <https://arxiv.org/abs/1507.02437>.
- V8 fast properties: <https://v8.dev/blog/fast-properties>.
- V8 elements kinds: <https://v8.dev/blog/elements-kinds>.
- V8 identifier internalization: <https://v8.dev/blog/scanner>.
- Superinstructions and replication:
  <https://www.complang.tuwien.ac.at/cd/papers/A73-full.pdf>.
- LLVM JITLink linking/fixup/relaxation phases: <https://llvm.org/docs/JITLink.html>.
