# 311 — Primary-source algorithm research, round twenty-two

Status: complete

Research additional VM/compiler algorithms under the standing contract: every function
executes a stencil image from first entry; runtime code production is bounded
copy/patch/share and semantic IC repair; rustc/LLVM cooks a finite general catalog AOT;
there is no interpreter fallback, execution-count hotness gate, runtime LLVM, benchmark
identity, or V8v7-trained selector.

## Correct current baseline

Do not repeat stale prescriptions. `Value` is already one NaN-boxed word. Objects already
use immutable shapes, fixed slots, stable non-`Rc` handles, canonical transition edges,
and shape-guarded property stencils. Arrays already have a dense path. The remaining
cost is not “add NaN boxing” or “add hidden classes”; it is helper/frame boundaries,
per-edge ownership, repeated guards/materialization, generic aggregate storage, and
insufficient optimization across stencil boundaries.

Task 310's canonical POD guest ABI measured `2054.04 -> 2047.02` (-0.34%). This is useful
falsification: representation infrastructure does not change the score until the native
call edge actually consumes it.

## Ranked result

1. **Keep the direct guest call/return continuum first (Tasks 146/181).** Production VM
   sources converge on one contiguous guest frame, actual-argument count in that frame,
   a custom native ABI, direct callee transfer, and explicit return/exception
   continuations. The present helper-assisted direct-call experiment regresses despite
   high IC hit rates, so another IC-width or frame-layout tweak cannot substitute for
   removing the Rust transition.

2. **Implement Task 309's semantic micro-op algebra and compile-on-load partial
   evaluation.** CPython explicitly found direct bytecode copy-and-patch too coarse for
   useful optimization; it optimizes micro-ops first. Weval specializes an existing
   interpreter against a fixed program using an abstract-value lattice and a worklist
   keyed by context/block, reporting 2.17x for SpiderMonkey AOT and 1.84x for Lua without
   runtime profiling. This VM should specialize its own semantic quote, not embed Weval
   or interpret micro-ops.

3. **Burn ordinary operands into stencil instances (Task 316).** The present cooker
   closes sequential/slow/branch control relocations, but ordinary handlers still read
   many register, local, literal, and IC operands from `InlineSite`. Copy-and-Patch and
   Deegen patch these runtime constants into code or closed local data. Until this lands,
   much of the catalog is native descriptor decoding rather than C-like direct code.

4. **Replace exact whole-block matching with a total costed cover (Task 157).** One
   unsupported operation currently demotes an otherwise coverable block to a generic
   Rust range. Dynamic programming over `(micro-op position, connector context)` lets
   primitive, supernode, block, and kernel arrows cover one block together while
   charging native/Rust/native seams explicitly. This is the multi-level monoid the
   architecture requires; it avoids both all-or-nothing blocks and thousands of peers.

5. **Use static BBV plus typed shapes (Tasks 144/152).** A deterministic compiler
   worklist, not runtime heat, propagates tag/range/shape facts through bounded block
   versions. Published Static BBV reports roughly 10% average improvement with a
   two-version cap; typed-shape BBV reports 25% execution-time reduction and 48% fewer
   type tests in its evaluated system. These are external results, not promises here.

6. **Make connector registers and ownership region facts (Tasks 158/172/292).** Native
   leaves still lose when values round-trip through virtual slots or retain/release at
   every seam. The first general allocator should be linear scan over block-argument SSA,
   with fixed guest/metadata/continuation registers and explicit edge-copy stencils.

7. **Add the bounded, independently measurable shape-transition refinement in Task
   312.** JavaScriptCore stores the common one-child Structure transition directly and
   promotes to a map only when the tree actually branches. The current engine still
   allocates an inner `HashMap` for every parent shape. This is the only newly found
   small representation experiment justified before the larger compiler substrate.

8. **Use register-pressure-aware pure-expression ordering (Task 313).** Sethi-Ullman
   numbering supplies a deterministic, linear-time order for independent expression
   subtrees that minimizes temporary register/storage pressure under its machine model.
   It gives Task 241's law-licensed search a real initial algorithm once Task 292 exposes
   register resources.

9. **If-convert only tiny pure diamonds (Task 314).** AArch64 can lower a compare plus
   two cheap pure values to `csel`; LLVM's own pass checks speculation legality, liveness,
   latency, and target profitability. Apply this after semantic/effect lowering, never
   to allocating, throwing, coercive, or heap-mutating arms.

10. **Undo harmful eager code motion with path-lazy pure duplication (Task 315).** V8
   documents that global deduplication may hoist an expensive pure computation above a
   branch, making untaken paths pay for it; its scheduler may duplicate the operation
   back into the consuming paths. This complements GVN/PRE rather than contradicting it.

11. **Refine Task 171 with sealed-block SSA construction.** Braun et al.'s algorithm
   translates bytecode directly into minimal pruned SSA, creates incomplete block
   arguments for unsealed blocks, and removes trivial phis as predecessors become known.
   Cranelift's production implementation confirms the data structure. This is a compact
   implementation route, not a separate optimization tier or runtime dependency.

12. **Promote Task 87 only on its measured RegExp component.** Current native samples
    attribute 13.61% to the bounded backtracker, 6.25% to hybrid forward search, and a
    further large share to allocation/free/memmove. A general AOT matcher can combine
    adjacent character operations, character-class ranges, bounded word-wide compares,
    and quick-check/Boyer-Moore prefilters. It must compile every supported pattern at
    creation and use a shared kernel for unsupported semantic forms—no regexp hotness
    counter.

## Architecture consequence

The small kernel remains:

`semantic definition -> micro-op quote -> immutable CFG/SSA -> pure reducers -> costed
cover -> StencilExpr -> one link`

Each optimization is a transformation of data or a choice among equal typed arrows.
`Kernel` is shared immutable executable identity. `StencilTemplate` is immutable copied
code plus holes. `StencilInstance` is a closed patched arrow and may be shared only when
its complete patch environment is identical. IC fields are data slabs; sharing code must
not accidentally share mutable site state.

## Primary sources

- Deegen preprint, first submitted 18 November 2024:
  <https://arxiv.org/abs/2411.11469>.
- Copy-and-Patch: <https://arxiv.org/abs/2011.13127>.
- CPython micro-op optimizer and stencil JIT:
  <https://github.com/python/cpython/blob/main/InternalDocs/jit.md> and
  <https://peps.python.org/pep-0744/>.
- Weval partial evaluation: <https://c1f.net/pubs/pldi2025_weval.pdf>.
- Static BBV: <https://doi.org/10.4230/LIPIcs.ECOOP.2024.28>.
- Typed Object Shapes: <https://arxiv.org/abs/1507.02437>.
- V8 Sparkplug and single-frame call layout: <https://v8.dev/blog/sparkplug> and
  <https://v8.dev/blog/adaptor-frame>.
- JavaScriptCore transition representation:
  <https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/StructureTransitionTable.h>.
- SpiderMonkey CacheIR recipe/stub architecture:
  <https://firefox-source-docs.mozilla.org/js/cacheir.html> and
  <https://firefox-source-docs.mozilla.org/js/how_we_optimize.html>.
- Sethi-Ullman expression ordering: <https://doi.org/10.1145/321607.321620>.
- LLVM machine if-conversion:
  <https://llvm.org/doxygen/IfConversion_8cpp_source.html>.
- Braun et al. SSA construction:
  <https://pp.ipd.kit.edu/uploads/publikationen/braun13cc.pdf> and Cranelift's
  implementation <https://docs.rs/cranelift-frontend/latest/src/cranelift_frontend/ssa.rs.html>.
- V8 CFG/effect/scheduling experience:
  <https://v8.dev/blog/leaving-the-sea-of-nodes>.
- V8 and JSC RegExp fusion/word compare results:
  <https://v8.dev/blog/regexp-tier-up> and
  <https://webkit.org/blog/8685/introducing-the-jetstream-2-benchmark-suite/>.

No score claim changed in this research task. Published results are evidence for
experiment order, never projected local gains.
