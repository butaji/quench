# 93 — Zero-overhead diagnostic block kernels

Status: complete

Specialize the generic Rust semantic block kernel at stencil-link time. Normal execution
must not test the instruction budget, opcode statistics, block profiler, or source-line
coverage flags for every bytecode. A separately monomorphized instrumented kernel keeps
all existing diagnostics when any related option is enabled.

This is a staging boundary: diagnostic configuration is immutable for a `Vm`, so it is
an object-level choice made before lowering. Both variants implement the same bytecode
morphism and share `execute`; only the observation effect differs. Direct stencils and
generic fallback blocks remain composable under the same connector ABI.

Acceptance: debug/release tests; each diagnostic mode demonstrably still emits or
enforces its output; complete V8v7 smoke; alternating full-suite A/B before the fast
variant remains enabled.

Implementation result: `DynJitCode::build` receives the immutable instrumentation mode
from `Vm`. `dyn_block_step_impl` is monomorphized over both name-snapshot refresh and
instrumentation effects, producing four kernels. Copy-patching selects one helper at
link time. The slow adapter has matching instrumented and fast variants. Instrumented
images disable direct blocks so instruction budgets and opcode/coverage observations
cannot be skipped by a native region.

Evidence: 40 release tests pass; an instruction budget of one reports exhaustion at
dynamic stencil PC 1; opcode stats report 3,684,660 kernel exits in a short Richards
run and zero direct blocks, proving the instrumented image is active. All eight suites
complete in `reports/zero-diagnostics/smoke.jsonl`. The longer six-repetition balanced
A/B in `reports/zero-diagnostics-ab-6/comparison.txt` moves the aggregate from 569.716
to 597.078 (+4.80%). Crypto improves 22.26%; the largest regression is Navier-Stokes at
-4.76%, inside the standing -5% gate. The shorter four-repetition record is retained in
`reports/zero-diagnostics-ab/comparison.txt` for transparency; its noisy Navier median
was -6.17% and motivated the longer run.
