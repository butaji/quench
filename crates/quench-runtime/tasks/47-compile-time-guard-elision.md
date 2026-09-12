# 47 — Compile-time guard elision for provably stable bindings

Status: planned

Requires [[46-closed-world-mode]]: under closed-world guarantees, a binding whose constructor/shape is never reassigned anywhere in the whole program can have its guard eliminated entirely at compile time rather than merely cached at runtime per [[19-guard-typed-connectors]]/[[25-generalized-speculative-guards]].

Concrete steps:
1. Build a whole-program mutability analysis (under [[46-closed-world-mode]]'s colimit pass) that marks a binding/class "closed" when no assignment site anywhere in the program can change its shape or reassign the constructor.
2. Extend guard connector selection ([[25-generalized-speculative-guards]]) to check this analysis result first: if closed, emit the unguarded fast-path stencil directly with no runtime check; otherwise fall back to the existing guarded/bounded-polymorphic path ([[31-bounded-polymorphic-guards]]).
3. Add a correctness test that mutates a "closed" binding through a reflection-like path (if any exists in the supported language subset) to confirm the analysis is sound, not just optimistic.

Acceptance: a property access on a provably-closed shape compiles with zero runtime guard instructions, verified by inspecting emitted code; a binding incorrectly assumed closed is caught by the mutability analysis's own test suite, not discovered as a runtime correctness bug; the guarded fallback path remains available and correct for every binding the analysis cannot close.
