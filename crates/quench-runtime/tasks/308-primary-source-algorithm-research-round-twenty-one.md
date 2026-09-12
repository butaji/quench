# 308 — Primary-source algorithm research, round twenty-one

Status: complete

This pass searched current primary papers, production-engine documentation, and engine
source for additional algorithms compatible with the standing rules: stencil execution
from first invocation, finite rustc/LLVM-cooked templates, runtime copy/patch/share only,
no interpreter fallback, no execution-count hotness gate, and no benchmark identity in
selection.

## Main conclusion

The missing factor is not another isolated opcode stencil. Current systems consistently
put a small optimizable operation algebra between source bytecode and copy-and-patch
emission, then remove whole boundaries over that algebra. CPython explicitly reports
that compiling ordinary bytecode did not provide enough optimization potential; its JIT
instead optimizes finer micro-ops before copy-and-patch. Weval reports 3–5x over its
generic interpreter by partially evaluating interpreter control against a fixed program.
Druid's generated baseline frontend reaches only 0.7x its handwritten JIT, further
warning that semantic generation alone does not create C-like code.

## Ranked patterns to try

1. **Finish the native guest-call continuum (Tasks 146/163/181).** The current feature
   is heavily exercised yet regresses the four call-heavy suites by 4.68% geometrically;
   even Richards and Splay regress with 96.79% and 98.44% cache hit rates. Therefore the
   miss policy is not the problem. Remove `execute_direct_call`, `prepare_direct_child`,
   Rust `DynFrame` construction, and `complete_dyn_frame` from the successful edge.
   V8's one-frame actual-argument-count layout improved Richards 4.6% and EarleyBoyer
   6.1%; CPython likewise allocates most language frames contiguously on a per-thread
   stack.

2. **Add one semantic micro-op algebra (Task 309).** Decompose complex `DynOp`s into a
   quoted sequence of guards, pure loads/conversions/operations, stores, calls, effects,
   and exits. `RegionPlan` optimizes this data; the costed tiler then selects cooked leaf
   or coarse stencil templates. This is the missing bridge between Tasks 149/171 and the
   machine catalog, and is not another execution tier.

3. **Run compile-on-load partial evaluation over that algebra (Tasks 144/149/152/158/
   164/171).** Bytecode operands, CFG edges, lexical slots, constants, and immutable
   shapes are static inputs; specialize them away at link time. Dynamic tags/shapes stay
   explicit guard coproducts whose failure continuation enters a generic stencil version,
   never an interpreter. Weval validates the algorithmic scale; this project implements
   it over its own Rust semantic quote and finite stencil catalog.

4. **Inline or partially inline small stable callees (Tasks 20/163/288/290).** A direct
   native call removes Rust machinery; inlining additionally exposes caller/callee
   guards, loads, and ownership traffic to the same reducer. Use static size/effect/SCC
   budgets and bounded target sets, not call counts.

5. **Make ownership an SSA fact (Tasks 148/172/193).** CPython 3.15 reports avoiding
   reference-count operations and using uniqueness to enable in-place operations. Here,
   ownership SSA should decide which heap-word moves need retain/release or cleanup and
   generate precise frame root/owned-slot maps. This is a prerequisite for discarding a
   pointer-bump guest frame safely.

6. **Keep shared kernels near copied code and cold obligations out of line (Tasks 189/
   274/276/289).** V8 documents measurable penalties when generated code cannot use short
   direct calls to builtins. The link arena should range-check direct branches and create
   one shared near-code island only when needed; it should not copy large semantic
   kernels into every image.

7. **Pin the single metadata plane (Task 203).** Sparkplug stores the frequently used
   feedback-vector pointer in its compatible frame. This VM already has one canonical
   `InlineSite` plane; its base, VM/heap base, frame base, and current value context are
   the connector state. Do not add a second IC metadata representation.

## Architecture distilled

The data flow is one staged expression:

`DynCode -> semantic micro-ops -> RegionPlan fixed point -> costed stencil cover -> link`

The effectful boundary is only final linking and later bounded IC case publication.
Kernels remain immutable shared arrows. Stencil templates remain finite patched arrows.
Both consume and produce the same typed connector contexts, so coarse recipes are closed
under the same composition law as leaves.

## Primary sources

- CPython copy-and-patch rationale and micro-op optimizer:
  <https://peps.python.org/pep-0744/>.
- CPython 3.15 JIT, register allocation, ownership, and AArch64 results:
  <https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>.
- CPython contiguous language frames:
  <https://github.com/python/cpython/blob/main/InternalDocs/frames.md>.
- V8 one-frame JavaScript call convention and measured results:
  <https://v8.dev/blog/adaptor-frame>.
- V8 interpreter-compatible baseline frames and pinned feedback vector:
  <https://v8.dev/blog/sparkplug>.
- V8 direct versus indirect builtin calls and near-code placement:
  <https://v8.dev/blog/short-builtin-calls>.
- V8 custom register ABI, tail calls, and leaf-frame elision:
  <https://v8.dev/blog/csa>.
- Weval partial evaluation and reported JavaScript use:
  <https://github.com/bytecodealliance/weval> and
  <https://cfallin.org/blog/2024/08/28/weval/>.
- Druid AOT meta-compilation:
  <https://arxiv.org/abs/2502.20543>.
- Deegen's generated optimization inventory:
  <https://arxiv.org/abs/2411.11469>.
- SpiderMonkey's guard/pure-result CacheIR discipline:
  <https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>.

No score claim changed during this research pass.

