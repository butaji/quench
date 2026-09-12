# 247 — Fused pointer-rotation stencil for splay-tree restructuring

Status: planned

splay.js's entire hot path is `SplayTree.prototype.splay_`
(`splay.js:293`), the top-down splay algorithm: repeated tree rotations, each
reassigning a small, fixed set of `.left`/`.right` pointers between 2-3 node objects in
one restructuring step. This is a distinct shape from every pattern already targeted in
this session: not a traversal (earley-boyer's cdr-chase, [[221]]), not a
construction (raytrace's field-setter, [[222]]), but an **in-place multi-pointer swap**
— read 2-3 existing pointer fields, write 2-3 new pointer values across 2-3 different
node objects, as one indivisible logical operation (a single rotation), currently
compiled (implicitly) as N independently-guarded property stores scattered across
whichever objects the rotation touches.

**This is a direct instance of [[240-fibered-kernel-selection-over-shape-lattice]]'s
product-kernel criterion, not a separately-justified fusion.** [[240]] states precisely
when two or more independently-proven facts should compile to one product kernel rather
than nested/sequential guards: when the facts' fibers are genuinely independent (proving
one node's shape tells you nothing new about the others') and each participating node
shares the same tree-node shape. A rotation's 2-3 nodes satisfy exactly this — the
combined guard is a product of independently-established shape facts, not a dependent
sum — so this task's acceptance should be read as an application of [[240]]'s already-
stated criterion, with the concrete evidence and rotation-shape enumeration supplied
here.

Every rotation in a splay tree is one of a small, fixed number of shapes (zig, zig-zig,
zig-zag, and their mirror images — the standard top-down splay case analysis
`splay.js`'s own comment at line 287 cites as "the simplified top-down splaying
algorithm"), each touching the same small, statically-known set of node fields in the
same statically-known pattern. This is exactly the kind of small, closed,
frequently-repeated multi-field operation [[130-static-property-region-stencils]] and
[[222]]'s product-introduction framing already handle for single-object field writes —
this task extends that idea to a multi-object pointer-swap region: recognize a
rotation's whole read-then-write sequence as one region ([[128-effectful-region-vocabulary]]'s
coarse-region vocabulary is the natural home for this) and compile it as one fused
stencil doing 2-3 direct field reads and 2-3 direct field writes with a single combined
shape guard (all participating nodes share the tree's node shape, provable once, not
re-checked per pointer touched), rather than N separately-dispatched property-store
stencils each paying their own guard/dispatch cost.

Concrete steps:
1. Enumerate the small, fixed set of rotation shapes `splay_` actually performs (zig,
   zig-zig, zig-zag and mirrors — confirm the exact set directly against
   `splay.js:293-370` rather than assuming the textbook shapes match this
   implementation's variable-naming exactly).
2. Recognize each shape at the compiler level as a bounded region: a fixed sequence of
   field reads across up to three node-typed locals followed by a fixed sequence of
   field writes across the same locals, with no intervening call/effect (matching
   [[222]]'s totality/no-interleaved-effect precondition, applied here to a multi-object
   region instead of a single constructor).
3. Compile each recognized shape to one fused stencil: one combined shape guard, direct
   field accesses with no per-field IC dispatch, matching [[130]]'s existing static-
   property region approach extended across multiple objects in one region.

Acceptance: at least one rotation shape from `splay_` compiles to one fused
multi-pointer-swap stencil with a single combined shape guard, verified by instruction-
count comparison against today's N-independent-store baseline; splay-tree correctness
tests (insert/remove/find, including tree-shape invariant checks) pass unchanged;
alternating A/B on the splay suite shows a measured gain with the full V8v7 suite at or
above the standing regression floor; a rotation variant not matching the recognized
closed set correctly falls back to the existing per-field-store path, not a
miscompiled fused stencil.

Source: `/private/tmp/js-engine-benchmark/v8-v7/splay.js:283-370` (local V8v7 corpus
checkout, the `splay_` method and its cited top-down splaying algorithm reference).
