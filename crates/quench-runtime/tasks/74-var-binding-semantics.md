# 74 — Correct hoisted var binding and parameter redeclaration semantics

Status: complete

Represent function-scoped `var` declarations as binding facts independently from runtime assignments. An uninitialized `var` declaration emits no assignment because the fixed local frame is already initialized to `undefined`; an initializer still executes at its source position. Script-level declarations retain their existing global declaration path.

This fixes the generated Scheme pattern `function loop(l1, l2) { var l1; var l2; ... }`. The old lowering overwrote both parameters with `undefined`, causing Earley's derivation routine to return `undefined`; `sc_length` then looped forever over an invalid tail.

Evidence:

- `uninitialized_var_redeclaration_does_not_clobber_parameter` guards the language rule.
- All 34 release tests pass.
- Earley input size 1 changed from nontermination beyond 30 seconds to completion in 0.28 seconds.
- The unmodified Earley-Boyer suite changed from timeout beyond 60 seconds to a valid smoke score of 1183.
- `reports/full-var-fix.jsonl` is the first successful three-repetition recording containing all eight V8v7 suites.

The fix is benchmark-independent: it implements ECMAScript function-scoped `var` semantics in the bytecode lowering functor, preserving the same binding meaning in every downstream stencil composition.

