# 209 — Static branch-weight metadata for AOT stencil cooking

Status: planned

[[161-primary-source-algorithm-research-round-three]] already cites LLVM's machine
block placement (ext-TSP-style layout, tail duplication) as the model for composed
stencil-block layout, and its refinement note for that item says "static semantic
likelihoods work on first execution; runtime counts may refine order but never gate
compilation." That refinement was never wired to a concrete LLVM mechanism. LLVM has
exactly the input machine block placement needs for this: branch-weight metadata
(`!prof` `branch_weights` on a terminator instruction, the same channel
`__builtin_expect`/`__builtin_expect_with_probability` and PGO both feed) directly
biases both the AOT codegen's branch layout choice and its `MachineBlockPlacement` pass.

At AOT stencil-cooking time ([[02-rustc-aot-templates]], [[43]], [[52]]), every stencil
template's exit shape is already known statically without needing a runtime profile:
- a shape/type guard's fail edge is the generic/slow path — always the cold arm;
- a loop backedge is always the likely-taken arm relative to loop exit;
- an exception-throw edge, a nullish/undefined-property error edge, and every
  `KernelExit` to the generic block step are cold by construction, per
  [[84-dead-result-condition-stencils]]'s and [[160]]'s existing dead/nullish-condition
  stencil families.

Emit this as explicit branch-weight metadata (via Rust's `core::intrinsics::likely`/
`cold_path`, or equivalent `#[cold]`-annotated slow-path functions, whichever the
rustc/LLVM AOT pipeline in [[02]] actually threads through to `!prof` metadata — verify
which mechanism survives to the emitted object code before assuming either works) on
every guard/loop/error edge in the stencil source templates, so `rustc`'s codegen and
LLVM's `MachineBlockPlacement` lay out the common/fast path as the fallthrough and push
every guard-miss/error/generic-exit arm out of line, without needing [[83]]'s runtime
block-frequency profile to do it — this is static, not measured, so it applies uniformly
before any profile exists, and [[28-profile-driven-block-layout]] remains the separate,
later refinement that uses measured frequency to override a static default where they
disagree.

Use LLVM's own profile-free vocabulary as the canonical policy: loop structure,
`cold`, `unreachable`, `noreturn`, unwind, and deoptimization/semantic-exit blocks.
Derive relative block frequency bottom-up through loop DAGs, then use that one result for
fallthrough choice, cold outlining, and bounded tail duplication. Do not maintain an
independent stencil-layout heuristic beside the LLVM metadata; one CFG fact must have
one representation.

Acceptance: every guard-miss, loop-exit-vs-backedge, and error/nullish edge in the AOT
stencil template source carries explicit static branch-weight annotation; disassembling
a linked stencil confirms the fast/common arm is the fallthrough and the cold arm is
outlined, verified on at least one guard family ([[08]]/[[129]]), one loop family
([[123]]/[[124]]), and one error family ([[84]]/[[160]]); alternating A/B on the full
V8v7 suite shows no regression and documents any measured gain from improved
instruction-fetch/branch-prediction behavior; [[28]]'s later runtime-profile-driven
layout is confirmed to compose with this (override, not conflict with, the static
default) rather than needing to replace it.

Experiment gate: first disassemble one representative guard, loop, and error stencil.
If rustc/LLVM already makes each semantic fast arm fall through and outlines the cold
arm, close the task as no-op evidence instead of adding annotations. Branch metadata is
not useful merely because it exists; it must change a wrong target layout.

Primary sources:
- LLVM Branch Weight Metadata: <https://llvm.org/docs/BranchWeightMetadata.html>
- LLVM machine block placement (already cited in [[161]]): <https://llvm.org/docs/doxygen/MachineBlockPlacement_8cpp.html>
- LLVM profile-free branch probability construction and loop-DAG block frequency:
  <https://llvm.org/docs/doxygen/BranchProbabilityInfo_8cpp_source.html> and
  <https://llvm.org/docs/BlockFrequencyTerminology.html>
