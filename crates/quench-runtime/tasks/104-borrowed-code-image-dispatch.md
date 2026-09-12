# 104 — Borrow linked code images during calls

Status: complete

Every user-function call currently clones either `Rc<LegoJitCode>` or `Rc<DynJitCode>`
out of its `RefCell` and drops that clone immediately after execution. Linked code
images are immutable shared kernels; a call needs only a scoped reference. Native
samples after [[103-owned-slot-overwrite]] place `call_arguments` high in Crypto's
recursive stack, making this ownership traffic worth isolating.

Borrow the optional code image for the duration of dispatch. Shared `RefCell` borrows
permit recursion, and compilation still occurs before the borrow because a missing
image is filled at the call boundary. No cache key, linking rule, or JS semantic path
changes.

Acceptance: all release tests, including recursive benchmark smoke, pass; then a full
alternating A/B against the exact [[103-owned-slot-overwrite]] binary clears aggregate
and component floors. Revert and record rejection otherwise.

## Result: rejected and reverted

All 43 tests and `reports/borrowed-code-image-smoke.jsonl` passed, but the exact
alternating six-repetition comparison in
`reports/borrowed-code-image-ab-6/comparison.txt` regressed aggregate score from
781.545 to 778.398 (-0.40%). Crypto regressed 2.65%, Splay 2.45%, and Navier-Stokes
0.86%. Keeping the `RefCell` borrow guard live through recursive execution costs more
than the short-lived Rc clone/drop pair on this implementation. The source was restored
to scoped image clones; no benchmark result from this experiment is retained.
