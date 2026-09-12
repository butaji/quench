# 85 — Direct stencil selection observability

Status: complete

Record `compiled_direct_blocks` and `compiled_direct_opcodes` in the existing structured
JIT statistics. Counts are derived while the immutable bytecode description is lowered,
before emission, so they add no branch or counter mutation to executing machine code.
They distinguish genuinely closed rustc/LLVM stencil regions from generic block-kernel
instances without claiming that a helper-kernel bytecode is an inline native entry.

The default lowering now selects coarse direct stencils whenever coverage is disabled.
`DEEGEN_BLOCK_KERNEL_ONLY=1` forces the generic block kernel solely for reproducible A/B
diagnostics. Stencil coverage continues to select the generic path because coverage
requires per-bytecode observation.

Acceptance: JIT JSON contains both counts; a program containing a supported condition
reports nonzero values by default and zero with block-kernel-only mode; the counters do
not change execution semantics or the hot machine-code path.

Evidence: the Splay smoke reports 7 direct blocks covering 30 bytecodes in default mode
and zero/zero under `DEEGEN_BLOCK_KERNEL_ONLY=1`, while producing valid scores in both
modes. The linked-code footprint difference (10,624 versus 6,544 bytes across all
compiled images) is also visible through the existing `compiled_code_bytes` field.
