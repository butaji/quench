# 146 — Direct-continuation VM call stack

Status: in_progress

Replace the remaining user-call path through `Vm::call_arguments`, `DynJitCode::call`,
`run_with_locals`, and Rust `Vec`/frame setup with one custom VM stack and a fixed stencil
ABI. A call stencil writes a fixed frame header, arguments, locals, and a linked return
continuation, then transfers directly to the callee entry. Return restores the caller's
categorical context and jumps to that continuation. Exception and constructor exits are
explicit alternative continuations, not hidden Rust unwinding paths.

Use one variable-sized frame and store the actual argument count in a named header slot.
Arguments and registers must remain directly addressable without an argument-adaptor
frame; missing parameters read as `undefined`, extra arguments remain available to rest
and `arguments`, and the epilogue discards the frame in one operation. This follows V8's
adaptor-frame removal rather than copying arguments into another Rust or VM frame.

Frame offsets, alignment, reserved registers, argument base, local base, return slot,
continuation slot, and maximum inline argument count must all be named constants. The
layout is shared by primitive stencils, coarse regions, IC slabs, and shared kernels so
no adapter is needed at their boundaries.

The fast call operation is a pointer-bump state transition, not construction of a Rust
`DynFrame`: subtract/add the statically linked frame size from the VM stack pointer,
initialize the named header words and arguments, and jump. Keep the frame/local base,
code/slow-data base, VM/heap base, and return continuation in the pinned connector
registers from Task 203. A slow kernel may materialize a Rust view at the edge, but the
ordinary callee entry and return must not build a `Vec<Value>`, clone an `Env`, recurse
through `DynJitCode::call`, or use the host stack as the guest call stack.

Non-capturing frames remain entirely on the VM stack. A closure capture performs an
explicit materialization morphism into a heap environment; only escaping bindings pay
for heap ownership. This refines Task 76 and supplies the direct target required by Task
145. It follows the fixed-state/custom-stack pattern described by Deegen
(<https://arxiv.org/html/2411.11469v2>) and the compatible-frame design goal
of Sparkplug (<https://v8.dev/blog/sparkplug>), plus V8's single-frame argument-count
layout (<https://v8.dev/blog/adaptor-frame>).

Task 167 is direct negative evidence for doing this boundary work before adding more
call-shaped superinstructions. Its wired five-op method-call stencil removed two property
materializations and generic block dispatch but kept the Rust call boundary; Richards
still regressed 0.75% after code-size overhead was confined to the one affected function.

Task 168 begins the migration by unifying non-capturing locals and virtual registers in
one variable-sized owned frame. Direct machine-code continuations remain required before
this task can complete.

Task 178 folds name-snapshot values into the same owned range. Its +0.11% full-suite
result confirms that consolidating one more temporary allocation is only infrastructure;
the next slice must remove the recursive Rust executor transition itself.

Acceptance: recursion, closures, constructors, exceptions, rest/default parameters, and
receiver semantics pass; native samples no longer show the Rust user-call dispatcher or
per-call frame allocation as dominant stacks; disassembly shows pointer-bump frame setup
with no host call/prologue on the ordinary edge; a monomorphic user call transfers directly
from its IC slab to the callee and back; complete V8v7 A/B improves.

## 2026-09-10 current-path falsification and concrete split

The existing `DEEGEN_DIRECT_CALL_REGIONS=1` path is not this task's target architecture.
It enters the cooked prefix but then calls `execute_direct_call`, which invokes
`prepare_direct_child`, constructs the complete Rust-owned `DynFrame`, and finishes via
`complete_dyn_frame`. Three alternating 300 ms pairs on the four call-heavy suites measure
774 -> 741 Richards, 757 -> 679 DeltaBlue, 1768 -> 1757 RayTrace, and 3337 -> 3228 Splay:
**-4.68% geometrically**. Richards and Splay still regress at 96.79% and 98.44% call-IC
hit rates. The helper/frame boundary, not IC misses, is the rejected cost. Raw artifacts
are under `reports/task181-current-direct-call-diagnostic/`.

Task 307 extracts the existing thirteen-word AOT-visible prefix into an offset-zero
`GuestFrameHeader` and proves the structural change neutral. The next implementation
slice is one immutable `FunctionCallRecipe`, one POD per-site `InlineCallTarget`, and one
mutable activation. Rust `Rc`, `OnceCell`, `Vec`, `HashMap`, `Env`, and error layouts stay
behind owners/slow materializers; AOT code sees only raw stable pointers and tagged words.
Success and exception continuations and caller-specific register operands belong to the
stencil instance, not the shared callee recipe.

The successful immediate/object-handle arm must perform frame-pointer bump, argument
placement, direct native transfer, owned-slot cleanup, and continuation return without
calling any of the four rejected Rust functions above. Ownership-sensitive values,
captured environments, `arguments`, handlers, and unsupported arities enter explicit
generic stencil/kernel arms until their corresponding materialization records exist.

Task 310 completes the POD boundary beneath this split. One macro-defined schema now
generates both the runtime and AOT thirteen-word header, the Rust-owned fields live in a
separate sidecar, and the result word has an explicit exactly-once ownership protocol.
Its complete-suite result was neutral (-0.34%), as expected: the executed call path did
not change. Do not spend another experiment on layout-only frame work. The next candidate
must introduce `FunctionCallRecipe`/`InlineCallTarget` data and use it to remove at least
one of `execute_direct_call`, `prepare_direct_child`, full sidecar construction, or
`complete_dyn_frame` from a successful guest edge.

Task 336 removes repeated full sidecar/value-buffer construction from warm non-capturing
monomorphic calls by retaining one cleared activation per call site. Its exact complete
comparison improves 2117.62 -> 2198.90 (+3.84%); Richards and DeltaBlue improve 9.69% and
17.26%. A Richards wiring run records 68 allocations against 5,291,282 reuses. This is
accepted evidence that the frame boundary is material, but it is not this task's terminal
architecture: call entry, completion, and return still cross Rust helpers. The remaining
work is the direct guest-stack pointer bump and native continuation described above, or
erasure of eligible boundaries by Task 20's quote-level inliner.

Tasks 340 and 341 remove two general current-edge costs before that migration: a redundant
environment retain/release and a per-call block-list scan. Their decisive compound
comparison improves 2183.02 -> 2234.06 (+2.34%). Task 342 then shows the limit of this
approach: conditionally eliding one source-ID push/pop is neutral at +0.11%. Continue with
the guest stack and inliner; do not grow a catalog of per-sidecar micro-optimizations.

Task 355 closes the last plausible sidecar bridge. Copying the successful call algorithm
into each site regressed the aggregate 6.22% because the fragment grew to roughly 900
bytes. Sharing the algorithm as one immutable kernel restored the call connector to 112
bytes and reached 1,787,922 calls in a 100 ms Richards run, but still regressed the full
aggregate 5.04%. Thus the native -> Rust ABI -> native transition is itself disallowed on
the fast call edge. The next slice must use the terminal representation described above:
an in-place guest frame, direct callee transfer, and a patched return continuation. The
successful edge may not invoke `execute_direct_call` or any replacement Rust ABI kernel.

Use Deegen's `MakeInPlaceCall` contract as the exact physical model: the caller arranges
its argument range so the callee frame base can sit on it, then performs a non-returning
transfer with an explicit return-continuation component. Use V8's adaptor-free layout for
arity mismatch: reverse caller arguments, guarantee the formal slots, store the actual
argument count in a named header word, and let the epilogue discard the variable-size
frame. These are one composable call morphism and one return morphism over the same pinned
machine context, not host functions. Evidence and sources are in Tasks 355 and 356.

Task 357 lands the first terminal-representation prerequisite and keeps it after a neutral
complete-suite gate (-0.13%). `FunctionCallRecipe` now exposes a distinct post-prologue
`guest_entry`, and the generated guest-frame ABI contains a named `return_target` word.
Every function exit is a two-instruction copied adapter which loads that word and
tail-branches; host frames point it at the shared exit kernel. The callee image can now be
entered and returned from without baking caller identity into it. This is infrastructure,
not completion: the current call site still invokes `execute_direct_call` and the Rust
activation path. The next slice must replace that successful edge with machine-state child
frame setup, argument placement, a caller continuation, and a tail transfer to
`guest_entry`.

Task 362 implemented that hostless edge for a narrow, statically proven leaf subset and
falsified the isolated-call introduction order. It reached zero calls in four representative
suites and only 1,210 of 495,967 attempts in DeltaBlue; widening the proof to control flow
was unsound in RayTrace. Even the safe form regressed because splitting an unsupported call
out of a coarse generic block adds native/Rust/native seams and loses block batching.

Keep this task in progress, but do not retry another call-only leaf. Its next executable
slice depends on Tasks 309/157/316 making the neighboring semantic cover total, or must be
a whole call-containing region whose context includes ownership, effects, and all return
edges. The frame/continuation design remains valid; the rejected granularity does not.

## External confirmation from JSC source

Fetched `Source/JavaScriptCore/interpreter/CallFrame.h` directly
(`https://raw.githubusercontent.com/WebKit/WebKit/main/...`). JSC's `CallFrame` is
`class CallFrame : private Register` — the call frame *is* a typed view onto a position
in the shared VM stack, not a separately allocated object. Every call writes a fixed
2-slot `CallerFrameAndPC` header (caller-frame pointer + return PC,
`CallFrame.h:109-118`) immediately before the callee's own registers; `codeBlock` and
`callee` then sit at fixed, compile-time-known offsets counted from that same header
(`CallFrameSlot::codeBlock = CallerFrameAndPC::sizeInRegisters`,
`CallFrame.h:176-177`), with `callerFrameOffset()`/`returnPCOffset()` as named constant
offsets rather than any runtime lookup. This is independent, external confirmation that
this task's own design — one fixed frame header, direct-addressable stack, no
argument-adaptor frame — is the same shape production engines converged on, for the
same reason (no separate frame allocation, no indirection to find caller/return-PC/
callee). Does not change this task's current granularity blocker; recorded as design
validation, not new scope.
