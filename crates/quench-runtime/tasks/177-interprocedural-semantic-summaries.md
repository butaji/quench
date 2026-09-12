# 177 — Interprocedural semantic-summary fixpoint

Status: planned

Compute one immutable summary per function and bounded call context over the closed
`DynCode` call graph. Summary facts include: may throw, may allocate, may mutate each
heap-effect domain, may capture each argument/environment, consumes or borrows arguments,
return representation, and possible direct callees. Recursive strongly connected
components converge through one pessimistic finite-lattice fixpoint driver.

This is an Attributor-style kernel: analyses exchange facts through a shared dependency
graph instead of each pass implementing a private call-graph traversal. Task 163 may
derive caller-customized summaries from the same representation, subject to its bounded
context budget. Unknown/native/eval-like calls receive a canonical conservative summary.

Summaries are categorical effect/ownership signatures for function morphisms. They let
Task 172 preserve borrows and Task 173 preserve unaffected heap locations across direct
calls without inlining the callee. Summary data is compile/link-time metadata; hot code
sees only the resulting stencil choice.

Acceptance: recursive, mutually recursive, throwing, allocating, capturing, native, and
unknown calls converge correctly; a named iteration/context budget forces conservative
termination; summary verification agrees with instrumented effects; direct-call regions
retain proven facts across calls; Richards/Earley-Boyer and complete V8v7 A/B improve.

Sources: <https://llvm.org/docs/doxygen/structllvm_1_1Attributor.html> and
<https://www.llvm.org/docs/doxygen/FunctionAttrs_8cpp.html>.

