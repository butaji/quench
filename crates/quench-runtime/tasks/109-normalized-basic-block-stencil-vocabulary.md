# 109 — Normalized basic-block stencil vocabulary

Status: complete

Replace more entries into the generic `dyn_block_step_impl` opcode switch with coarse,
rustc/LLVM-cooked basic-block stencils selected solely from normalized bytecode structure.
First collect `DEEGEN_BLOCK_SHAPE_TRACE` across the complete suite, count repeated shapes,
and choose a general semantic family with broad coverage. Source locations, identifiers,
literal values, benchmark names, and runtime hotness must not participate in selection.

The representation remains staged and compositional:

- immutable normalized bytecode form is the quoted input;
- a pure selector rewrites a supported form to one `StencilTemplate` family;
- operands and CFG labels are copy-and-patch obligations in `StencilInstance`;
- the linked block has the same `Connector -> Connector` categorical interface as kernels
  and existing direct stencils;
- a semantic miss tail-calls the one canonical slow adapter.

Avoid the rejected [[32-element-kind-guarded-arrays]] `deegen_dyn_run` design: a copied
stencil containing another opcode loop/switch is not a fused block. The cooked handler
must express the fixed semantic structure directly so LLVM can optimize across operations.

Acceptance: record the shape-frequency artifact and coverage calculation; add unit tests
for positive selection, near-miss rejection, fast-path execution, and slow-path agreement;
all tests and V8v7 smoke pass; an exact alternating A/B against
[[108-batched-call-binding-initialization]] improves aggregate without crossing the
standing -5% component floor. Revert and record negative evidence otherwise.

Stage 1 — terminal blocks: `reports/block-shape-vocabulary/compile-frequency.txt`
contains 1,411 normalized compiled shapes. Its leading terminal forms are `Return`
(225 compiled blocks), `LoadLocal;Return` (112), and `LoadLiteral;Return` (102). These
now select three macro-authored rustc/LLVM stencils. Immediate results stay in copied
code; heap results take the canonical slow edge so `Rc` ownership remains exact. A
per-function exit connector targets one shared immutable exit kernel.

All 45 tests and `reports/terminal-block-stencil-smoke.jsonl` pass. The exact six-run
comparison in `reports/terminal-block-stencil-ab-6/comparison.txt` is essentially
neutral but clears the stated gate: 831.281→831.681 (+0.05%), with all component
changes between -1.83% and +2.70%. Direct compiled blocks rose from 8 to 28 in Richards
and from 71 to 159 in Crypto. Keep this connector/coverage infrastructure while adding
the next runtime-ranked coarse family; do not mistake this small result for the desired
structural speedup.

Stage 2 — unconditional transfer: the runtime semantic profile records 284,721 entries
for single-op `Jump` blocks. Selecting the existing rustc-cooked jump stencil exposed
and then fixed a symbolic CFG edge case: `target == code.len()` now maps to the explicit
shared function-exit label. All 45 tests and
`reports/direct-jump-stencil-smoke-fixed.jsonl` pass. The exact six-run comparison in
`reports/direct-jump-stencil-ab-6/comparison.txt` raises 817.699→825.998 (+1.01%); no
component is below -0.45%. The accepted binary SHA-256 is
`7cac9a8c7d3df8385f490b3afa9e4788b31ff3464d9d7eca9a6d37869d3cb67f`.

The normalized data representation, shared-exit connector, terminal family, and direct
unconditional transfer establish the intended vocabulary mechanism. Further families
are separate measured work items beginning with [[110-constant-condition-control-stencils]].
