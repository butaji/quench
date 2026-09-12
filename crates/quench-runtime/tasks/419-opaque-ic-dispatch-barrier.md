# 419 — Opaque inline-cache dispatch barrier for rustc-cooked stencils

Status: planned

Deegen compiles inline-cache dispatch points through LLVM `callbr` (backed by GCC's
asm-goto): a control-transfer instruction whose destination the optimizer must treat as one
of several possible targets, unspecified which. That lets a static compiler emit code
adjacent to a self-modifying IC chain without proving anything false about the chain's
runtime target, and without the optimizer speculating across, hoisting past, or merging code
around the patch point.

Quench cooks stencils through rustc/LLVM, not raw asm-goto, so the same *problem* needs its
own boundary: an operand or region that the register-region planner and the stencil cooker
both treat as an opaque multi-target branch, so no fact from either side of an IC chain link
is allowed to flow across the patch site. Right now IC chaining relies on stencils being
compiled as physically separate units and relinked (Tasks 145, 153, 166); this task asks
whether that unit-of-compilation boundary is sufficient or whether a real optimizer barrier
(a rustc/LLVM intrinsic, an inline-asm clobber, or a documented invariant enforced by the
stencil-cooker audit) is needed to stop future codegen changes from silently assuming a
fixed target across a patchable dispatch.

Acceptance: a concrete counterexample search showing whether current unit-of-compilation
isolation can be violated by any planned or existing optimization pass; if so, a minimal
barrier construct plus a regression test that fails without it; disassembly proof that nothing
is hoisted or folded across the IC dispatch point; full correctness suite.

Primary source: sillycross, "Building a baseline JIT for Lua automatically"
<https://sillycross.github.io/2023/05/12/2023-05-12/> (CallBr / asm-goto section);
LLVM `callbr` instruction reference <https://llvm.org/docs/LangRef.html#callbr-instruction>.
