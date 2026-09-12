# 24 — Escape analysis and scalar replacement

Status: planned

Related negative evidence: [[76-noncapturing-stack-frames]] proved that merely changing the frame container is insufficient. Combined and fixed-inline local storage both failed the per-suite regression gate. The next implementation should eliminate allocations/aliases as a proven rewrite, rather than substitute another generic container.

Prove, per allocation site, that an object never escapes its function (no capture into a closure, no store into a field of another object, no return, no pass to an unknown callee). Where provable, replace the `Rc<RefCell<Object>>` allocation with individually tracked register/stack slots for its fields — eliminating the allocation, the `RefCell` borrow checks, and the refcount traffic identified as the dominant per-object cost.

Model this as a functor from the "heap category" (stencils operating on `Rc<RefCell<Object>>`) to the "register category" (stencils operating on plain `Value` slots), legal only when an escape-check morphism over the allocation's use-sites succeeds — the same shape as the guard-typed connectors in [[19-guard-typed-connectors]] and [[25-generalized-speculative-guards]], applied to allocation instead of arithmetic type or shape. Depends on [[07-hidden-classes]] and [[09-object-memory-model]] for a stable notion of an object's field layout to scalarize against.

Constraint: this is a static, provable analysis at compile time, not a speculative guard — an allocation that cannot be proven non-escaping is compiled through the ordinary heap path with no behavior change and no guard/deopt machinery required.

Acceptance: a function allocating a short-lived object used only locally (for example, a temporary record passed nowhere) compiles with zero heap allocations, verified by an allocation counter; a function where the same object is passed to an unknown callee is correctly left on the heap; object-identity and mutation-visible-through-aliasing tests catch any unsound escape judgment.
