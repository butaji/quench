# 143 — Primary-source VM optimization research synthesis

Status: complete

Research copy-and-patch, modern JavaScript baseline/optimizing JITs, quickening,
context versioning, hidden classes, inline caches, allocation, and Rust/LLVM linking.
Convert the results into repository work items ranked by the cost visible in the current
profiles. This is not permission to copy another VM or add a third-party VM: all runtime
execution remains this project's Rust implementation and pre-cooked stencil catalog.

## Result

The strongest evidence says the next order-of-magnitude gains will not come from making
individual tiny opcode stencils a few instructions shorter. They come from removing a
dynamic boundary across a larger stable region:

1. **Share immutable language kernels.** Task 142 immediately validated this pattern:
   a RegExp literal had rebuilt its immutable matcher on every evaluation. Sharing that
   kernel while retaining fresh mutable wrapper instances improves RegExp by roughly
   eleven times and the full geometric mean by roughly one third.
2. **Context-version whole stencil regions.** Lazy basic block versioning reports 71%
   of type tests removed and speedups up to 50%; its interprocedural form reports 94.3%
   of type tests removed on average. Task 144 adapts the algorithm to always-stencil,
   bounded, first-use composition with no interpreter or hotness threshold.
3. **Put IC hit logic in executable inline slabs.** Deegen and JavaScriptCore reserve
   patchable code beside the main path so a monomorphic hit is a shape/callee guard and
   direct operation, not a call into a Rust metadata helper. Task 145 makes this concrete
   for property and call sites.
4. **Use a direct VM call/return ABI.** Deegen pins VM state in registers and uses a
   custom stack; Sparkplug deliberately keeps frames compatible with the rest of V8.
   The current runtime still crosses Rust dispatch and constructs Rust-owned frame
   storage on user calls. Task 146 removes that boundary.
5. **Use transition-DAG shapes and compact key identities.** V8's fast-property model
   shares transition trees and descriptor metadata. This refines Tasks 07, 11, and 135:
   shape evolution must be keyed by `(parent shape, atom, attributes)`, rather than by
   cloning and hashing the complete ordered property-name vector.
6. **Replace per-value reference counting with a VM heap.** Immix and production JS
   collectors use region/cell allocation and tracing so a value move does not perform
   retain/release work. Task 148 specifies the smallest safe non-moving version.
7. **Derive specialized templates at build time.** Deegen's controlled de-abstraction
   derives bytecode variants and type-check reductions from one semantic definition.
   Task 149 applies that staging discipline through Rust macros and rustc/LLVM, without
   embedding LLVM or adding runtime compiler machinery.

Task 147 subsequently tested string-literal kernel sharing and rejected it at -0.39%;
its site-local `Rc` representation did not remove enough work to matter. The near-term
order is therefore 144, 145, 146, 148, and 149, with each step required to pass the
standing full-suite gate. Equality saturation, polyhedral loop
transforms, and broad SIMD are deliberately below these items: they optimize expression
or arithmetic quality while profiles still show dispatch, calls, allocation, shapes,
and ownership as the larger costs.

## Primary sources

- Copy-and-Patch: <https://arxiv.org/abs/2011.13127>
- Deegen: <https://arxiv.org/abs/2411.11469> and
  <https://fredrikbk.com/publications/deegen.pdf>
- V8 Sparkplug: <https://v8.dev/blog/sparkplug>
- V8 Maglev: <https://v8.dev/blog/maglev>
- JavaScriptCore speculation and inline caches:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- Lazy basic block versioning: <https://arxiv.org/abs/1411.0352>
- Interprocedural basic block versioning: <https://arxiv.org/abs/1511.02956>
- Speculative staging/quickening: <https://arxiv.org/abs/1310.2300>
- V8 fast properties: <https://v8.dev/blog/fast-properties>
- V8 elements kinds: <https://v8.dev/blog/elements-kinds>
- Immix: <https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf>
- JavaScriptCore GC: <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>
- LLVM JITLink: <https://www.llvm.org/docs/JITLink.html>
