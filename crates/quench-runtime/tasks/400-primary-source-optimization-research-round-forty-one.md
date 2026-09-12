# 400 — Primary-source optimization research, round forty-one

Status: complete

Online primary-source research was repeated after the accepted Task 397/399 checkpoint and
correlated with fresh profiles of that exact binary. The purpose is to choose an algorithm
that removes measured work, not to accumulate another disconnected optimization list.

## Measured residual

The accepted binary is sampled in
`reports/task400-primary-source-optimization-research-round-forty-one/profiles/`.

- Richards attributes **56.40%** of samples to `dyn_block_step_impl`, then 10.10% to
  `reset_value_slots` and 9.53% to the dynamic execution boundary. A 20 ms run records
  520,911 semantic-kernel entries, 2,076,319 direct-opcode entries, and zero direct-call
  entries.
- Crypto attributes **87.32%** to `dyn_block_step_impl`. Its 20 ms run records 189,961
  semantic-kernel entries, 1,185,323 direct-opcode entries, 235,209 numeric-region entries,
  and zero direct-call entries.
- Richards' largest residual blocks combine property loads, calls, conditions, stores, and
  returns. Crypto's largest residual blocks combine dense accesses, Word32 operations,
  property loads, calls, and loop control. Therefore another isolated opcode stencil or
  another call-only split would increase seams without covering the dominant morphism.

The samples were intentionally diagnostic and use a profiler-distorting three-second
window; exact scores remain only those produced by Task 05's gate.

## Primary-source algorithms and their local form

1. **Known-context graph construction.** Maglev performs a liveness/loop-assignment prepass,
   pre-creates loop phis, and carries known node information while constructing SSA. In this
   VM, Tasks 171/144/152 should carry `Representation × Location × Shape × EffectVersion`
   across a whole quoted region. Task 385 is the physical register-resident realization.
2. **Bounded basic-block versions.** Lazy BBV reports eliminating 71% of type tests;
   interprocedural BBV reports 94.3%. The static-MVP adaptation is not heat-based cloning:
   enumerate a named maximum of context versions from bytecode demands and immutable call
   recipes, then use a total generic kernel as the final cover.
3. **Typed-shape propagation.** Higgs reports that typed shapes plus propagation eliminate
   48% of tests, reduce code size 17%, and reduce execution time 25%. Task 399 proves the
   local value of preserving a shape/backing fact. Tasks 152 and 368 should extend that fact
   across effect-safe regions and invalidate it with explicit epochs/watchpoints.
4. **Binding-time specialization and inline slabs.** Deegen's 18 November 2024 paper and
   LLVM talk describe copy-and-patch with runtime-constant propagation, IC inline slabs,
   hot/cold splitting, and tail-jump elimination. Here immutable `FunctionCallRecipe`,
   shape IDs, property slots, continuation PCs, and frame offsets are patch inputs; the
   successful call-containing arm should contain no Rust helper.
5. **Effect-versioned hoisting.** LLVM LICM uses MemorySSA and alias information to hoist
   invariant loads and promote must-alias memory. Task 173's finite JS effect tokens are the
   lawful analogue: property/dense facts may cross only operations whose effect summary
   cannot invalidate the corresponding version.
6. **Demand-driven Word32 islands.** V8's lowering uses consumer representation demands so
   bitwise/shift chains remain Word32 until an observing boundary. Tasks 264 and 385 should
   be implemented as one region transformation, not revived as the rejected per-op I32
   stencil experiment.

## Ranked implementation order

1. **Task 401:** quote one-call residual superblocks as a context-preserving
   `prefix ; call ; continuation` morphism. This attacks both Richards' dominant property
   call blocks and the call seams inside Crypto loops and supplies Task 379's required
   coarse surrounding cover.
2. **Tasks 264/385:** once calls no longer force a whole block into Rust, carry Word32
   demands and physical registers across maximal call-free islands. This directly targets
   Crypto's two largest residual shapes.
3. **Tasks 173/152/368:** thread effect versions and stable shape dependencies so the first
   guard is reusable beyond one region without repeated Rust validation.
4. **Task 328:** lower the remaining bounded entry guard product as one cooked AArch64
   predicate only after the higher-level context has enough reach to amortize it.

All selectors remain static and source-independent. Every budget uses named constants;
there is no hot-path counter, benchmark name, source offset, property spelling, or exact
V8v7 code pattern in the plan.

## Primary sources

- Deegen, Xu and Kjolstad, 2024: <https://arxiv.org/abs/2411.11469>
- V8 Maglev design: <https://v8.dev/blog/maglev>
- Lazy basic-block versioning: <https://arxiv.org/abs/1411.0352>
- Interprocedural basic-block versioning: <https://arxiv.org/abs/1511.02956>
- Typed object shapes and shape propagation: <https://arxiv.org/abs/1507.02437>
- LLVM LICM and LCSSA: <https://llvm.org/docs/Passes.html#licm-loop-invariant-code-motion>
- LLVM MemorySSA: <https://llvm.org/docs/MemorySSA.html>
- V8 use-directed Word32 lowering source:
  <https://chromium.googlesource.com/v8/v8/+/f45c842fe1b011f7fde237112067dcc999b71dd3/src/compiler/simplified-lowering.cc>
