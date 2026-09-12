# 18 — Identity-node erasure as a proven peephole pass

Status: planned

`branch` and `loop_` insert `identity::<Ctx>().labeled(end_label)` as join points. Since `identity() == Stencil::empty()` and `a + identity == a` is a proven law (see `stencil_category_free_monoid_laws` in [[01-stencil-category-core]]), a link-time pass may delete any `Empty` node carrying no label reference, and merge a labeled-but-otherwise-empty node into the following node's label set. This removes no-op landing pads and reduces branch/jump count in the final image.

Distinguish this from [[17-seq-flatten-linear-link]]: flattening changes tree shape without deleting content; this pass deletes semantically-inert content, and is only safe because the identity law is a theorem rather than an assumption about the current combinators.

Acceptance: generated code size and jump count decrease on branch/loop-heavy functions; a labeled empty node whose label is target of a symbolic jump is never removed or is correctly retargeted; existing control-flow tests pass unchanged.
