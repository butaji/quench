# 17 — Linear-time Seq flattening at link time

Status: planned

`StencilNode::Seq { parts: Vec<Rc<StencilNode>> }` is a free monoid: sequencing is associative and `Empty` is the identity, both already covered by `stencil_category_free_monoid_laws` in [[01-stencil-category-core]]. Composition across opcode, block, loop, and function levels currently risks re-flattening or re-walking nested `Seq` nodes at each abstraction level. Defer flattening to a single pass performed once, immediately before an `image: Rc<OnceCell<MaterializedStencil>>` is forced, instead of at every intermediate `+`.

Because the law is proven, the flatten pass may freely re-associate any nesting of `Seq` without a correctness argument beyond the existing tests — the risk is purely a performance one (link time), not a semantic one.

Acceptance: a benchmark measuring link time as a function of function size and nesting depth stays linear, not quadratic; the flattened `Seq` produces byte-identical output to the unflattened composition on the existing composition tests.
