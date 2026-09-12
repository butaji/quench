# 353 — Closed native leaf-call stencils with burned caller operands

Status: complete

Replace Task 352's rejected bytecode-splicing approach with one coarse, closed stencil
morphism for a bounded leaf-call family. Selection is structural and source-independent.
The initial hypothesis was a property chain such as `parameter.car.cdr`; an opcode trace
falsified it because Earley's generated `sc_caar` family is defined but not called. The
executed closed family is instead identity, `typeof` equality, and `instanceof` predicates.

The quoted recipe is immutable data:

`ExactTarget × LeafExpression × CallerOperands × ResultRegister × FallbackLabel`.

The rustc/LLVM cooker produces templates for the finite leaf-expression algebra. Copy
and patch burns the exact target word, caller argument-register offsets, result-register
offset, literals, and both successor labels. The selected instance executes as one
machine-code block:

`IdentityGuard ; Shape/SlotChain ; StoreResult ; Continue`

and branches to the unchanged ordinary call stencil on an identity/type/overflow miss.
It must not append callee bytecodes, bindings, locals, or virtual registers to the caller
image. The closed leaf and ordinary call remain compatible
`Stencil<Connector, Connector>` morphisms and are composed by the existing free-monoid
linker; selection is a pure quote/rewrite phase before one final materialization.

Implementation order:

1. Add a small `LeafExpression` data algebra and quote it from eligible `DynCode` using
   the same `CallBindingRecipe` and static target resolver as Task 351.
2. Reuse the rustc-cooked direct-call connector for the closed leaf and bind it to a
   first-execution monomorphic shape/slot-chain kernel. The call target, argument, and
   result are call-site operands, not new virtual registers.
3. Add a closed call-site stencil selector which emits the native leaf plus the existing
   ordinary-call fallback and continuation labels.
4. Law-test deterministic quoting, operand substitution, identity fallback, numerical
   semantic equivalence, and category composition.
5. Count selected/executed/missed families only behind an instrumentation feature, run
   all release tests and V8v7, and retain only after a long interleaved A/B gain.

Do not introduce hotness detection, benchmark/source names, a general runtime compiler,
or a second definition of JavaScript arithmetic semantics. If the finite family does
not cover enough calls, report its exact structural rejection census before expanding
the algebra.

Outcome: rejected from the runtime. The prototype quoted exact direct-binding callees
into a finite `LeafExpression` algebra with `Identity`, `TypeofEquals`, `InstanceOf`, and
`PropertyChain` constructors. It forced only selected call PCs through the existing
rustc-cooked direct-call stencil, evaluated the closed expression without creating a
callee activation, and retained the ordinary call morphism on target, constructor, or
shape misses. No heat threshold or source-name condition was used.

The path was demonstrably live. At a 20 ms Earley-Boyer window it linked 11 sites and
reported 83,394 attempts, 83,394 hits, and zero misses. A focused smoke verified identity,
`typeof`, `instanceof`, property chains, callee reassignment, and constructor reassignment.
All 128 release tests passed. Nevertheless, a three-pair 200 ms interleaved comparison
against the frozen accepted binary measured a median aggregate of 2382.41 for baseline
and 2381.90 for candidate (-0.02%). Earley-Boyer itself measured 2845 versus 2831.

This rejects the hypothesis that removing the small initially-eligible leaf family can
move V8v7. It also shows that a shared Rust callback behind the direct-call connector is
still too fine-grained: activation removal did not amortize the call boundary. The next
call experiment must cover the multi-million computed property/receiver call population
and put its shape guard, target load, and operation in the cooked machine-code stencil.
The experimental module, fixture, call-site payload, forced tiling, and counters were
removed. The default binary returned byte-for-byte to
`a02330525ba32d3d371027349f5029c75a5120c9f966644783c66053bafc9688`.
