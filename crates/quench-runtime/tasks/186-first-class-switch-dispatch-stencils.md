# 186 — First-class switch-dispatch stencils

Status: planned

Preserve an eligible JavaScript `switch` as quoted `SwitchPlan` data instead of lowering
it immediately to a linear chain of strict comparisons and jumps:

- dense integer literals -> guarded range check plus explicit patched jump table;
- sparse integer literals -> balanced comparison tree;
- interned string literals -> frozen atom table;
- effectful or nonliteral case expressions -> canonical ordered semantic chain.

Tables contain symbolic labels and are linked by the existing hole machinery. Do not ask
LLVM to discover a jump table whose relocations Task 102 cannot represent. Density,
maximum table span, and tree/linear crossover are named target policy constants and the
costed selector returns an ordinary composable stencil expression.

Acceptance: evaluation order, fallthrough, default placement, duplicate cases, signed
zero, NaN, strings, and side-effecting case expressions pass; dense/sparse/atom plans
are structurally selected without benchmark identity; the current 13 V8v7 switches are
reported by plan kind; full A/B improves without excessive code growth.

Primary sources: V8 `SwitchOnSmiNoFeedback`,
<https://chromium.googlesource.com/v8/v8/+/e0a28a6c432486017f6961bc4ba746c7b64a8a0d/src/interpreter/interpreter-generator.cc>,
and JSC baseline switch linking,
<https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/jit/JIT.cpp>.

