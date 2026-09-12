# 168 — Contiguous non-capturing call frame

Status: complete

Implement the first independently measurable slice of Task 146: represent every
non-capturing user call with one pooled, variable-sized value buffer whose derived layout
is `locals | registers`. The frame owns that buffer once, stencil-visible local and
register bases point into it, and return clears/releases it once. Capturing functions and
scripts retain their external environment storage but use the same execution-frame ABI.

`CallFrameLayout` is the quoted source of all offsets and counts. No handler may infer
the register base independently. Rust semantic kernels must access registers through the
same raw base exposed to AOT stencils; the owning vector exists only for lifetime and
cleanup, not as a second addressing model.

This removes one pool pop/push, one vector object, and the nested acquire/release boundary
from every non-capturing call. It does not yet implement direct machine-code caller/callee
continuations, so Task 146 remains in progress.

Acceptance: layout and semantic tests, complete release tests and V8v7 smoke, native
profile evidence, and alternating full-suite A/B against the accepted Task 160 binary.
Reject and revert if aggregate performance does not improve or any component crosses the
standing regression floor.

## Result

Accepted. `CallFrameLayout` derives the local count, register base, and total slot count.
A non-capturing call now acquires one pooled `Vec<Value>`, initializes locals in its
prefix, exposes the suffix as the shared stencil/kernel register base, and clears/releases
the whole range once. The separate local-value pool and its acquire/release routines were
removed. Capturing frames and scripts enter the same `run_with_owned_values` executor
with external locals and a register-only owned range.

All Rust semantic helpers now address registers through `DynFrame::register_values`, the
same base consumed by AOT machine stencils. The owning vector is used only to keep the
allocation alive and return it to the pool. A debug invariant proves that the derived
register range is contained in the owned buffer.

Evidence:

- 81 release tests pass, including the explicit contiguous-layout test.
- Complete smoke: `reports/task168-contiguous-frame-smoke.jsonl`.
- Targeted ten-pair Richards A/B:
  `reports/task168-contiguous-frame-richards-ab-10/comparison.txt`, 522 to 538.5
  (**+3.16%**).
- Full six-pair, 500 ms comparison:
  `reports/task168-contiguous-frame-full-ab-6/comparison.txt`, 1723.24 to 1779.39
  (**+3.26%**). Richards improves **6.94%**, DeltaBlue **8.38%**, and the lowest
  component is Navier–Stokes at **-0.75%**, safely above the standing floor.
- Accepted native sample: `reports/task168-richards-accepted.sample.txt`.
  `release_local_values` is absent; the remaining dominant boundaries are the generic
  block step, `run_with_owned_values`, `DynJitCode::call`, and
  `call_arguments_with_ic`.
- Accepted binary: `/tmp/deegen-task168-contiguous-frame`, SHA-256
  `06c24dc25fcf926f2430f49f6f1775fcc256d47d519dbfd478fc0131247508a1`.

This completes the contiguous-storage slice only. Task 146 remains open for direct
machine-code call/return continuations, one reusable VM stack, and explicit exceptional
continuations.
