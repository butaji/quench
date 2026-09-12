# 415 — Budgeted loop-invariant guard unswitching

Status: planned

Turn a loop-invariant semantic condition into a choice between whole loop morphisms rather
than rechecking it in every iteration. The first supported conditions are representation,
shape, dense-element kind, exact callee, and protector/epoch facts already modeled by the
quoted abstract context.

For an invariant condition `p` and loop `L`, derive:

```text
guard(p) ; trace(L_fast)  +  guard(not p) ; trace(L_total)
```

where `L_fast` and `L_total` are themselves composable loop-level `StencilExpr` values.
This is a compile-time CFG rewrite followed by normal physical covering. It does not add a
special loop node to the stencil kernel, inspect a benchmark name, count loop executions,
or invoke LLVM at runtime.

Run LICM/effect validation first. Clone only when the predicate is unchanged by every loop
effect and the fast arm erases a measured guard, conversion, property lookup, or indirect
call. Use named limits for candidate count, cloned blocks, copied bytes, nesting depth, and
total function-image growth. The general arm preserves complete JavaScript semantics and
side exits reconstruct canonical state through Task 330.

The transform composes at the loop abstraction level: operation and block stencils remain
unchanged, while the loop cover chooses between shared `Kernel` references and patched
`StencilInstance`s under the same connector category. Task 157 prices both forms; Task 291
may reject a locally faster clone that harms instruction-cache footprint.

Acceptance: invariant-proof and mutation-counterexamples; loop/header/exit label and
parallel-copy law tests; disassembly showing one preheader guard and no corresponding body
guard; copied-byte budget tests; full correctness; and randomized alternating complete
V8v7 A/B with no component-floor violation.

Primary sources: LLVM's production loop-unswitch transformation and cost threshold
<https://llvm.org/doxygen/classllvm_1_1SimpleLoopUnswitchPass.html>; JavaScriptCore's
description of loop unswitching through bounded tail duplication
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>.

