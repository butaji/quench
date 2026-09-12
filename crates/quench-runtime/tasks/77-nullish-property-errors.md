# 77 — Correct nullish property-access errors

Status: complete

Dynamic `GetStatic`, `GetComputed`, `SetStatic`, and `SetComputed` now reject `null` and `undefined` receivers through the ordinary stencil error edge. JavaScript requires this failure; returning `undefined` masked upstream semantic bugs and turned finite failures into unbounded loops.

Evidence: `nullish_property_access_fails_instead_of_returning_undefined` covers the read path, the Earley diagnostic now fails immediately at the invalid access before [[74-var-binding-semantics]] is applied, and the complete V8v7 suite passes after both corrections.

Remaining property coercion and exception-object fidelity belong to the broader property/runtime semantics work, but silent nullish access is no longer possible in stencil execution.
