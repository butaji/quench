# 86 — Fused local numeric recurrence stencils

Status: complete

Close two reusable loop-recurrence forms as rustc/LLVM-generated stencils after proving
their expression temporaries have no uses outside the region:

1. `LoadLocal; LoadLiteral; Add/Sub; StoreLocal; Jump`;
2. `LoadLocal; LoadLiteral; Add/Sub; StoreLocal; LoadLiteral; Compare; JumpIfFalse`.

The second family is the product of two update operators and six numeric comparison
relations. Equality and strict equality share a numeric fast-path template because their
semantics coincide after the number guards; nonnumeric/coercing inputs take the canonical
slow edge. Each instance mutates only the local recurrence slot and transfers directly
to its patched successor. Dead register materializations disappear.

This is a block-level stencil schema over bytecode data, not one frozen benchmark body.
Selection uses opcode/operand topology and whole-function register-use facts only. It
does not inspect source names, lines, benchmark identity, execution counts, or literal
values.

Evidence motivating priority: `reports/semantic-block-profile.jsonl` records roughly
1.67 million entries for the seven-op subtract/store/greater-equal form and about 88
thousand entries for a five-op add/store/jump form.

Acceptance: selection/liveness tests pass; complete V8v7 correctness smoke passes;
compiled direct-block counters increase; alternating A/B satisfies aggregate and
per-suite regression gates before the family remains enabled by default.

Result: 38 release-mode tests pass, including recurrence selection and rejection when
an intermediate remains live. `reports/recurrence-smoke.jsonl` contains a correct
complete-suite run. Three-repetition alternating evidence in
`reports/recurrence-ab/comparison.txt` measured aggregate 564.916 to 570.644 (+1.01%)
and Crypto 378 to 414 (+9.52%); all other suites remained within the regression gate.
The general recurrence family remains enabled by default.
