# 270 — Preserve-none stencil continuation ABI

Status: complete

Replace the C ABI at successful stencil-to-stencil tail transfers with an internal
preserve-none continuation ABI. A predecessor never resumes, so no register is live
*after* the transfer in the predecessor; forcing the successor to save callee-saved
registers is dead work. The connector context remains the categorical object from Task
203, but its physical fields may use the larger preserve-none argument register set.

Keep ordinary Rust/C kernel calls behind explicit adapters. An adapter materializes the
canonical VM state, crosses the host ABI, then reconstructs the internal connector once.
Never let `preserve_none` leak into external functions, unwinding, variadics, or a call
which can return to its stencil caller.

Rust currently does not expose a typed `preserve_none` function ABI. Evaluate these
build-time routes in order:

1. have rustc emit LLVM IR, rewrite only macro-identified handler definitions and their
   `musttail` calls to `preserve_nonecc`, then run the pinned LLVM backend;
2. use a future rustc-supported equivalent only if it produces identical explicit IR;
3. use naked `extern "custom"` shims only as a last resort, because losing rustc's
   optimization of handler semantics defeats the design.

The IR rewrite is data-driven from the generated handler inventory, not a textual guess.
`STENCIL_CONTINUATION_CALLING_CONVENTION`, argument-register sets, reserved registers,
and host-adapter clobbers are named per-architecture constants. The extractor rejects a
handler whose definition/call conventions disagree or whose final transfer is not
`musttail`.

Acceptance: IR and disassembly prove no internal continuation saves/restores C callee-
saved registers; all Task 203 context fields survive arbitrary leaf/composite/loop/IC
composition; kernel adapters preserve semantics; full release tests and stencil-only
smoke pass; Task 158 shows lower spills/frame traffic; alternating V8v7 A/B decides
acceptance. AArch64's literature result may be neutral, so close with measured no-op
evidence rather than retaining complexity if bytes and counters do not improve.

Primary sources: LLVM `preserve_nonecc` and `musttail`
<https://llvm.org/docs/LangRef.html#calling-conventions>; Clang's AArch64/x86-64
attribute <https://clang.llvm.org/docs/AttributeReference.html#preserve-none>; CPython's
Copy-and-Patch experiment <https://github.com/python/cpython/issues/115802>; Rust's
current custom-ABI boundary <https://rust-lang.github.io/rfcs/3980-extern-custom.html>.

## Result

The prototype proved that rustc's optimized LLVM IR can be rewritten consistently:
all `deegen_dyn_*` and `deegen_region_*` definitions, their `musttail` transfers, and
the three continuation declarations used `preserve_nonecc`. Clang accepted the rewritten
AArch64 IR after removing rustc's unsupported `nocreateundeforpoison` attribute. The
resulting catalog remained relocation-closed, shrank from 11,528 to 11,524 bytes, and
all 94 release tests passed.

The required C-to-preserve-none entry adapter saves the host's callee-saved registers
once, then places the connector frame and site in `x20` and `x21`. That boundary cost is
not amortized by the current call architecture. The focused five-pair comparison in
`reports/task270-whole-function-focused-ab-5/comparison.txt` regressed from 1771.41 to
1758.15 (-0.75%); Richards regressed 2.43%. A cfg-gated production-source check still
regressed the full-suite aggregate by 0.70% in
`reports/task270-cfg-gated-default-full-ab-5/comparison.txt`. Consequently the ABI
prototype and all runtime scaffolding were removed from the accepted source.

The exact rejected preserve-none binary SHA-256 is
`f007098aa83d1ff6b751142275d5f2b3b35a3b3bb09c3a33b89e06ff738984ac`.
After removal, the rebuilt accepted binary has the same symbol map, `__text` size, and
`__text` SHA-256 (`a52b6a722d7b621d8777ca003630d920d1516e070c9055c029c9cc0f9d8ea2a1`)
as Task 273. Whole-function preserve-none is therefore a measured rejected transform,
not latent complexity in the VM. Reconsider only after Task 146 can amortize one host
adapter across a direct-continuation JS call chain, or as a region-local ABI with
explicit typed entry/exit adapters.
