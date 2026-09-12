# 418 — Straight-line register-region coverage

Status: in_progress

Permit the physical register planner's already-supported straight-line form. The existing
entry check requires `trace_header() == Some(start)`, so every quoted basic block—whose trace
header is correctly `None`—is rejected before liveness or stencil selection. Replace that
predicate with: reject only when a trace header exists and differs from the region start.
Screen a named four-operation minimum because the previous eight-operation amortization bound
makes the corrected block path inert; retain that lower bound only if physical and score gates
show that the entry/leave seams are repaid.

This changes no category object, stencil ABI, or semantic operation. A straight-line quoted
sequence and a single-header traced sequence both lower through the same
`Connector -> RegisterRegionConnector -> Connector` composition. Blocks simply omit the
backedge morphism. The existing target-range checks remain the authority for unsupported
control flow.

Acceptance: unit coverage for a straight-line numeric block; all release tests; rejection
census proves the former `NotSingleTrace` bucket falls and register-region selection rises;
native code remains finite rustc/LLVM-cooked copy-and-patch stencils; and an alternating
V8v7 A/B determines retention.

## Current evidence

With the original eight-operation bound, the corrected header predicate moved 78 attempts
from `NotSingleTrace` to `TooLittleNumericWork` but selected no new region. A named
four-operation bound selected 28 regions instead of 11 and raised counted alias updates from
24 to 37. The three-pair 100 ms development screen at
`reports/task418-four-op-block-screen-3/comparison.txt` measured +1.09% aggregate, with Crypto
at -2.47% and every component inside the development floor.

The first exact gate at `reports/task418-four-op-block-exact-9/` is invalid and was aborted
after five pairs. Many unrelated Deno test processes consumed multiple cores concurrently;
pair five collapsed to scores of 1170.81 and 368.32. Those raw files are retained as
contamination evidence, not performance evidence. The four-operation candidate is not
accepted until the randomized nine-pair exact gate is rerun on a quiet machine.
