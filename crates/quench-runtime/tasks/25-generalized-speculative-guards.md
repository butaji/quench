# 25 — Generalized speculative guard connectors

Status: planned

Extend the guard-typed connector mechanism in [[19-guard-typed-connectors]] from int32/f64 arithmetic to a general family of guard connectors: `ShapeGuarded<S>` (receiver hidden class stable, feeding [[08-property-inline-caches]]), `CallTargetGuarded<F>` (monomorphic call target, feeding [[27-callsite-devirtualization]]), and `ElementKindGuarded<K>` (dense/packed array backing, feeding [[12-array-fast-path]]). Each guard is a cheap check plus a fast typed path; composition-typechecking (left `Out` must match right `In`) is the single mechanism that decides how far an unguarded run can extend before re-checking, exactly as already specified for arithmetic chains — this task is that mechanism generalized, not a new mechanism per guard kind.

A failed guard is a coproduct choice: bail out to the general `Stencil<Connector, Connector>` path, which must remain semantically complete on its own (per [[03-stencil-only-execution]]'s no-fallback-hiding rule).

Acceptance: property, call, and array guard families share one generic guard-connector implementation rather than three bespoke ones; a guard failure at any of the three sites falls back correctly with no duplicated semantic logic between fast and slow paths; type-mismatched guard composition is rejected at compile time, not at runtime.
