# 39 — Vectorized stencils for packed-numeric loops

Status: planned

navier-stokes and crypto are dominated by fixed-stride float/int array map and reduce loops over provably packed-numeric backing. Once [[32-element-kind-guarded-arrays]]'s `ElementKindGuarded<K>` connector proves an array is packed-numeric for the duration of a loop, that guard is exactly the precondition needed to lower the whole loop body to a vectorized AOT template instead of a scalar per-element stencil, rather than only removing the per-element bounds/kind check.

Extend the rustc/LLVM extraction pipeline ([[02-rustc-aot-templates]]) with SIMD-lowered handler variants for the common map/reduce shapes (elementwise arithmetic, multiply-accumulate), either via autovectorization of the handler source in `stencil-aot/handlers.rs` or hand-written SIMD kernels for the specific shapes the extractor recognizes. This must remain a general lowering keyed on the guarded element-kind and loop shape, not a benchmark-specific fused loop (consistent with [[04-bytecode-coverage-map]]'s constraint against aspirational or benchmark-shaped optimization).

Acceptance: a packed-numeric elementwise loop compiles to a vectorized stencil verified by inspecting emitted instruction width/count; a loop where the element kind cannot be proven stable for its full duration falls back to the scalar guarded path from [[32-element-kind-guarded-arrays]]; navier-stokes and crypto show measured A/B gains via [[05-performance-harness]].

## Round-ten refinement: two vectorizers, one quoted input

Treat loop widening and basic-block SLP as two derived lowerings from the same typed
dependence graph, not as one generic "SIMD" switch. Loop vectorization packs consecutive
iterations. SLP packs isomorphic independent scalar trees inside or across basic blocks,
including sibling fields of fixed numeric objects and explicitly unrolled arithmetic.
Rust macros generate the finite AOT kernel/template families for both; Task 157's cost
model chooses among scalar, loop-vector, and SLP forms without runtime hotness.

Only exact integer/bitwise operations or FP transformations preserving JavaScript's
rounding order may be packed. A vector-width choice and scalar remainder are named
policies, never magic literals.

Primary source: LLVM documents its Loop Vectorizer and SLP Vectorizer as distinct passes
with distinct opportunity shapes: <https://llvm.org/docs/Vectorizers.html>.
