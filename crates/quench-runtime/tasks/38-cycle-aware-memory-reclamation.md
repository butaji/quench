# 38 — Cycle-aware memory reclamation

Status: planned

`Rc<RefCell<Object>>` never collects reference cycles. deltablue's constraint graph (constraints referencing constraints via `input`/`output`) and richards' scheduler (tasks referencing a scheduler that references tasks back) both build cyclic object graphs that leak under pure refcounting for any long-running instance. Escape analysis ([[24-escape-analysis-scalar-replacement]], [[33-allocation-site-escape-analysis]]) removes allocations that provably don't escape, but cannot help anything that genuinely escapes into a cycle — a disjoint problem needing a disjoint mechanism.

Introduce either a tracing collector for objects that escape past function-local scope, or a region/arena allocator scoped to a natural execution boundary (a benchmark run, a top-level statement) so cyclic garbage is reclaimed in bulk rather than individually traced. Frame the chosen mechanism categorically as a scope parameter threaded through composition (an allocation region as context the stencil category is indexed over, freed at scope exit) so collection is a boundary operation consistent with [[01-stencil-category-core]]'s "effects occur only at the link/execute edge" constraint, rather than an ad hoc pass bolted onto the existing `Rc` model.

Acceptance: a synthetic cyclic-reference test (mutually referencing objects going out of all reachable scope) demonstrates memory reclamation, which the current `Rc`-only model cannot do; deltablue and richards show bounded memory growth over an extended run instead of monotonic leak; no regression in the existing correctness suite from introducing collection pauses or region boundaries.
