# 364 — Primary-source algorithm and measurement research, round thirty-five

Status: complete

Find additional general-purpose experiments that can move the stencil-only VM toward a
verified V8v7 score of 10000. Preserve the standing constraints: OXC plus Rust, no
third-party VM, no interpreter fallback, no runtime hotness threshold, no source or
benchmark identity in selection, and one semantic quote lowered to rustc/LLVM-cooked
`StencilTemplate` instances or shared immutable `Kernel` values.

## What the current implementation already has

Do not repeat obsolete advice. The accepted runtime already has an eight-byte tagged
`RawValue`, canonical shapes and fixed property slots, raw traced-object handles, bump
allocation, dense-array views, property ICs, and monomorphic call recipes. The remaining
gap is not “add NaN boxing” or “invent hidden classes.” It is that the facts those systems
produce are not carried through one continuous native region: depending on suite, 61.98%
to 91.26% of the latest counted residual-plus-native block entries still enter a coarse
Rust block kernel. Task 362 further proves that extracting one unsupported call into a tiny
native leaf makes this fragmentation worse.

The current 100 ms split-suite smoke score is 2408.43. This is a useful development metric,
not yet an exact upstream V8v7 result: `scripts/run-v8v7.sh all` starts one process per
suite and takes a geometric mean of already formatted suite scores. Upstream loads the
eight suites in one process, performs a discarded warmup, measures for at least one second
and 32 iterations, and computes from the raw timing/reference ratios. With the current
hybrid tracing/`Rc` heap and known cycle leaks, fresh processes can hide cumulative retained
state. Task 365 therefore owns an exact acceptance lane.

Sources: upstream V8v7
<https://chromium.googlesource.com/v8/v8.git/+/dd3f1ecf719afd21b4c695c776b4da2fb494ef92/benchmarks/base.js>
and suite driver
<https://chromium.googlesource.com/v8/v8.git/+/dd3f1ecf719afd21b4c695c776b4da2fb494ef92/benchmarks/run.js>.

## The one quoted pipeline

Keep the Lisp staging order and categorical interface:

`DynCode -> RegionPlan<MicroOp> -> abstract reduction -> costed cover ->`
`StencilExpr -> size/layout -> one copy/patch/link effect`.

`Context = Rep × Location × Shape × Ownership × Range × Effects × PcState` is the one
machine-state object. Opcode, block, loop, call-containing region, and whole function are
nested morphisms over the same quote. `Kernel` and patched `StencilInstance` are two
physical realizations of a compatible morphism, not separate semantic systems. Compiler
passes are pure quote-to-quote functions; only final linking and explicit IC publication
mutate runtime state.

This framing rules out two recurring mistakes: selecting the smallest stencil merely
because it exists, and treating machine-code concatenation as an optimizer. Larger tiles
matter only when they let rustc/LLVM remove frame traffic, guards, address calculations,
site-pointer updates, ownership operations, or host connector transitions.

## Ranked experiments

1. **Make semantic cover total — Tasks 309/157/316/348.** Lower every bytecode to a small
   generated micro-op vocabulary. A throwing or dynamic micro-op uses a typed cold edge to
   one shared kernel; it must not demote its native prefix and suffix. Cost the cover across
   primitive, block, loop, and shared-kernel candidates using the actual cooked bytes,
   relocations, loads, stores, branches, and connector transitions. CPython independently
   validates bytecode -> micro-op optimization -> copy-and-patch staging
   (<https://github.com/python/cpython/blob/main/InternalDocs/jit.md>); CACAO shows why a
   relocatable exception edge is needed inside copied superinstructions
   (<https://www.complang.tuwien.ac.at/cd/papers/A73-full.pdf>).

2. **Erase the physical site/IP dependency chain — Task 366.** Carry a symbolic known PC
   through closed composites, burn successors and branch targets, and materialize a real
   site pointer only at a slow path, throw, call boundary that needs it, or region exit.
   The relevant ECOOP 2024 result identifies VM instruction-pointer updates as a serial
   dependency and removes or defers them
   (<https://drops.dagstuhl.de/storage/00lipics/lipics-vol313-ecoop2024/LIPIcs.ECOOP.2024.14/LIPIcs.ECOOP.2024.14.pdf>).

3. **Add bounded typed context versions and pass-through connectors — Tasks
   171/144/152/164/158/203.** Run one liveness/loop prepass and forward abstract reduction.
   Guard once, keep `I32`, `F64`, shape and backing facts in registers through the region,
   and reconstruct canonical state only on terminal side exits. Maglev demonstrates the
   compact SSA-CFG, known-state propagation, representation selection and simple forward
   register allocation combination (<https://v8.dev/blog/maglev>); static BBV provides a
   no-runtime-hotness algorithm for dynamic checks, arithmetic and array bounds
   (<https://doi.org/10.4230/LIPIcs.ECOOP.2024.28>).

4. **Move property/call IC successes into machine state — Tasks 145/153/154/368.** Express
   a property hit as `GuardShape ; LoadOffset ; Continue`. Fold several shapes sharing one
   offset, and use prototype/property watchpoints so stable inherited accesses do not walk
   the chain per hit. Keep a shared structural kernel plus separate immutable fields when
   sharing wins; copy and burn fields only when the measured costed cover wins. SpiderMonkey
   documents precisely this guard/pure/result IR, stub folding, and shared-code/separate-field
   design (<https://firefox-source-docs.mozilla.org/js/cacheir.html>); JSC documents shape
   propagation and watchpoint invalidation (<https://webkit.org/blog/10308/speculation-in-javascriptcore/>).

5. **Only then restore guest calls as whole regions — Tasks 146/181.** The morphism must
   include surrounding value/ownership/effect context, callee guard, guest frame setup,
   direct entry, return continuation, and native suffix. Task 362 rejects the isolated
   opcode form, not guest continuations. V8's adaptor-frame removal provides the actual
   argument-count frame convention and reports gains in Richards and Earley-Boyer
   (<https://v8.dev/blog/adaptor-frame>).

6. **Forward effects and remove allocations — Tasks 172/329/191/176/321.** Once native
   state is continuous, forward loads/stores, transfer owners on last use, scalar-replace
   nonescaping objects, and omit barriers for fresh objects. JSC shows allocation sinking
   becoming possible when property effects are represented in IR
   (<https://webkit.org/blog/10298/inline-caching-delete/>).

7. **Compact physical data — Tasks 165/214/265/369, then 148/193/320/321.** Split ordinary
   object and array payloads, add a compact shape/flags header and bounded inline slots,
   track monotone packed/holey numeric/value array kinds, and finish one managed tracing
   heap for functions, strings and environments. V8 separates named properties from
   elements and specializes packed/holey Smi/double/value stores
   (<https://v8.dev/blog/fast-properties> and <https://v8.dev/blog/elements-kinds>).

8. **Treat strings and RegExp as their own representation problems — Tasks 11/87/190/227/230.**
   Use numeric interned property keys, Latin-1/UTF-16 sequential strings, slices/ropes, and
   flatten only at explicit kernels. Generate general RegExp scan/check/advance, word-compare,
   class-range and test-only stencil families; do not bake patterns from V8v7. V8 reports
   benefits from RegExp dispatch and peephole fusion
   (<https://v8.dev/blog/regexp-tier-up>).

9. **Optimize physical realization after coverage — Tasks 189/291/322/347 and 52.** Use
   static branch heuristics, hot/cold splitting, fallthrough placement and target-measured
   tile sizes. Test rustc PGO only for shared host/kernel code as an independent candidate
   (<https://doc.rust-lang.org/nightly/rustc/profile-guided-optimization.html>); it cannot
   substitute for eliminating Rust boundaries.

## Immediate experiments and proof counters

The first performance slice remains Tasks 309/157/316/348 plus Task 366: take one mixed
block family, preserve native prefix and suffix across one effect kernel, burn all closed
operands/successors, and eliminate per-leaf site updates. Before/after diagnostics must
report, normalized by guest bytecodes or suite iterations:

- residual block-kernel entries and explicit slow-kernel edges;
- native/Rust/native connector transitions;
- physical site advances and exact-PC materializations;
- `InlineSite` operand loads and canonical frame loads/stores;
- guards, ownership operations, calls, copied instance bytes, unique kernel bytes, patches,
  and direct fallthroughs.

The next typed-loop slice may proceed only after this cover is total. It must show an
unboxed loop-carried `I32` or `F64`, one entry guard, direct backedge, and terminal side-exit
materialization in disassembly. Suites guide measurement only: object/call work is measured
in Richards/DeltaBlue/RayTrace/Earley-Boyer/Splay, `I32` work in Crypto, `F64` packed-array
work in Navier-Stokes, and matcher/string work in RegExp. Those identities never enter
lowering or selection.

## Measurement correction

Use 20 ms split-suite runs only for correctness and gross-regression screening. Diagnose a
candidate for 3–5 seconds with normalized structural counters and a time profile. Screen
with at least nine fresh-process randomized paired A/B repetitions, paired log ratios and
confidence intervals. Final acceptance uses Task 365's same-process upstream-shaped lane,
unrounded ratios, one-second warmup and measurement windows, at least 32 iterations, and
at least nine independently launched paired runs. The geometric-mean contribution is the
mean of suite log ratios; raw point gains are not additive.

Google Benchmark explains randomized interleaving and its minimum-repetition warning
(<https://github.com/google/benchmark/blob/main/docs/random_interleaving.md> and
<https://github.com/google/benchmark/blob/main/docs/tools.md>); Kalibera and Jones give the
VM-specific repetition/confidence methodology
(<https://kar.kent.ac.uk/33611/45/p63-kaliber.pdf>).

Publish anonymous executable images to platform profilers where possible, then inspect only
the dynamically implicated templates with `llvm-objdump` and `llvm-mca`. Task 331 owns this
instrumentation. The Linux JIT interface is specified at
<https://github.com/torvalds/linux/blob/master/tools/perf/Documentation/jitdump-specification.txt>.

## Ideas explicitly deferred

Do not try another isolated call/property leaf, hand-written exact superinstruction, broad
vectorizer, code-layout-only change, pointer-compression migration, or host PGO pass before
total cover. Published systems use all of these techniques, but the local evidence says
they cannot dominate the millions of coarse Rust entries and connector transitions still
executing today.
