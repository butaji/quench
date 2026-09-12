# 372 — Quoted register-copy and dead-definition rewrite

Status: complete

Run one conservative, load-time simplification over canonical `DynCode` before any stencil
selection. This is a Lisp-style quote-to-quote rewrite: bytecode remains immutable data
until the rewrite reaches a fixed result, and only the final form is linked into executable
stencils. It is not runtime hot-path detection and does not inspect source identity.

The first rule coalesces an adjacent producer followed by `Move` when the producer can write
the move destination directly, the source dies at that boundary, and the destination is
not read by the producer. The second rule removes dead definitions only for operations with
no allocation, coercion, mutation, call, throw, or other observable effect. Control targets
are symbolic bytecode boundaries during the rewrite and are remapped after compaction;
`Jump`, `JumpIfFalse`, `ForInNext`, and exception-handler targets share the same checked
remapper. Invalid or completely erased programs fail closed.

This is the smallest register-level instance of the same categorical composition law used
by function and block stencils: replacing `producer ; copy` by an observationally equal
producer is a quotient of morphisms, and the quotient is applied before the lowering
functor. Coercive arithmetic and comparison operations remain untouched even when their
result is dead.

## Evidence

- 136 release tests pass, including focused copy-coalescing, both-edge PC remapping, and
  coercive-binary retention tests.
- The complete 50 ms smoke passed all eight V8v7 suites.
- In matched residual censuses, RayTrace's `LoadLocal,Move,Jump` shape fell from 464,694
  entries to zero. Earley-Boyer's `LoadLocal,Move,Jump` fell from 10,570 to zero and
  `LoadLiteral:Null,Move,Jump` fell from 499,938 to zero. The unrelated
  `LoadLocal,Return` shape remained exactly 565,104, confirming bounded scope.
- The first three-pair 200 ms full-suite comparison moved the geometric mean from 2398.75
  to 2407.42 (+0.36%). The stronger five-pair 300 ms comparison moved 2348.68 to 2360.29
  (+0.49%); every suite stayed within the standing regression floor.

The rewrite is retained because it deletes nearly one million measured stencil operations
in the two largest targeted residuals, simplifies the canonical quoted program, and is
small-positive in both alternating measurements. It is infrastructure, not the score-gate
breakthrough: the authoritative exact score remains Task 365's 2390.00712577743 until a
new exact run is justified by a larger candidate.

Reports:

- `reports/task372-smoke.jsonl`
- `reports/task372-current-residual-census/`
- `reports/task372-candidate-residual-census/`
- `reports/task372-register-simplify-ab-200ms-3/`
- `reports/task372-register-simplify-ab-300ms-5/`

