# Alignment with the Deegen paper (arXiv:2411.11469)

This document replaces the closed `tasks/` queue (001-026, all themes
complete) as the source of truth for what the VM plan still owes the paper.
Previously the project sanctioned exactly one Deegen technique — copy-and-patch
baseline-JIT code generation (§7) — and explicitly excluded the rest of the
paper's two-tier design. That boundary is now lifted: the plan aligns on the
paper's full architecture, and the gap below is the new task backlog.

## What the paper describes (full scope)

1. **Single bytecode semantics source of truth** generating every tier
   (interpreter, baseline JIT, optimizing JIT) from one declaration (§3, §5.1).
2. **Baseline JIT via copy-and-patch** stencil code generation (§7).
3. **Generic inline caches** with a λi (idempotent probe) / λe (effectful
   apply) split, reused across interpreter and JIT tiers (§5.2).
4. **Profiling and tier-up policy** — call-count/loop-count thresholds that
   promote a function from interpreter to baseline JIT to optimizing JIT (§3).
5. **Optimizing JIT tier** — a DFG-style IR built from collected type
   feedback, doing real instruction selection, register allocation, and
   speculative type-based optimization (§5.1, algorithm 𝒜 extended beyond
   type-check elimination into full specialization).
6. **OSR entry** — on-stack replacement so a hot loop already running in a
   lower tier can jump into optimizing-JIT code mid-execution (§3).
7. **Deoptimization protocol** — bailout from speculative optimizing-JIT code
   back to the baseline tier/interpreter when a guarded assumption fails,
   including correct frame/stack reconstruction (§3).

## What quench already has

| Paper component | Status | Evidence |
|---|---|---|
| Single-source bytecode semantics (`vm_op!`) | Done | tasks 003-006 (closed queue) |
| Copy-and-patch baseline JIT | Done | `docs/copy-and-patch-jit.md`, `stencil_*.rs` |
| Inline caches (quickening sites) | Done, partial paper fidelity | `quickening.rs`, tasks 013-016 (closed queue) — bounded polymorphic cache, not yet the paper's fully generic reusable λi/λe IC body shared *across* interpreter and JIT |
| Callee-directed CPS dispatch | Done | task 026 (closed queue), prerequisite for tier-up/OSR entry points too |

## What's missing (the new backlog)

- **Profiling/tier-up policy.** No call-count or loop-count instrumentation,
  no promotion decision from interpreter → baseline JIT → optimizing JIT.
  Stencil rendering today is memoized-by-fact, not hotness-triggered — the
  paper's tier-up is a different mechanism entirely and doesn't exist yet.
- **Optimizing JIT tier.** No DFG-style IR, no real instruction selection or
  register allocation, no speculative type specialization beyond the
  region-stencil's build-time type-check elimination. This is the largest
  gap.
- **OSR entry.** No mechanism to transfer a live interpreter/baseline-JIT
  frame into optimizing-JIT code mid-loop.
- **Deoptimization protocol.** No bailout path from speculative code back to
  a lower tier with reconstructed frame state. The stencil tier's `Repatch`/
  `Retired` degrade path (task 024) is the closest existing analog but only
  handles fact staleness, not mid-execution speculation failure.
- **Fully generic IC bodies shared across tiers.** Current inline caches are
  interpreter-side only; the paper reuses the same λi/λe IC across baseline
  and optimizing JIT.

## Sequencing

Tier-up policy and OSR entry are prerequisites for the optimizing JIT tier
(you cannot promote into code that doesn't exist, and you cannot deoptimize
out of a tier that's never entered). Deopt is a prerequisite for admitting any
*speculative* optimization in the optimizing tier — without it, the tier can
only do what the existing build-time-proven region stencils already do, which
would make it redundant. Recommended order: profiling/tier-up → OSR entry →
deopt protocol → optimizing JIT IR (bring-up with deopt as the safety net from
day one, not bolted on after).
