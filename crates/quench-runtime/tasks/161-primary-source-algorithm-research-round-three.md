# 161 — Primary-source algorithm research, round three

Status: complete

Research additional algorithms that can move the always-stencil VM toward C-like
execution without adding a third-party VM, a runtime LLVM dependency, an interpreter
fallback, a hotness threshold, or benchmark-specific code. Treat mechanisms as quoted,
composable data and prefer transformations that erase a measured boundary across a
region over another exact bytecode-pattern handler.

## Current evidence

The accepted Task 160 Richards sample (`reports/task161-richards-accepted.sample.txt`)
still attributes the largest top-of-stack counts to the generic block step (958),
`run_with_locals` (254), closure execution (214), `Value` drop glue (165), local release
(71), call argument setup (57), and `DynJitCode::call` (54). Property helpers, environment
lookups, equality helpers, allocation, and hashing remain visible but smaller.

This says the next work must remove whole boundaries: repeated generic semantic dispatch,
Rust-owned call frames, frame-memory traffic, and per-value ownership. Shortening one
already-direct leaf cannot plausibly close the current 1713-to-10000 gap.

## New algorithms and adaptations

1. **Canonical side-exit state (Task 164).** Maglev attaches abstract interpreter frame
   state to every deoptimizing node, and JavaScriptCore uses compact stackmaps for large
   numbers of side exits. The stencil adaptation is a persistent `FrameState` value plus
   a composable materialization morphism from a specialized context into the canonical
   generic stencil frame. This is the missing safety mechanism for keeping values
   unboxed or register-resident across a region. The exit target is a generic stencil,
   never an interpreter.
2. **Caller-customized stencil images (Task 163).** Self customization, interprocedural
   BBV, and SpiderMonkey Trial Inlining all avoid mixing unrelated caller types into one
   callee profile. Give each bounded monomorphic call site its own callee context/image,
   with parameters already typed and the return continuation specialized. This is
   first-use context specialization, not hot-path detection.
3. **Non-moving nursery allocation (Task 162).** A VM heap removes `Rc` traffic, but a
   plain whole-heap mark/sweep collector still makes allocation and collection expensive.
   JavaScriptCore uses fixed-size cell blocks, local free lists, generational collection,
   sticky mark bits, and write barriers. Adapt the smallest subset: bump/free-cursor
   allocation inside size-class blocks, an eden epoch, sticky marks, and a remembered
   set. Stable handles remain valid because cells do not move.
4. **Typed property-field representations (Task 165).** Typed shapes should describe not
   only property presence and slot number but also slot representation. V8 uses shape
   transitions and lazy migration when a field widens from an integer to a double or
   generic value. Combined with context propagation, a property region can load/store a
   raw representation without repeating tag checks; the widening path is a shared
   migration kernel.
5. **Static ext-TSP-style layout and bounded tail duplication (Task 28 refinement).**
   LLVM's machine block placement explicitly trades fallthroughs against I-cache growth
   and duplicates only tiny tails. Apply the same cost model to composed stencil blocks:
   direct-success arms and loop backs fall through, slow exits are outlined, and only
   small identity/materialization tails may be duplicated. Static semantic likelihoods
   work on first execution; runtime counts may refine order but never gate compilation.
6. **IC health scoring (Task 166).** SpiderMonkey scores IC recipes to expose expensive
   and generic stubs. Record each site's recipe, arm count, fallbacks, helper calls,
   property depth, and executed recipe cost. Aggregate by normalized recipe, not source
   benchmark. This turns the improvement routine into `profile -> highest-cost missing
   recipe -> general stencil vocabulary -> A/B`, instead of guessing another pattern.

## Important refinements to existing tasks

- **Task 146:** use one variable-sized VM frame with the actual argument count in a named
  slot; do not allocate/copy an argument-adaptor frame. V8 reports this improved Richards
  and EarleyBoyer in Octane, which directly matches the current profile.
- **Task 144:** representation selection must include `I32`, `F64`, tagged word, object
  with shape, and dense-element views. Phi/join contexts choose a common representation;
  conversion happens at joins or side exits, not at every opcode.
- **Task 153:** keep CacheIR's useful normal form: zero or more guards, zero or more pure
  idempotent loads, then exactly one effect/result. Recipe fields are data beside shared
  kernel code unless baking a field demonstrably removes a hot load or branch.
- **Task 157:** price a candidate tile by dynamic operations removed—loads, stores,
  guards, calls, retains/releases, and taken branches—plus code bytes. Do not reward
  fusion merely for reducing the count of stencil nodes.
- **Task 87:** because this VM always uses stencils, compile a general RegExp matcher
  template on first use rather than adding a RegExp interpreter tier. V8's native matcher
  data still shows why removing the JS-to-runtime transition matters.

## Ranked implementation order

`145 -> 146 -> 164 -> 144 -> 152 -> 158 -> 163 -> 148 -> 162 -> 156 -> 165 -> 153 -> 154 -> 166 -> 157 -> 28 -> 159`

The order follows the measured boundaries. Inline slabs and the call ABI remove the
largest current helper/frame seams. Canonical side exits make larger typed contexts safe.
The VM heap and nursery then remove ownership/allocation costs without invalidating code
pointers or shape guards.

## Rejected or deferred ideas

- A Sea-of-Nodes IR is not the right next structure. V8 reports high revisit cost and
  moved its JavaScript backend toward CFG IR; bounded context-versioned CFG regions fit
  the copy-and-patch architecture better.
- Full concurrent GC is unnecessary for the single-threaded MVP and would add barriers
  and synchronization before a basic tracing heap exists. Generational, stop-the-world,
  non-moving collection is the useful first performance step.
- Pointer compression is primarily a memory/locality optimization and requires a cage.
  The VM already has a word-sized tagged value; remove `Rc` and stabilize the heap before
  considering compressed handles.
- Runtime BOLT or LLVM optimization violates the copy-and-patch runtime boundary. LLVM
  PGO/function specialization may improve AOT cooking later, using a general training
  corpus rather than V8v7, but it does not replace region specialization.

## Primary sources

- Lazy basic-block versioning: <https://doi.org/10.4230/LIPIcs.ECOOP.2015.101>
- Interprocedural BBV: <https://arxiv.org/abs/1511.02956>
- Typed shapes and shape propagation: <https://arxiv.org/abs/1507.02437>
- Self customization: <https://doi.org/10.1145/74818.74831>
- Self polymorphic inline caches: <https://research.google/pubs/optimizing-dynamically-typed-object-oriented-languages-with-polymorphic-inline-caches/>
- SpiderMonkey CacheIR: <https://firefox-source-docs.mozilla.org/js/cacheir.html>
- SpiderMonkey Warp and Trial Inlining: <https://hacks.mozilla.org/2020/11/warp-improved-js-performance-in-firefox-83/>
- V8 Maglev representation selection, frame state, and register allocation: <https://v8.dev/blog/maglev>
- V8 argument-adaptor removal: <https://v8.dev/blog/adaptor-frame>
- V8 typed field transitions and lazy shape migration: <https://v8.dev/blog/react-cliff>
- JavaScriptCore speculative exits and patchpoints: <https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/>
- JavaScriptCore allocator and generational GC: <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>
- JavaScriptCore sticky-mark generational collector: <https://webkit.org/blog/7122/introducing-riptide-webkits-retreating-wavefront-concurrent-garbage-collector/>
- LLVM machine block placement and tail duplication: <https://llvm.org/docs/doxygen/MachineBlockPlacement_8cpp.html>
- LLVM function specialization: <https://llvm.org/doxygen/FunctionSpecialization_8h_source.html>

