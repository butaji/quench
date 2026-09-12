# 380 — Primary-source optimization research, round thirty-eight

Status: complete

Research the next general-purpose optimization trials after Task 378, constrained to OXC
plus Rust, rustc/LLVM-cooked copy-and-patch stencils, shared immutable kernels, execution
through stencils from first invocation, no third-party VM, no hotness threshold, and no
V8v7-shaped selection. The accepted exact comparison entering this review is 2293.53 ->
2326.86, +1.45% with a 95% interval of `[+0.67%, +2.13%]`.

## What the primary sources say

1. **Deegen's performance unit is an IC-capable semantic component, not copied bytes.**
   Its baseline system combines pinned VM state, specialized bytecodes, outlined slow
   paths, call/generic polymorphic ICs, inline IC slabs, and hot/cold layout. This confirms
   that the next work must erase complete native/kernel/native boundaries; adding a leaf
   that still calls a generic Rust helper is not the mechanism described by the paper:
   <https://arxiv.org/abs/2411.11469>.
2. **CacheIR provides the right shared recipe normal form.** A recipe is guards, then
   idempotent operations, then exactly one result; per-site fields such as shapes and
   slots are distinct from baked immediates. One recipe can feed baseline stubs and a
   later optimizing tier. That maps directly to Tasks 145 and 153 and prevents property,
   call, and lexical ICs from becoming unrelated hand-written selectors:
   <https://firefox-source-docs.mozilla.org/js/cacheir.html>.
3. **Lexical access should be context plus depth/slot, and mutable cells need a type
   state.** V8's context-specialization reducer folds a known outer context and direct
   slot; its mutable-heap-number work tracks context slots as constant, Smi, Int32,
   HeapNumber, or generic. Task 378 now supplies direct addressability. Task 204 is the
   next representation step for captured/global numeric state:
   <https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/compiler/js-context-specialization.h>
   and <https://v8.dev/blog/mutable-heap-number>.
4. **A compact SSA/CFG tier is justified only when it keeps facts across operations.**
   Maglev uses a liveness/loop prepass, forward abstract interpretation, known-node facts,
   representation selection, and a simple forward register allocator. Its central win is
   unboxing and reusing guards across a region, not re-encoding each bytecode. This
   reinforces Tasks 144, 152, and 171 after mixed-region coverage becomes total:
   <https://v8.dev/blog/maglev>.
5. **Allocation must be an inline machine operation.** WebKit reports a roughly 40%
   improvement on its relevant JetStream 3 WasmGC subtests from contiguous header-plus-
   payload objects, then inlines the bump-pointer fast path while keeping exhaustion in a
   slow helper. The reported number is not a projection for this VM, but the dependency
   chain is directly applicable to Tasks 191, 321, and 369:
   <https://webkit.org/blog/17899/introducing-the-jetstream-3-benchmark-suite/>.
6. **Dense elements need their own representation lattice.** V8 separates indexed
   elements from named properties and tracks packed/holey plus Smi/double/tagged kinds.
   Direct vector access and stable element facts should therefore precede generic
   computed-property cleverness. This reinforces Tasks 12 and 165:
   <https://v8.dev/blog/elements-kinds> and <https://v8.dev/blog/fast-properties>.
7. **The linker should own layout, relocation, and publication exactly once.** LLVM
   JITLink models content and relocations as a graph, performs pruning/layout before
   allocation, applies fixups after final addresses exist, and finalizes permissions at
   the edge. We do not need ORC or JITLink as a runtime dependency; Task 348 should copy
   this phase separation into the small Rust linker:
   <https://llvm.org/docs/JITLink.html>.
8. **The external score establishes scale, not a target-specific recipe.** The benchmark
   repository currently lists optimizing production engines around 44k--46k and GraalJS
   around 8k on its published platform, while this VM's same-run accepted estimate is
   2326.86. Hardware/build differences make those rows non-comparable measurements, but
   they demonstrate that 10000 requires region-scale optimization, not forty independent
   one-percent leaves: <https://github.com/ahaoboy/js-engine-benchmark>.

## Local evidence changes the ordering

Task 373 measured 20,570,009 residual helper entries. Task 377 found call alone can close
4,060,419 entries, call plus name 5,502,818, and computed plus bitwise plus numeric-unary
977,987 under unit costs. Task 378 then demonstrates why realization matters: eager name
snapshots lost up to 5.84%, while a lazy use-site IC gained 1.45%. Static reach must be
multiplied by the dependency chain actually removed and charged for setup, code bytes,
guards, ownership misses, and new boundaries.

The Lisp-shaped canonical pipeline remains:

```text
DynCode
  -> immutable RegionPlan<MicroOp, Effect, Context>
  -> pure rewrite to a fixed point
  -> costed cover of Kernel | StencilTemplate(bindings)
  -> one two-pass materialization
  -> immutable FunctionImage
```

IC observations update POD site data or choose among already quoted general recipes. They
do not introduce a hotness counter or a second semantic definition.

## V8v7 as validation strata, not training input

The upstream sources expose distinct reusable stresses. Use them to attribute a measured
change, never to select generated code by suite identity:

| Suite | Dominant reusable pressure visible in source | Mechanism to validate |
|---|---|---|
| Richards | constructor allocation, mutable object fields, repeated method/task calls | shape/property/call IC continuum; inline allocation |
| DeltaBlue | dense arrays plus object graphs and virtual-looking method calls | element kinds, property-call recipes, type/shape context |
| Crypto | packed integer arrays, shifts, masks, `Math.floor` | I32/bitwise micro-ops, typed elements, unboxed loop state |
| RayTrace | short-lived numeric objects, property arithmetic, computed copy loop | fixed fields, allocation grouping, numeric region facts |
| Earley-Boyer | closures, recursive calls, tagged list objects, `instanceof`, strings | lexical/call continuum, allocation, coproduct/tag dispatch |
| RegExp | compiled patterns, string slicing/concatenation, matching | shared matcher kernels and string views/ropes |
| Splay | tree-node allocation, property mutation, callback traversal | inline allocation, shapes, direct callback calls |
| Navier-Stokes | long numeric loops over dense arrays | F64/I32 element backings, bounds/shape hoisting, loop regions |

This map is derived from the unmodified V8v7 programs mirrored by the benchmark
repository. It explains why a whole-suite result is required: an optimization aimed at
one dependency family must not silently tax unrelated strata.

## Ranked trials

1. **Rerun the residual frontier with Task 378's load capability and physical sample.**
   Distinguish direct immediate/traced-object hits from ownership slow arms. This is the
   smallest step needed to avoid choosing from stale Task 373 structure.
2. **Complete dense computed access together with bitwise and numeric-unary leaves**
   (Tasks 12/165/157). The existing joint frontier is nearly one million entries and the
   array representation already has a packed-number fact.
3. **Build one fused property-load plus direct-call IC recipe/slab** (Tasks 145/153), but
   only after the refreshed frontier proves the surrounding block remains native. The
   warmed arm must be shape check, slot load, callee check, guest-frame link, direct entry,
   and native return continuation with no Rust callback.
4. **Make mixed regions total and remove physical per-leaf PC state**
   (Tasks 309/157/316/348/366). A general kernel is a legal coarse morphism; it must not
   demote an otherwise native prefix and suffix or force site materialization internally.
5. **Propagate typed context through those continuous regions**
   (Tasks 171/144/152/204). Keep numeric loop-carried and mutable lexical values unboxed,
   reuse one shape/type guard, and materialize generic state only at a side exit.
6. **Inline bump allocation and contiguous initialization**
   (Tasks 191/321/369), prioritized by allocation counters from Earley-Boyer and Splay.
7. **Only then tune static layout and cooker output** (Tasks 322/347/367). Linker/cooker
   work must remove an observed instruction or relocation seam, not merely produce a
   cleaner artifact.

## Explicit non-trials

- No hot-path detector or tier threshold: specialization remains first-use IC state or
  static bounded context expansion.
- No eager activation snapshots for ordinary names: Task 378 measured the tax directly.
- No new helper-calling isolated call leaf: Tasks 352/353/362 already measured that seam.
- No pointer compression now: values are already one machine word, and the current gap is
  execution coverage and dependency length rather than heap capacity.
- No benchmark-trained superinstruction catalog: V8v7 remains a holdout. Candidate
  patterns must come from the semantic grammar and a separate conformance/training corpus.

Every retained limit and ABI fact remains a named constant. Every experiment needs a
line/block reach preflight, disassembly showing the removed boundary, correctness tests,
and Task 365's exact randomized paired gate.

## First implementation outcome

Task 381 implemented trial 2 in independently gated slices. General direct dense
computed access passed at +1.57% `[+0.03%, +3.36%]`. The follow-up isolated bitwise and
numeric-unary leaves failed at -3.45% `[-8.65%, +0.49%]` and were reverted. Typed elements
remain high priority, but I32 transforms must compose inside a context-carrying region
instead of paying standalone double-to-int and int-to-double conversions.
