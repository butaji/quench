# 334 — Effect-safe loop distribution into composable stencil traces

Status: planned

Split a traced loop into two or more ordered loop morphisms only when the quoted SSA/effect
graph proves that cross-part dependences permit it. The immediate use is to isolate a pure,
packed-numeric map/reduction from an aliasing store, call, or scalar recurrence that prevents
[[39]] from vectorizing the entire original loop.

This is a rewrite over the same hierarchical `Region`/SSA data. Each distributed component
is an ordinary `Stencil<LoopState, LoopState>`; their sequence remains categorically
composable, and the original loop is retained as the identity-cost fallback. No source-loop
name, benchmark identity, observed trip count, or runtime hotness participates.

Legality requires [[173]] memory/effect tokens, [[175]] induction/range proofs, and [[278]]
runtime no-alias versioning where static disjointness is unavailable. The planner derives:

- a dependence partition preserving exact JavaScript effect and exception order;
- required scalar live-ins/live-outs and materialization edges;
- candidate scalar, interleaved, and vector stencil plans;
- a cost including added loop control, guards, code bytes, register pressure, and cache
  footprint.

Acceptance: dependence cycles and observable ordering reject distribution; no-alias guarded
versions rejoin the canonical fallback; zero-trip, remainder, exception, and side-exit tests
pass; LLVM optimization remarks/disassembly prove the isolated loop vectorizes or interleaves;
the candidate must improve Navier-Stokes/Crypto and the complete V8v7 A/B.

Primary sources: LLVM documents loop distribution as a way to isolate the part inhibited by
memory dependencies, and VPlan's Legal/Plan/Execute split keeps alternative plans immutable
until materialization: <https://clang.llvm.org/docs/LanguageExtensions.html> and
<https://llvm.org/docs/VectorizationPlan.html>.
