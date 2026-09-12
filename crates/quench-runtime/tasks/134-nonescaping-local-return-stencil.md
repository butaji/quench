# 134 — Non-escaping local ownership return stencil

Status: complete

Specialize the structural `LoadLocal(dst, slot); Return(dst)` terminal block when the
function frame provably cannot escape. Move the local's one-word owner directly into the
empty result slot and clear the local. The intermediate register write is dead by the
existing liveness predicate, so the two bytecodes lower to one coarse AOT stencil.

The capability predicate is general and static: a function frame is non-escaping only when
it has no hoisted nested function and no `MakeClosure` bytecode. Capturing functions retain
the existing copy-and-guard stencil because their environment may outlive the call. This
turns escape analysis into an explicit choice between two morphisms with the same observable
input/output behavior but different ownership contexts.

Acceptance: structural near-miss and capture rejection tests, cooked Rc ownership transfer,
full release tests, complete smoke, residual coverage delta for `LoadLocal,Return`, and an
alternating full-suite A/B with standing floors.

## Result: rejected and reverted

The experiment added a rustc/LLVM-cooked two-bytecode stencil plus a static frame-escape
predicate. Its cooked ownership test, structural capture tests, all 76 release tests, and the
complete smoke passed. In the residual profile it reduced `LoadLocal,Return` generic entries
from 264,865 to 200,816; the remaining entries were predominantly Earley-Boyer functions
whose frames genuinely escape through nested closures.

The four-run, 500 ms alternating full-suite measurement in
`reports/task134-owned-local-return-full-ab-4/comparison.txt` moved 1177.89 to 1173.03
(-0.41%). The result passed the standing component floors but did not provide a repeatable
gain, so the implementation and its temporary tests were reverted. Task 133 remains the
accepted source state, confirmed by rebuilding the exact SHA-256
`9cd16751b8cc6a8ba9b7453f47f635aeac07a21f7ca663508dc22ddefaa73a24`.

The evidence remains in `reports/task134-owned-local-return-smoke.jsonl` and
`reports/task134-owned-local-return-residual.jsonl`. A future revisit must avoid adding a
larger terminal template for only this low residual coverage, or combine the transfer into a
materially coarser whole-block stencil.
