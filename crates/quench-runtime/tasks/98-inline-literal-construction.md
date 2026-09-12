# 98 — Inline literal construction

Status: complete

Test forced LLVM inlining of `literal` into the semantic block kernel. The helper is a
five-way tagged construction and remains a call for every unmatched `LoadLiteral`.
Immediate undefined/null/bool/number values require only a word construction; string
literals retain the existing allocated slow representation.

Acceptance: tests, full smoke, executable-size record, and a balanced full-suite A/B.
Revert if copying the string arm into the immutable kernels harms locality more than
removing the common immediate-literal call helps.

## Result: rejected and reverted

The forced-inline candidate passed all 41 release tests and the full smoke suite in
`reports/literal-inline/smoke.jsonl`. Its executable was 2,990,624 bytes, 48 bytes
smaller than the 2,990,672-byte accepted task-97 executable.

The exact alternating four-repetition A/B in
`reports/literal-inline-ab/comparison.txt` regressed the aggregate from 675.593 to
672.581 (-0.45%). DeltaBlue regressed 8.62% and Splay regressed 6.47%, both beyond
the component acceptance floor; Richards also regressed 3.04%. Crypto, Earley-Boyer,
RegExp, and Navier-Stokes improved, but the aggregate and two component gates failed.
The host was visibly noisy during this run, yet the alternating medians still do not
justify retaining a failed candidate. The `#[inline(always)]` annotation was removed.

This experiment rules out indiscriminately cloning the allocating string arm into
every semantic block. A future literal specialization should keep string allocation
in an out-of-line cold kernel and encode only undefined, null, boolean, and number
construction in the caller.
