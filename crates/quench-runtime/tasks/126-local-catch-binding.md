# 126 — Local catch binding in stack frames

Status: complete

The Task 13 stack-frame audit found one semantic hole: function lowering includes a
catch parameter in its immutable local layout and rewrites later reads to `LoadLocal`,
but the `Catch` opcode still writes the thrown value through `frame.environment`. For a
non-capturing function that environment is now the outer lexical environment, so an
actually-taken catch would leave its local slot undefined and mutate the wrong scope.
V8v7 normally does not throw through these paths, so benchmark validation alone cannot
prove this invariant.

Represent the catch destination once as a tagged binding (`Name` for scripts, `Local`
for functions). Function binding lowering transforms the name to its slot; execution
pattern-dispatches on that data and writes either the canonical environment or local
store. This preserves stack frames without reintroducing a synthetic heap environment.

Acceptance: tests cover a taken catch in a non-capturing function, outer-name
non-mutation, and a closure-capturing catch; all release tests and complete V8v7 smoke
pass. Since this is a correctness repair, it is accepted on semantics, while an exact
binary/score checkpoint is still recorded before subsequent optimization.

Implemented `CatchBinding::{Name, Local}` as the single quoted destination
representation. Function-local lowering resolves `Name` to the immutable local slot;
the stencil executor then pattern-dispatches the destination without reconstructing a
heap environment. The regression test exercises an actually taken local catch, verifies
that a same-named outer binding is untouched, and verifies closure capture of the caught
value. All 63 release tests and the complete V8v7 smoke suite pass.

The exact accepted candidate is `/tmp/deegen-task126-local-catch-binding`, SHA-256
`98ea83e931d3137d64c738cf58b6e8f5140232a08a1c4b7cee8d07c2d7c3d80c`.
The alternating four-repetition, 200 ms-window comparison is recorded in
`reports/task126-local-catch-binding-ab-4/comparison.txt`: aggregate 1191.67 to
1182.88 (-0.74%). This cold-path representation change is accepted for correctness;
the last performance-improving gate checkpoint remains 1177.48 from Task 125.
