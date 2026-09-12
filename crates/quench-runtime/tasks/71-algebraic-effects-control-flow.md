# 71 — Algebraic-effect representation for exceptions, generators, and async

Status: planned

JS's `try`/`catch`, generators, and `async`/`await` are currently special-cased escape hatches in the compiler (per `README.md`'s "exceptions" support and the exception-helper-stencil labels seen throughout `reports/richards-stencil.md`'s coverage output). Algebraic-effect calculi (Plotkin/Pretnar's effect handlers) were built specifically to model exactly this class of non-local control transfer compositionally — as ordinary morphisms with a handler that intercepts an effect operation, rather than a parallel control-flow mechanism bolted onto normal composition.

Model exception-raising, generator `yield`, and `await` suspension as algebraic effect operations within the `Stencil<In, Out>` category: an effectful stencil is a morphism that may either complete normally (its stated `Out` connector) or invoke a handler (a distinguished alternate connector state), with handler installation/removal itself expressible as ordinary composition rather than a distinct exception-table mechanism.

Concrete steps:
1. Audit current exception handling (`exception helper stencil with native exceptional edge`, per the coverage labels) and generator/closure suspension logic in `main.rs`/`dynjit.rs` to identify where control-flow special-casing exists outside the normal `Stencil` composition path.
2. Define an effect-operation connector state and handler-composition combinator analogous to `branch`/`loop_` in `main.rs`, so `try`/`catch` becomes a specific instance of effect handling rather than its own bespoke mechanism.
3. Re-express generator suspension/resumption and `async` continuation as the same effect-handling mechanism, testing whether one unified construct actually covers all three JS features or whether genuine differences require distinct (but still compositional) treatment.

Acceptance: exception handling, generator suspension, and async suspension are each expressed as instances of one effect-handling combinator, with the combinator's composition behavior law-tested the same way [[01-stencil-category-core]] tests identity/associativity; no regression in exception/generator/async correctness tests; the coverage report ([[04-bytecode-coverage-map]]) shows these constructs' stencil-selection labels reflecting the unified mechanism rather than three independent special-case descriptions.
