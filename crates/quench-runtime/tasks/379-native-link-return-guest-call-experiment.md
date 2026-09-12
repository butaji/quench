# 379 — Native link/return guest-call experiment

Status: planned

## Post-Task-381 audit

The call-looking cooked handler is still not a native call continuum. Call-region
splitting is gated by `DEEGEN_DIRECT_CALL_REGIONS`; `deegen_dyn_direct_call` loads the
frame's function pointer and crosses into Rust `execute_direct_call`, which performs
`prepare_direct_child`, frame entry/completion, and result handling before returning a
continuation address. Therefore normal V8v7 does not gain an end-to-end call stencil merely
because this handler exists, and enabling the gated path repeats the fragmentation already
rejected by Tasks 353/362. The refreshed residual profile contains about 1.35 million
call-only block entries in 20 ms, but reach is not permission to reuse this callback.

Keep this task planned until the complete call/return region can consume the existing
immutable `FunctionCallRecipe` and stable activation directly from the guest connector,
with no host call on the successful arm and a coarse surrounding cover.

Evaluate a second physical realization of the existing guest-call morphism for exact-arity,
noncapturing, monomorphic calls inside a fully native caller region:

```text
CallLink = GuardTarget ; PushGuestFrame ; BLR(guest_entry)
ReturnLink = PopGuestFrame ; RET
```

The semantic representation remains CPS with an explicit return continuation. Only final
AArch64 lowering chooses between the current frame-stored `return_target` plus tail branch
and a matched `BL`/`BLR` plus `RET`. Thus kernels and patched stencil instances remain
compatible arrows in the same category; this is not a second call semantics.

Save and restore `x30` in a named guest-frame word for nested calls. Preserve precise GC
roots, recursion, stack overflow, caught/uncaught exceptions, argument padding, and result
ownership. Variadic calls, `arguments`, capturing frames, tail calls, constructors, and
effect paths initially use the existing continuation/kernel realization. Direct branch
range, frame-word offsets, and maximum exact arity are named constants.

The hypothesis is physical and must be measured: Arm documents that `BLR` pushes and
`RET` pops the return stack, while an indirect tail branch uses general indirect-target
prediction. The candidate may still lose because native-stack bookkeeping or exception
recovery outweighs prediction. Do not proceed while adjacent bytecodes still enter generic
Rust blocks; Tasks 309/316/348 and direct surrounding coverage are prerequisites.

Acceptance: disassembly proves a successful hit contains no Rust call helper and has a
matched call/return pair; nested recursion and exception tests preserve continuation
semantics; Task 331 records indirect-branch/return behavior where platform counters allow;
call-heavy suite diagnostics show the direct arm is exercised; the complete exact A/B gate
decides whether this lowering is retained.

Primary sources: Arm's documented AArch64 return-stack behavior
<https://documentation-service.arm.com/static/649ac6d4df6cd61d528c2bf1> and V8's concrete
JavaScript call-frame/return-address convention
<https://v8.dev/blog/adaptor-frame>.
