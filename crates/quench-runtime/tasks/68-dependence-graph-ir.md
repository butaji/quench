# 68 — Dependence-graph IR generalizing StencilNode from a tree

Status: planned

V8/JSC/HotSpot's real optimizers (TurboFan, DFG/FTL, C2) abandon tree-shaped IR in favor of a sea-of-nodes graph where both control and data dependencies are first-class edges — this is what enables aggressive scheduling, global value numbering, and reordering that a tree shape structurally cannot express (a tree forces an artificial total order on operations that have no real dependency between them). `StencilNode` today is tree-shaped (`Seq { parts: Vec<Rc<StencilNode>> }`).

Evaluate whether the categorical structure should generalize from "category of trees" to "category of dependence graphs" *before* [[67-egraph-as-primary-strategy]]'s rewrite-rule investment compounds on a substrate that may be too weak to express the rewrites that matter most (in particular, [[64-commutative-parallel-composition]]'s commutativity detection and [[29-induction-variable-strength-reduction]]'s cross-statement rewrites are naturally graph-shaped problems being forced through a tree-shaped representation today).

Concrete steps:
1. Identify at least three concrete rewrites already planned elsewhere in this task set that are awkward or impossible to express cleanly over `StencilNode`'s tree shape but natural over a dependence graph (candidates: [[64-commutative-parallel-composition]]'s commutative-block reordering, [[29-induction-variable-strength-reduction]]'s induction-variable rewrites, [[09-object-memory-model]]-adjacent aliasing-sensitive rewrites).
2. Define a dependence-graph category (nodes = operations, edges = data/control dependency) as either a replacement for or a richer view alongside `StencilNode`, preserving the existing category laws ([[01-stencil-category-core]]) over the new object shape.
3. Prove the existing tree-shaped composition (`Seq`, `+`, `identity`) embeds into the graph category as a special case (a tree is a graph with no reordering freedom), so no existing proof is invalidated, only generalized.

Acceptance: the three identified awkward-on-trees rewrites are demonstrably natural and correctly expressible over the graph representation; the embedding of the current tree-shaped category into the graph category is proven, not merely asserted; this task's outcome is a documented go/no-go decision (backed by the above evidence) on whether to actually migrate `StencilNode`, not an assumption that migration is obviously correct.
