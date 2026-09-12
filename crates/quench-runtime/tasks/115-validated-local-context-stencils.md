# 115 — Validated-local context for AOT stencils

Status: complete

Make local-slot validity part of the categorical input object for native dynamic code.
Validate every `LoadLocal`, `DeclareLocal`, and `StoreLocal` operand once when converting
`DynCode` into `DynJitCode`; reject malformed code before executable composition. AOT
stencils then receive a `ValidatedLocals` context and may directly address local slots
without repeating bounds checks at every execution.

This is a proof-moving optimization, not unchecked optimism: the build-time validator is
the constructor for the refined context, and only that constructor can reach the native
stencil linker. The canonical semantic kernel retains its defensive behavior. No JS
source, suite, or hotness participates.

First evaluate the smallest change over already-accepted direct families: remove local
bounds checks only from terminal, dead-condition, update, and recurrence stencils after
the validator is wired. Record extracted byte-count/disassembly deltas before timing.
Do not reintroduce [[114-dead-local-local-condition-stencils]] in the same experiment;
that would conflate proof removal with vocabulary expansion.

## Result

The boundary validator rejected malformed `LoadLocal`, `DeclareLocal`, and `StoreLocal`
operands before native linking, while valid code built normally. Per-stencil bounds proofs
were removed from the accepted terminal, condition, update, and recurrence families. All
47 release tests and full smoke passed. The cooked byte reductions were:

| Stencil representative | Before | After |
|---|---:|---:|
| return local | 80 B | 64 B |
| dead local/number condition | 96 B | 84 B |
| dead local update + jump | 112 B | 100 B |
| dead recurrence | 140 B | 128 B |

Candidate SHA-256 was
`ca0f6bf6ebc74e4cda93db7482d1fd33b75d3e9eaa5662d896a02b0ffbf26e33`.
The exact alternating six-run comparison in
`reports/validated-local-context-ab-6/comparison.txt` measured 831.976 baseline versus
828.296 candidate, an aggregate regression of 0.44%. Seven suites were flat-to-negative;
Splay was +0.44% and Navier-Stokes +0.08%.

Rejected and reverted. The final binary is byte-identical to the Task 110 baseline
(`a239bed0433b589ed5efcd9f41029041db382e7c76c566052c196989a6887bed`). The refined
context is sound, but shrinking isolated stencils is not yet enough to improve this
runtime. The next structural target should compact the shared `InlineSite` representation:
its current 56-byte stride forces expensive address reconstruction and burdens every
direct stencil and block kernel, whereas all operands already fit in 32 bits.
