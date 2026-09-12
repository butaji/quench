# 145 — Inline-slab property and call IC stencils

Status: in_progress

Turn property and user-call IC hits into actual machine-code stencils embedded beside the
caller, rather than metadata that a generic Rust semantic kernel consults. Every eligible
site reserves a fixed-size, named inline slab in its `StencilInstance`. Initially the slab
branches to a shared miss `Kernel`. The first cacheable miss copy-patches a pre-cooked
monomorphic stub into the slab:

- property get/set: receiver-shape identity, then fixed-slot load/store;
- user call: callee identity, then direct transfer to the linked entry/continuation ABI.

The slab state is an explicit finite coproduct: `Empty | Mono | Poly | Mega`. Small arms
remain inline; overflow arms compose by reference to an outlined immutable kernel or
shared closed instance. No source spelling or benchmark identity participates in the
selection. Patching is caused by the first semantic miss, not a hotness counter.

Use an `IC_EMPTY_KEY_SENTINEL` that is proven impossible for the recipe's key domain, so
the empty and monomorphic states share the same compare-and-miss sequence instead of an
extra occupancy branch. On a call IC hit, the callee identity proof subsumes the generic
"is callable" tag test; test the IC key first and perform the generic tag/semantics only
in the miss kernel. Property and call effect arms are not C-ABI functions: they consume
the exact typed machine context at the call site and branch directly to the continuation.
Only the miss path crosses the shared kernel adapter.

The JIT-memory API must own write protection, atomic publication, AArch64 instruction
cache synchronization, relocation range validation, and named constants for slab size,
alignment, branch reach, and maximum inline arms. Never expose writable and executable
memory simultaneously where the platform API permits that guarantee.

This is the missing execution half of Tasks 08, 31, 137, and 140. The design follows
Deegen's impossible-key optimization, call-IC-check hoisting, context-local IC effect
arms, and inline slab (<https://arxiv.org/html/2411.11469v2>) and
JavaScriptCore's repatched inline-cache sequence
(<https://webkit.org/blog/10308/speculation-in-javascriptcore/>).

Acceptance: disassembly proves a warmed monomorphic hit stays inside linked machine code,
contains no separate occupied/callable checks, and calls no Rust IC helper; mono/poly/mega,
prototype invalidation, recursion, exception,
and concurrent-publication tests pass; counters distinguish slab hits and miss-kernel
entries; complete V8v7 A/B improves without component regressions.

## First executable vocabulary slice

Task 160 adds macro-generated own-property nullish predicate templates and accepts them
at +10.00% on Richards and +1.84% aggregate. A warmed shape hit already stays in copied
machine code and calls no Rust semantic helper, validating the intended fast-path body.
It still loads shape/slot fields from `PropertyIcSite`; executable slab reservation,
write-protected publication, inline mono/poly arms, and call-site slabs remain open here.

## Depth-one inherited arm granularity result

Task 306 extended all three existing cooked property-block families with a unified
own-or-immediate-prototype shape/slot recipe. It was genuinely cooked and executed, but the
full-suite result was -0.26% while adding 200 catalog bytes. Profiles showed the selected
block families overwhelmingly consume own fields; inherited traffic instead feeds user
calls. The arm was removed. Continue this task only as part of Task 163's fused method-call
recipe or after a site-coverage profile proves a different property slab reaches material
standalone traffic.

## Next executable slice after Tasks 352 and 353

Do not retry a leaf call through `execute_direct_call`. Task 353 eliminated 83,394 Earley
activations in 20 ms but was exactly neutral because the selected population was small and
the successful arm still crossed a Rust callback. Deegen's decisive constraint is stronger:
an IC effect arm is not a function; it consumes the surrounding JIT machine state and
branches directly to its continuation.

The next slice is therefore one fused property-call slab for existing bytecode, not an
inliner and not a benchmark pattern:

`GuardReceiverShape ; LoadMethodSlot ; GuardCallee ; PushGuestFrame ; JumpEntry`

The return continuation stores the owned result and resumes the caller without
`execute_direct_call`, `prepare_direct_child`, `make_frame`, or `complete_dyn_frame`.
The empty slab initially jumps to the canonical miss kernel; the first cacheable miss
copy-patches the complete arm. Task 153 supplies the immutable recipe, Task 316 burns
caller operands, and Tasks 146/181 supply the guest-stack entry/return context. Preflight
must demonstrate at least one million eligible executions in a 200 ms call-heavy suite and
disassembly must show no host call on a hit before a full A/B is attempted.
