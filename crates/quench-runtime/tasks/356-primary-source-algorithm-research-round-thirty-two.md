# 356 — Primary-source algorithm research, round thirty-two

Status: complete

Research additional general-purpose VM/compiler algorithms after Task 355 proved that
both a large copied call stencil and a small connector to a shared Rust call kernel lose
on V8v7. The standing constraints remain: OXC plus Rust, no third-party VM, stencils from
first execution, no runtime-hotness tier trigger, rustc/LLVM performs expensive generation
only in the AOT cooker, and V8v7 remains a holdout rather than a source of special cases.

## New empirical fact

Task 355's shared-kernel variant reached 1,787,922 Richards calls in 100 ms yet regressed
the complete aggregate 5.04%, including -11.79% Earley-Boyer and -8.79% RayTrace. The
connector was only 112 bytes, so code duplication is no longer a sufficient explanation.
The executed native -> Rust ABI -> native transition is itself the rejected boundary.

## Primary-source synthesis

1. **Calls must be CPS machine-state morphisms, not helper functions.** Deegen models a
   guest call as a non-returning transfer with a typed return continuation, supports
   in-place calls where the callee frame starts directly at the argument range, and makes
   IC stubs operate on the surrounding machine state rather than a fixed function ABI.
   This directly selects Tasks 146/181 and rules out another call-kernel wrapper.
   Source: <https://arxiv.org/html/2411.11469>, Sections 4.4 and 7.1 plus Appendix A.3.5.

2. **Use one argument-count-bearing guest frame, no adaptor frame.** V8 removed its
   argument-adaptor frame by reversing caller arguments, guaranteeing enough slots for
   formals, carrying the actual count in the callee frame, and cleaning the variable-size
   frame in the epilogue. It reported 4.6% Richards and 6.1% Earley-Boyer improvements in
   Octane2. This supplies a concrete layout algorithm for Task 146, including mismatched
   arity without a Rust allocation/copy stage. Source: <https://v8.dev/blog/adaptor-frame>.

3. **A custom register ABI is part of the optimization, not incidental glue.** V8's
   CodeStubAssembler uses a custom register ABI, machine tail calls, and frameless leaf
   code specifically to avoid C++ trampolines. The rustc cooker should generate guest ABI
   kernels and continuation fragments whose pinned-register contract is verified from
   object-code relocations/disassembly. A normal `extern "C"` call is a slow-edge tool.
   Source: <https://v8.dev/blog/csa>.

4. **Keep baseline frames compatible, then add a small optimizing region layer.**
   Sparkplug uses interpreter-compatible frames and compiles bytecode directly without an
   IR, but V8 needed Maglev's compact SSA CFG to keep values unboxed, propagate known shape
   facts, select representations, and perform a simple forward register allocation.
   Stencils solve compilation latency; they do not eliminate repeated tagging or frame-slot
   traffic by themselves. This selects Tasks 171/144/152/158 after the call ABI lands.
   Sources: <https://v8.dev/blog/sparkplug> and <https://v8.dev/blog/maglev>.

5. **IC recipes should separate immutable code shape from per-site data.** SpiderMonkey's
   CacheIR restricts a stub to guards, idempotent operations, and one terminal result.
   Baseline stub fields live outside shared JIT code, while uncommon complicated work can
   be a shared helper. This is exactly the `Kernel` versus patched `StencilInstance` choice:
   share identical code shape, burn only fields whose direct use removes a dependency, and
   inline the arm only when it runs in the caller's machine context. Source:
   <https://firefox-source-docs.mozilla.org/js/cacheir.html>.

6. **Bounded block versioning is the smallest plausible C-like numeric tier.** Lazy BBV
   reported eliminating 71% of dynamic type tests and speedups up to 50%; interprocedural
   BBV reported 94.3% test elimination and up to 56% speedups by propagating parameter and
   return facts. Because this VM forbids runtime-hotness selection, use compiler-worklist
   demand with a named version cap, seeded by static/literal and installed IC facts. This
   maps to Tasks 144/152/177, not a new execution subsystem. Sources:
   <https://arxiv.org/abs/1411.0352> and <https://arxiv.org/abs/1511.02956>.

7. **Executable locality is a correctness condition for stencil economics.** Task 355's
   first variant empirically demonstrates the copied-code failure mode. V8's short-builtin
   work allocates generated code close enough to shared builtins for short direct calls;
   LLVM JITLink supports pre-reserved address ranges and link-time relaxation. Task 189's
   near-code immutable kernel arena and Task 291's instruction-cache budget should govern
   physical realization, but neither can rescue a semantically expensive helper boundary.
   Sources: <https://v8.dev/blog/short-builtin-calls> and
   <https://llvm.org/docs/JITLink.html>.

## Ranked experiment queue

1. **Task 146/181: direct in-place guest call/return continuum.** Implement one exact-
   arity non-capturing slice first, but use the final variable-size guest-stack frame and
   direct patched return continuation. Required disassembly: no `execute_direct_call`, no
   Rust prologue/epilogue, no host ABI spill sequence on the successful edge.
2. **Task 145/153: machine-state property/call IC slab.** Once direct calls exist, compose
   `GuardShape ; LoadSlot ; GuardCallee ; PushGuestFrame ; JumpCallee` as one IC arm with a
   shared slow kernel. It must not be a callable stub.
3. **Tasks 171/144/152/158: bounded SSA block versions.** Carry number/int32, shape, and
   location facts through whole loops; unbox once at entry, retain values in registers,
   and rebox only on explicit side exits.
4. **Tasks 316/348: burn operands and link all control edges in two passes.** A stencil
   which reloads `InlineSite` has not compiled bytecode into CPU operands. Resolve frame
   offsets, literals, IC data, and successor/continuation targets at final link.
5. **Tasks 157/189/291: cost physical realization.** Choose shared `Kernel` versus copied
   `StencilInstance` by eliminated seams and executable bytes. Track copied bytes per
   function, branch reach, and aggregate hot text; retire any large instance that retains
   a helper boundary.
6. **Tasks 162/193/320: nursery after precise roots.** Once guest frames and register maps
   are canonical, replace per-value ownership churn with bump allocation and bulk young
   reclamation. Do not micro-optimize `Rc` while it remains the semantic ownership model.

The ordering is intentionally hierarchical and Lisp-shaped: quote bytecode/function/IC
descriptions as immutable data, macroexpand facts and versions to a fixpoint, tile with one
cost algebra, and perform one final copy/patch/link effect. No benchmark-shaped stencil or
new parallel representation is introduced by this research.
