# 110 — Constant-condition control stencils

Status: complete

Normalize `LoadLiteral(immediate); JumpIfFalse` to a direct control-flow morphism when
whole-function use analysis proves the temporary register dead outside the branch.
Truthiness is decided from the literal at link time; the copied stencil only transfers
to the selected symbolic successor while maintaining the connector's current-site value.

This is a general constant-folding rewrite over bytecode data. It supports undefined,
null, boolean, and numeric literals, including `0`, `-0`, and `NaN`; allocating string
literals stay canonical. It does not inspect source text, benchmark identity, branch
frequency, or runtime hotness.

Runtime evidence: `reports/semantic-block-profile.jsonl` records 425,447 entries for
the leading `LoadLiteral:Bool;JumpIfFalse` shape. The family is implemented with two
rustc/LLVM-cooked tail-transfer templates (truthy and falsey), selected as a pure
rewrite and patched to ordinary bytecode or explicit shared-exit labels.

Acceptance: selection tests cover truthy, falsey, exit targets, and a live-register
near miss; all tests and V8v7 smoke pass; an exact alternating six-run A/B against
[[109-normalized-basic-block-stencil-vocabulary]] improves aggregate without crossing
the standing -5% component floor. Revert and record rejection otherwise.

Result: accepted. Forty-six release tests and
`reports/constant-condition-stencil-smoke.jsonl` pass. The exact alternating six-run
comparison in `reports/constant-condition-stencil-ab-6/comparison.txt` raises aggregate
score from 827.602 to 830.452 (+0.34%). Richards, DeltaBlue, Earley-Boyer, Splay, and
Navier–Stokes improve; the worst component change is RayTrace at -0.55%.

Accepted binary SHA-256:
`a239bed0433b589ed5efcd9f41029041db382e7c76c566052c196989a6887bed`.
