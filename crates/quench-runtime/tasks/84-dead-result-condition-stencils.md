# 84 — Dead-result fused condition stencils

Status: complete

Turn the general bytecode form
`LoadLocal; LoadLiteral; BinaryCompare; JumpIfFalse` into one closed branch morphism
when the three intermediate virtual registers have no readers outside those adjacent
consumer instructions. Because the compiler allocates virtual registers monotonically,
an all-use check is a simple, conservative proof that their materialized values are
dead. The fused stencil then reads the local and literal directly and patches its two
control-flow exits without performing redundant register writes or `Rc` ownership work.

Generate reusable rustc/LLVM templates with macros for:

- numeric ordering and equality predicates, with a numeric guard and canonical slow
  exit for coercion;
- strict equality/inequality against immediate null, undefined, and boolean literals;
- loose nullish equality/inequality, whose complete JS behavior is null-or-undefined.

This is a category-preserving quotient rewrite: the four-op path and the closed branch
stencil have the same input/output connector and effects, but the dead internal objects
are erased. Selection depends only on bytecode data and use structure, never source
locations, benchmark names, execution hotness, or exact V8v7 code.

Acceptance: semantic tests cover number, NaN, signed zero, null, undefined, boolean,
and coercion slow exits; direct-stencil counters prove selection; an alternating A/B
run accepts the optimization only if aggregate and per-suite regression gates pass.

Result: 37 release-mode tests pass, including structural selection tests for NaN,
signed zero, null, undefined, boolean, string/coercion rejection, and a deliberately
live intermediate. The complete-suite correctness smoke is
`reports/dead-condition-direct-smoke.jsonl`. Three-repetition alternating A/B evidence
in `reports/dead-condition-ab/comparison.txt` measured aggregate 565.349 to 577.923
(+2.22%), Earley–Boyer +4.49%, Splay +5.02%, and Navier–Stokes +5.00%, with no suite
outside the regression gate. Coarse direct stencils are therefore the default;
`DEEGEN_BLOCK_KERNEL_ONLY=1` exists only for diagnostic baselines and coverage still
uses the observable generic kernel.
