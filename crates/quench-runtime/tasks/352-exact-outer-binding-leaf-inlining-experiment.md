# 352 — Exact outer-binding leaf inlining experiment

Status: complete

Implement the first nonzero, general Task 20 slice proved by Task 351. Resolve a direct
outer binding while compiling the caller, require a user-function target whose closure
outer environment is pointer-identical to the caller's outer environment, and classify
the actual target through the canonical `FunctionCallRecipe`/`InitialInlineDecision`.

Represent the result as immutable data:

`StaticCallTargetRecipe = ExactIdentity × EnvironmentIdentity × CalleeQuote × CallABI`.

Lower that recipe through the ordinary stencil/category algebra:

`GuardExactValue ; AlphaRename(InlineBody) ; ReturnContinuation`

with the unchanged ordinary call morphism as the guard-failure branch. The exact target
owner must remain rooted by the caller image. Composition occurs before final PC-indexed
CFG, liveness, IC, label, and relocation derivation. There is no runtime heat counter and
no benchmark/source-name condition.

Initial bounds are the existing straight-line, exact-arity, noncapturing,
non-`arguments`, non-nested-call subset. Recursive/SCC and property-derived targets remain
normal calls. Constants name every code-size, frame-size, target-count, and expansion
bound.

Acceptance:

- semantic tests cover arguments, result/return continuation, free-name lookup through
  the identical outer environment, reassignment fallback, and recursive rejection;
- structured counters report nonzero exact applied calls on the Task 351 holdout (the
  prior census predicts roughly 283K Earley-Boyer executions at a 300 ms window);
- all 128+ release tests and the complete V8v7 suite pass;
- retain only if an interleaved long all-suite A/B improves aggregate score without a
  component-floor violation; otherwise remove the machine-code/rewrite experiment while
  preserving the reach result.

This is a proof slice, not the full call solution. Property-call target recipes and
hierarchical nested-call composition remain necessary for Richards, RayTrace, Crypto,
and Splay.

Outcome: rejected from the runtime after two measured variants. The first variant
expanded the callee bytecode into the caller and materialized callee parameter/local
slots. A 5 × 500 ms interleaved, same-binary toggle comparison measured 2288.60 without
the rewrite and 2291.41 with it (+0.12% aggregate; Earley-Boyer +0.70%), which is
noise-level and below the acceptance bar. A second variant forwarded read-only
parameters directly from caller registers, avoiding the embedded callee locals. Its
3 × 200 ms screen measured 2305.00 without the rewrite and 2291.53 with it (-0.58%
aggregate; Earley-Boyer +0.52%).

Both variants were semantically correct: focused tests covered exact arguments/results,
free-name lookup through the identical outer environment, identity-change fallback,
recursive rejection, and caller-local shadowing. The rustc cooker extracted the
exact-identity guard with both symbolic exits, and V8v7 reported 16 applied Earley-Boyer
sites. The failure is therefore not missing reach or an unwired branch. Generic
bytecode expansion keeps paying ordinary block execution and permanently expands the
caller register file; avoiding a call activation is insufficient by itself.

The rewrite opcode, guard stencil, statistics, tests, and experimental toggle were
removed. Task 351's static reach census remains. Evidence is retained in
`reports/task352-inline-ab-200ms-3`, `reports/task352-inline-ab-500ms-5`, and
`reports/task352-forwarded-inline-ab-200ms-3`.

The next experiment must inline at a coarser abstraction: select one closed,
rustc/LLVM-cooked leaf-call stencil whose operand holes bind caller registers directly,
whose identity guard and fallback are part of the same composite, and which adds no
callee bytecodes, locals, or registers to the caller image.
