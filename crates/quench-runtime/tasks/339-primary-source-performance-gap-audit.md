# 339 — Primary-source performance-gap audit, round twenty-eight

Status: complete

Re-audit the runtime against primary descriptions of Deegen, copy-and-patch, Sparkplug,
JavaScriptCore, V8 object storage, CPython's stencil JIT, and LLVM's inliner. Do not turn
production-engine features into predictions; use local profiles and alternating V8v7 A/B
as the decision boundary.

## Findings

The earlier representation critique is stale. This runtime already has an eight-byte
NaN-boxed `Value`, immutable hash-consed shapes, fixed property slots, shape-guarded own
and prototype ICs, dense-array storage, and a nonmoving traced object heap. Rebuilding
those would duplicate facts rather than improve the executed path.

The live gap is cross-boundary optimization. Deegen combines register pinning, tail-call
continuations, type-check removal, IC slabs, hot/cold splitting, and a custom guest stack.
Sparkplug likewise removes bytecode dispatch while retaining a fixed frame convention.
In this runtime, residual blocks and user calls still enter Rust helpers, and Task 337
proved that wrapping the same helper in another copied fragment is slower. Task 338 also
proved that making activation cleanup sparse is slower. The remaining order is therefore:

1. remove avoidable work on the current call edge, retaining only measured wins;
2. census exact initial inline candidates without changing the default binary;
3. implement Task 20's SCC-bottom-up quote rewrite for exact straight-line leaves;
4. widen it to control flow, nested calls, and caller-local IC recipes;
5. implement Task 146's custom guest stack and preserve-none continuation ABI for edges
   that remain uninlined;
6. propagate representations/shapes through the enlarged graph and tile it into
   multi-level stencil instances plus shared kernels once.

This is one staged data pipeline:

`bytecode graph -> semantic facts -> rewrite fixed point -> stencil tiling -> copy+patch`.

The representation remains cold and inspectable until the final emission edge. Kernels
are shared immutable leaves; templates become patched instances only at final link.

## Primary sources

- Deegen paper, published 18 November 2024: <https://arxiv.org/abs/2411.11469>.
- Copy-and-patch paper and artifacts: <https://compilers.stanford.edu/publications/copy-and-patch/>.
- V8 Sparkplug compiler design: <https://chromium.googlesource.com/v8/v8.git/+/refs/heads/main/docs/compiler/sparkplug/compiler-sparkplug.md>.
- JavaScriptCore value profiling, structures, and ICs: <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- V8 hidden classes and fixed-slot properties: <https://v8.dev/blog/fast-properties>.
- CPython's generated copy-and-patch stencil pipeline: <https://github.com/python/cpython/blob/main/InternalDocs/jit.md>.
- LLVM's bottom-up CGSCC inliner discipline: <https://llvm.org/docs/doxygen/Inliner_8cpp_source.html>.
- Typed-shape basic-block versioning evaluation: <https://arxiv.org/abs/1507.02437>.

