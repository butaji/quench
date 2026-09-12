# 315 — Path-lazy pure-computation sinking and duplication

Status: planned

After GVN/PRE, identify a pure value hoisted to a common dominator even though every use
lies in only a strict subset of successor paths. When duplicating the computation into
those paths costs less than executing it on the other paths, clone it at the nearest
legal consuming blocks and delete the eager common copy.

This is the inverse pressure to global common-subexpression elimination: sharing reduces
code size, while path-lazy duplication avoids work on paths that do not consume the
value. Only operations proven pure, non-throwing, non-allocating, ownership-neutral, and
cheaply duplicable may move. Loads require an identical Task 173 memory version at each
new location. Named code-growth, clone-count, latency, and loop-depth limits bound the
rewrite; no runtime profile or benchmark identity is used.

Represent the transformation as a copying reducer over immutable CFG/SSA. It returns the
same type of region and therefore composes with every other reducer before final tiling.

Acceptance: tests show an unused branch no longer performs an expensive pure operation;
negative tests reject division/coercion/load/ownership cases without sufficient proof;
the reducer is deterministic and bounded; emitted code and full-suite A/B show a gain
before enablement.

Primary source: V8's production account of GVN hoisting pure work too early and its
scheduler duplicating the work back into consuming paths:
<https://v8.dev/blog/leaving-the-sea-of-nodes>.

Depends on Tasks 62, 128, 164, 171, 173, 195, 209, 241, and 309.
