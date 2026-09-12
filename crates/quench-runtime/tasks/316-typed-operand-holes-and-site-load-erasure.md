# 316 — Typed operand holes and `InlineSite` load erasure

Status: planned

Extend the AOT cooker/linker from control-only holes to the runtime constants ordinary
native operations actually consume. Define one macro-generated typed patch vocabulary:

- `RegisterByteOffset` and `LocalByteOffset`;
- `LiteralBits` or a relocation-closed local-data reference;
- `IcDataPointer`;
- `DirectTarget`;
- `SequentialContinuation`, `BranchContinuation`, and `SlowContinuation`.

Cook handlers with named symbolic placeholders, extract the corresponding relocations,
and patch derived byte offsets/addresses during final `StencilInstance` linking. Do not
patch source slot ordinals and multiply them again in the hot code. Every AArch64 field
encoder owns a named mask, shift, signed/range predicate, and fallback template; no
instruction bit pattern or immediate limit may appear as a magic number at call sites.

This makes the current stencil path materially different from a native wrapper around
an `InlineSite` descriptor. A successful load/store/arithmetic/property leaf should use
the burned operand directly and must not reload its register/local/literal operands from
site metadata. The metadata plane remains canonical for slow kernels and mutable IC
fields; it is not repeatedly decoded by closed fast paths.

The semantic macro is the single source for both operand declarations and stencil patch
records. Unsupported immediate ranges select an explicitly named general template or
relocation-closed data atom; they must not truncate or synthesize runtime assembly.

Acceptance: extractor negative tests reject missing/duplicate/wrong-kind operand holes;
round-trip tests cover minimum/maximum and one-out-of-range encodings; linked
disassembly for representative arithmetic, local, branch, and property operations has
no `InlineSite` operand loads; template bytes remain immutable; complete release/stress
tests and alternating V8v7 A/B improve before default enablement.

Primary sources: Copy-and-Patch burns literals, frame offsets, calls, and branches into
stencils <https://arxiv.org/abs/2011.13127>; Deegen extends that mechanism to generated
bytecode semantics <https://arxiv.org/abs/2411.11469>. LLVM's relocation/fixup model is
documented in JITLink <https://llvm.org/docs/JITLink.html>.

Depends on Tasks 02, 36, 43, 54, 55, 79, 102, 137, 149, 203, 274, and 309.

Round-thirty-five safety refinement: [[367]] must differentially validate every new typed
hole against rustc/LLVM codegen before the operand catalog grows. Identical cooker inputs
must produce identical objects; changing only named placeholders may change only the
declared instruction-field masks. [[366]] then consumes burned successors and operands to
carry a symbolic guest PC through a closed composite instead of advancing `InlineSite` after
each leaf.
