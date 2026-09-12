# 271 — First-class predicate and flags continuations

Status: planned

Generalize Task 268's compare-and-branch supernodes into a reusable representation:

- `Predicate<G>` is an unmaterialized truth fact in a named GPR or target flags state;
- comparison/truthiness/nullish/shape tests produce `Stencil<Gamma, Gamma × Predicate>`;
- branch, select, guard, and materialize-Boolean are consumers of that same context;
- only `MaterializeBoolean` constructs the observable JS `true`/`false` `Value`.

This is a sum-elimination interface, not another Boolean value kind. Target flags are
valid only across an immediately adjacent compatible consumer; a GPR bit can survive
ordinary pure connectors according to Task 158's liveness plan. Calls, effects, merges,
and incompatible stencils force an explicit `Predicate -> Boolean Value` or branch
before the clobber. Composition validity is enforced by context types and the target ABI
catalog, never by a runtime assertion.

Rust macros derive producer/consumer variants from one predicate recipe table. Include
numeric comparisons, nullish tests, property-presence tests, tag tests, and generic
truthiness only where their semantics are already defined once. Use named constants for
the predicate register budget and maximum compare-chain length. Do not add a Cartesian
product of every producer with every branch: Task 272's selector composes compatible
producer and consumer morphisms and chooses a fused cooked template only where LLVM
generates materially better code.

A predicate producer may be either copied code or a shared immutable kernel. Task 285
demonstrates the latter: a complete `instanceof` observation is too large and ownership-
sensitive to duplicate profitably, while a tiny stencil can consume its canonical
false/true return directly as control. Model this as the same `Predicate<G>` connector,
not as a special `instanceof` ABI. The recipe decides `Kernel -> Predicate` versus
`StencilInstance -> Predicate` from code-size/effect facts, and every branch/select/
materialize consumer remains unchanged.

Acceptance: dead Boolean materialization disappears for all represented branch families;
observable Boolean uses still materialize exactly; NaN, minus-zero, empty string,
null/undefined, object truthiness, and exception cases pass; disassembly shows direct
compare-and-branch or one-bit branch; catalog size stays within named budgets; full V8v7
A/B passes.

Primary sources: CPython's bit-predicate design and AArch64 instruction-count evidence
<https://github.com/python/cpython/issues/149238>; V8's `FlagsContinuation`, which
selects branch/set/select/deopt/trap consumption without prematurely materializing a
Boolean <https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/compiler/backend/instruction-selector.h>.
