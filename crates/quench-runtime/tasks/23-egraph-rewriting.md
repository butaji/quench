# 23 — Quotient-category rewriting (e-graph style)

Status: planned

Generalize [[18-identity-erasure-peephole]] from a single proven law (`a + identity = a`) into a general equality-saturation framework. Represent a region of `StencilNode`s as a quotient category: nodes are grouped into equivalence classes under a fixed set of semantics-preserving rewrites (constant folding of pure literal compositions, algebraic identities such as `x + 0` / `x * 1`, common-subexpression sharing via structural or `Rc` pointer identity), and a cost-based extraction picks the cheapest representative per class at materialization time — the same point where [[17-seq-flatten-linear-link]] already performs its one deferred pass.

Each rewrite rule must itself be proven or tested against the same law style used for identity/associativity in [[01-stencil-category-core]]; this is a framework for adding proven rewrites cheaply, not a general unverified peephole matcher. Modeled on Cranelift's ISLE/aegraph approach and the `egg` library's equality saturation.

Acceptance: adding a new rewrite rule requires no changes to the extraction or scheduling logic; extraction is deterministic and picks the documented minimal-cost representative; a benchmark with redundant subexpressions shows measured code-size and instruction-count reduction; existing composition and category-law tests pass unchanged.
