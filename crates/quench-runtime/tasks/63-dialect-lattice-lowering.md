# 63 — Explicit intermediate dialect between AST and per-architecture machine code

Status: planned

The current Kernel/Stencil lowering goes directly from the AST category to one architecture's machine-code category (AArch64 today). Following MLIR's architecture (a lattice of dialects connected by verified-functorial lowering passes, so optimizations proven at a higher dialect survive lowering to every lower one), introduce an explicit intermediate dialect between the AST-level `StencilNode` category and any one architecture's instruction category — this becomes necessary, not merely elegant, once [[55-architecture-target-matrix]] and [[56-x86-64-sysv-backend]] introduce a second architecture, since without an intermediate level every optimization proven against AArch64 would need re-proving against x86-64 independently.

Concrete steps:
1. Define the intermediate dialect: an architecture-neutral instruction-shape representation (e.g. "guarded binary numeric op," "tag-mask compare," "tail dispatch to next") that both `a64_abi` and the future `x64_abi` ([[56-x86-64-sysv-backend]]) lower into via their own architecture-specific functor, per [[54-llvm-capability-catalog]]'s principle-vs-encoding split.
2. Prove (structurally, the same style as [[01-stencil-category-core]]'s composition laws) that the AST→intermediate-dialect lowering functor preserves composition — this is the step that lets [[17-seq-flatten-linear-link]], [[18-identity-erasure-peephole]], and [[23-egraph-rewriting]]'s rewrites all operate once at the intermediate level and apply to every architecture automatically.
3. Verify the two existing/planned architecture-specific lowerings (`a64_abi`, future `x64_abi`) are each a functor out of this intermediate dialect, not a parallel reimplementation of AST-level lowering.

Acceptance: a rewrite proven correct at the intermediate-dialect level (e.g. identity erasure) requires zero additional per-architecture proof to apply to both AArch64 and x86-64 output; the AArch64 backend is refactored to route through the intermediate dialect with no behavior change (regression-tested via [[05-performance-harness]]); [[56-x86-64-sysv-backend]]'s x86-64 backend is built as a lowering functor from the same intermediate dialect rather than a second from-scratch AST lowering.
