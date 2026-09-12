# 317 — Immutable call recipes and POD inline targets

Status: complete

Factor immutable per-function call metadata out of `DynJitCode` into one
`FunctionCallRecipe`, and give every call site one stable `InlineCallTarget` record.
The record owns the decoded register operands, destination, source span, cached callee
identity, target environment, and published recipe pointer. The non-null recipe pointer
is the sole readiness fact and is published last, after all other fields are initialized.

This is the cold-data half of [[181-native-direct-user-call-continuation-regions]]. It
removes repeated bytecode decoding and gives cooked call stencils a POD target they can
address directly. It is not yet the native call continuum: the successful edge still
uses Rust frame construction and completion helpers, so this task makes no performance
claim by itself.

Current implementation:

- `FunctionCallRecipe` is the compact immutable runtime fact for entry, final frame
  layout, parameter slots, `this`, `arguments`, capture, and argument use.
- Its parameter pointer is backed by `DynJitCode`'s immutable boxed slot array; keeping
  that storage physically separate preserves the measured compact field layout.
- `InlineCallTarget` is `repr(C)` and owns one immutable operand view plus the mutable
  monomorphic target publication fields.
- `CallIcSite` owns the operand array and publishes environment/identity before recipe.
- direct-child preparation and completion consume the target record rather than
  decoding `DynOp::Call` again.

Acceptance: layout tests prove the recipe is canonical and each call site owns one POD
operand view; publication ordering has an explicit test; release and GC-stress tests
pass. Completion of [[181-native-direct-user-call-continuation-regions]] additionally
requires the AOT call-main stencil to build/activate the guest frame, issue the direct
guest branch-and-link, and resume in a cooked return continuation without the current
Rust prepare/execute/finish seam.

Task 20's `InlinePlan` is now a derived view of `FunctionCallRecipe`, not parallel
metadata. The recipe remains the single fact for parameters, receiver, frame layout,
capture, arguments, and return behavior; inlining adds only caller-specific slot renaming
and continuation labels. This preserves one representation of call semantics whether the
edge is emitted as a direct call or erased by composition.

Task 336 now consumes this representation on the executed monomorphic edge. Each site
retains one mutable, cleared activation compatible with its immutable recipe; recursive
entry is safe because the activation is taken out of the cache for the duration of the
call. The complete-suite A/B improves 3.84%, with 5,291,282 reuses versus 68 allocations
in a Richards wiring run. Task 351 completes the representation acceptance: final
linking, frame construction, and Task 20 eligibility share the compact recipe. An
always-present higher-level wrapper was measured and rejected; categorical compatibility
does not require co-locating cold backing storage with the ABI record. The native
call/complete seam remains separate Task 181 work.
