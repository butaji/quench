# 303 — An AArch64-native value/object representation unconstrained by x86/32-bit heritage

Status: planned

V8's `Smi`/NaN-boxing/pointer-compression value representation and JSC's own tagged-
pointer scheme were designed under constraints this project does not share: both must
run correctly and competitively on 32-bit *and* 64-bit targets, and both must perform
well on x86/x86-64 as a first-class target, not only ARM64. [[55-architecture-target-matrix]]
already scopes native stencil execution to AArch64 only — this task asks what a value
representation looks like if it is designed *from* that constraint rather than adapted
*to* it, potentially diverging from both V8's and JSC's schemes rather than picking the
better of the two.

Concrete AArch64-specific capabilities neither V8 nor JSC can fully commit to, because
of their x86 obligations:
1. **NEON 128-bit loads for multi-field access.** A guarded object access that reads
   two or three adjacent fixed-offset fields (exactly [[222]]'s constructor/product
   pattern, or [[243]]'s columnar access) could load them as one 128-bit NEON vector
   register load instead of two or three separate scalar loads, when the fields are
   contiguous and the access pattern is already proven — x86-first engines rarely design
   around this because SSE/AVX register pressure and calling-convention conventions
   differ enough that it isn't a clean win on that architecture the way it can be here.
2. **TBI-tagged pointers** ([[208]], already planned) as a genuinely different
   tagging scheme from NaN-boxing, not merely an ARM64 port of it — TBI gives free
   hardware-level tag masking that NaN-boxing's bit-pattern scheme was never designed to
   need, since NaN-boxing's whole point is packing a tag into an IEEE-754 double's spare
   bit patterns, a constraint that has nothing to do with ARM64's actual pointer format.
3. **AArch64's larger, more regular general-purpose register file (31 GPRs vs. x86-64's
   16)** may change the register-pressure tradeoffs behind [[203]]'s pinned tag-register
   design and [[292]]'s monoidal register-resource model — a design decided by
   reading x86-first engines' choices risks importing a register-scarcity-driven
   decision that doesn't actually bind this project's target.

This is explicitly a *design proposal to evaluate*, not an assumed win — each of the
three capabilities above needs its own measurement before being adopted, since "ARM64
has a capability V8/JSC don't fully exploit" is not automatically "exploiting it helps
this specific workload."

Concrete steps:
1. Prototype one guarded multi-field access (from [[222]] or [[243]]'s work) as a single
   NEON 128-bit load versus the current scalar-load-per-field baseline, and measure
   directly — this is the kind of claim [[251]]'s disassembly-verification discipline
   already requires, not a design document assertion.
2. Cross-check [[208]]'s TBI design against this task's framing: TBI is not "porting
   NaN-boxing to ARM," it's a structurally different mechanism, and this task's
   contribution is stating that distinction explicitly so [[208]]'s implementation
   doesn't accidentally constrain itself to NaN-boxing's bit-budget thinking when it
   doesn't need to.
3. Re-evaluate [[203]]'s and [[292]]'s register-pressure assumptions against AArch64's
   actual register file size, not an x86-64-derived intuition about how scarce
   registers are.

Acceptance: at least one concrete NEON-based multi-field access is prototyped and
measured against its scalar baseline, with a stated result (adopted, or rejected with
the measured reason); [[208]]'s TBI design is confirmed to not be constrained by
NaN-boxing's bit-packing assumptions where it doesn't need to be; [[203]]/[[292]]'s
register-pressure assumptions are checked against AArch64's actual register count rather
than an unstated x86-derived default; alternating A/B on any adopted change shows a
measured gain with zero correctness regression.

No external primary source needed beyond ARM's own architecture reference for NEON load
instructions and the AArch64 register file, already implicitly in scope for this
project's AArch64-only target.
