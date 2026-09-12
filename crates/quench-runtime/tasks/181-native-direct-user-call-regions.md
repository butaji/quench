# 181 — Native direct user-call continuation regions

Status: in_progress

Implement the next executable slice of Task 146. Split any ordinary bytecode block at
user `Call` operations into a quoted sequence of maximal semantic ranges and rustc-cooked
direct-call stencils. Range leaves still invoke the singular semantic executor for their
whole maximal range; a warmed monomorphic, noncapturing user-call IC prepares one child
frame and calls the already-linked callee machine entry directly. Return transfers the
owned result, reclaims the child frame, and continues with the next range.

The composition is general and bytecode-based:

`range(start, call) ; direct_call(call) ; range(call + 1, next_call) ; ...`

No block shape, property spelling, source identity, benchmark identity, call count, or
hotness threshold selects it. An empty range is the categorical identity. IC-empty,
polymorphic, capturing, native, and unsupported cases tail-rejoin the canonical slow
stencil at the unexecuted call bytecode.

Child `DynFrame` storage comes from a stable reusable frame-slot pool. The callee's
existing prologue/exit preserves the parent connector frame around a normal AArch64
`blr`. A dedicated resume morphism consumes `current_site` plus `resume_target` after a
callee exception and jumps to the caller catch/exit without replaying the call. Frame
preparation, entry, completion, and exceptional resumption are explicit state-machine
transitions with named status and ABI constants.

Rustc/LLVM cooks the direct-call handler from `stencil-aot/handlers.rs`; runtime work is
only ordinary frame preparation plus transfer into already-linked code. Kernels and
patched instances retain the same connector category.

Acceptance: direct-call catalog and disassembly tests; structural call-range tests;
warm IC, recursion, nested calls, receiver/arguments, capturing fallback, native
fallback, thrown/caught/uncaught exception, result ownership, and frame-pool tests; all
release tests and complete V8v7 smoke. Counters or native samples must prove warmed calls
enter the callee directly. Keep only after targeted Richards/Earley-Boyer and complete
alternating A/B improve without component-floor violations.

## 2026-09-10 experiment: correctly wired, rejected as a default tiling

The previously present handler was dormant: no selector referenced
`deegen_dyn_direct_call`, no block was split at `Call`, and the resume label was not
emitted. The experiment added a quoted `SemanticRange | DirectCall` sequence, bounded
semantic-range execution, the missing dynamic exception-resume adapter, matching AOT
and runtime ABI assertions, execution counters, and a one-kernel stack-local child
activation. With `DEEGEN_DIRECT_CALL_REGIONS=1`, a Richards smoke recorded 280,428
direct hits and 9,347 misses, proving that the cooked stencil executed.

The composition is nevertheless a losing tiling while neighboring ranges remain generic
block kernels. A five-repetition, 200 ms alternating Richards comparison against the
accepted Task 178 binary measured 752 versus 685, **-8.91%**. The old path crosses one
block-kernel boundary and performs the cached call inside it; the new path crosses a
bounded prefix, call kernel, and bounded suffix. Moving the child frame from a TLS box
pool back to the native Rust stack removed an avoidable allocation but did not remove
the boundary multiplication.

The general infrastructure remains available behind the explicit
`DEEGEN_DIRECT_CALL_REGIONS` experiment switch, which is independent of source identity,
benchmark identity, counters, and hotness. It is not enabled by default. Re-evaluate only
after adjacent semantic ranges lower to actual closed stencil leaves, so composition
replaces rather than multiplies kernel transfers. Task 183 is next because canonical
built-in identity removes real allocations without requiring this unfavorable split.

## Round-twenty-one priority: method-call/frame/return continuum

Current 500 ms measurements pair inherited-property hits with warmed direct-call hits:
Richards 8.56m/7.75m, DeltaBlue 12.62m/10.19m, RayTrace 1.97m/1.82m, and Splay
0.82m/3.69m. Native profiles attribute roughly 25%, 32%, 17%, and 8% respectively to named
call/frame machinery. Task 306 also proved that an isolated inherited arm has negligible
reach in the currently selected property block patterns.

The next experiment must therefore be one coarse morphism:

`InheritedMethodGuard ; MonomorphicCalleeGuard ; GuestFramePush ; DirectTransfer ; ReturnContinuation`

It must not call `execute_direct_call`, `prepare_direct_child`, `make_frame`, or
`complete_dyn_frame` on the successful arm. Implement Task 146's POD guest-stack frame and
precise roots first; then consume the property and call IC facts without materializing the
intermediate method `Value`. Capturing/native/polymorphic/unsupported layouts rejoin the
canonical miss kernel before the call executes. This is the only justified retry of the
direct-call tiling; another prefix/call/suffix split around the existing Rust frame path is
already falsified.

Task 337 re-ran this experiment after Task 336's activation reuse, then moved the IC
readiness/identity guard into the AOT stencil and removed redundant checks from the
guard-certified helper. Helper misses disappeared, but the complete aggregate still
regressed 2174.78 -> 2162.03 (-0.59%). This separates two costs conclusively: frame
allocation mattered and was profitably cached, but helper-first IC lookup was not the
reason the direct tiling lost. The surviving cost is the host boundary and basic-block
fragmentation itself. The candidate is reverted; this task may complete only with the
native guest-stack transfer/return contract above.

Task 362 then supplied the missing hostless physical edge and still rejected this task's
call-only granularity. The safe semantic subset produced essentially no native hits; a
broader control-flow proof violated RayTrace correctness. More importantly, an unsupported
call still split one batched generic block into multiple connector transitions. The 20 ms
complete smoke fell to 2184.65 versus 2236.70 for the accepted comparison, and the dormant
default representation measured -0.96%, so the prototype was removed completely.

Reopen only after Tasks 309/157/316 can cover both neighboring ranges without generic block
re-entry, or replace the split with one whole call-containing region morphism. This task's
next proof obligation is therefore total surrounding cover, not another call IC, frame
field, or eligibility probe.
