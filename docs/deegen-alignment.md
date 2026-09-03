# Alignment with the Deegen paper (arXiv:2411.11469)

This document is the source of truth for what the VM plan owes the paper.
Status below is from a direct code audit (file:line evidence), not from doc
comments or names alone — several existing names are misleading (see #8).

**Deegen is a two-tier design.** Per §1: "Deegen currently automatically
generates a VM with a state-of-the-art interpreter, a state-of-the-art
baseline JIT, and the tier-switching logic that connects the two tiers." An
optimizing JIT and deoptimization are both named explicitly as unimplemented
future work in the paper itself. Complete alignment means the interpreter,
the baseline JIT, tier-switching (tier-up + OSR-entry), and the paper's named
optimizations below — not an optimizing JIT or deopt.

## Audited status, mechanism by mechanism

| # | Paper mechanism (section) | Status | Evidence |
|---|---|---|---|
| 1 | Single-source DSL generating interpreter+JIT+IC bodies (§2-3) | **Partial** | `vm_op!` (`ir.rs:48-262`) generates the opcode catalog, dispatch metadata, and stencil/IC *eligibility facts* — it does not generate handler bodies, stencil machine code, or IC logic; those are hand-written Rust. One fact source, not one source emitting all three artifacts' bodies (the paper's stronger claim). |
| 2 | Call inline caching, dual monomorphic/polymorphic (§3) | **Present** | `quickening.rs` `GuardedCallHit`/`InstallCallGuard`, bounded-polymorphic call sites (`quickening.rs:400-404`) |
| 3 | Generic IC λi/λe split (§4, §5.2) | **Partial** | `GenericIcDecision<S>`/`GenericInlineCache` implement the idempotent-probe half (`quickening.rs:32-62`). |
| 3b | Slow-path outlining, `EnterSlowPath`-style CPS transfer, AOT-compiled non-inlined slow path (§4) | **Absent** | `Opcode::Slow` / `run_instruction_fallback` are ordinary fallback handlers, not a CPS "enter and never return" construct. |
| 4 | Type-check elimination algorithm 𝒜 (§5.1) | **Present** | `stencil_fact.rs` `BoxingFact`/`RegionKey::from_facts` over `Certainty::Proven/Guarded` vectors; `docs/copy-and-patch-jit.md` names this explicitly as mirroring 𝒜. |
| 5 | JIT-side λi/λe as self-modifying-code (SMC) IC stub chain (§7.1) | **Absent** | Repatch is hole-patching of pre-rendered bytes in a bounded arena (`stencil_patch.rs`), not an SMC stub chain that grows/branches at runtime. |
| 6 | Tag register optimization (§5.3) | **Absent** | No pinned register holding a large boxing constant for small-offset field access; `tagged_value.rs` embeds NaN-tagging constants as immediates. No `tag_register`/`TAG_REG` anywhere in the crate. |
| 7 | Register pinning for VM state across dispatch (§6.1) | **Absent** | `vm_dispatch.rs`/`vm_runtime.rs` pass `CodeView`/`RegisterFile`/`VmContext` as ordinary parameters; no fixed-register calling convention, register allocation left to rustc/LLVM. |
| 8 | Bytecode quickening — literal opcode/instruction rewrite on IC hit (§6.2) | **Absent (misleading name)** | `quickening.rs` implements a side-table IC (item 3), not literal bytecode-stream mutation. No opcode/instruction write-back found anywhere in the file. The paper's quickening specifically replaces the instruction itself; this doesn't. |
| 9 | Baseline JIT stencil granularity: one stencil per bytecode variant (§7) | **Diverges by design** | quench fuses multi-opcode **region** stencils instead (`docs/copy-and-patch-jit.md`: "this project's own extension beyond the paper"). Currently only a handful of hand-enumerated regions exist (Number Add+Return, one guarded property region, a two-region fallthrough — see `stencil_select.rs`), narrower in breadth than the paper's full per-bytecode coverage even though the fusion technique itself is a deliberate, documented extension. |
| 10 | Polymorphic IC + IC inline slab in JIT (§7.1) | **Absent** | No SMC stub chain and no inline-slab-vs-outlined-stub distinction anywhere in `stencil_arena.rs`/`stencil_patch.rs`. |
| 11 | Hot-cold code splitting by block-frequency analysis, fallthrough branch elimination (§7.2) | **Absent** | `CodeView::cold`/`cold_at` (`machine.rs:2006-2013`) is an unrelated compact-instruction operand-overflow side table, not frequency-based code splitting. `docs/copy-and-patch-jit.md` explicitly disclaims hotness-triggered mechanisms for the current tier. |
| 12 | Tier-up via per-function retired-bytecode counting (§3) | **Present, committed and verified** | `TierState{invocations, retired, threshold:32}` (`machine.rs:1940-1961`), `enter_invocation()`/`retire_at()` (`machine.rs:2241-2340`) — counts bytecodes retired within the function, matching the paper's design exactly. Covered by the runtime library gates and the neutral-corpus run recorded for task 027. |
| 13 | OSR-entry: branch into already-compiled JIT code at a back-edge (§7.1) | **Present and verified** | `is_osr_candidate`/`is_osr_entry` (`machine.rs`) computes admission; `maybe_osr_switch` (`vm_runtime.rs`) now checks the compiled plan entry before transferring the live frame into baseline code. The synthetic compact back-edge test records one transfer and matches the cold interpreter result; `ForI` is explicitly excluded as a structured loop with no bytecode back-edge. |

## Extra, beyond-paper: `ExecutionTier::Optimizing`

`machine.rs` has a third promotion tier (`ExecutionTier::Optimizing`,
`OptimizingPlan`) beyond the paper's two tiers. Its own doc comment: "a
physical execution view, not a second semantic IR" — re-wraps the same
baseline entries/stencil plans, x86_64-gated, no real instruction selection,
register allocation, or speculation (so no deopt needed — it never
speculates). **Decision: keep it**, verify it's a measured net win (task 030),
document it as not the paper's (nonexistent) optimizing JIT.

## Summary: what's real work vs. what's misnamed vs. what's done

- **Done**: call IC (#2), type-check elimination algorithm (#4), tier-up
  counting (#12), OSR-entry wiring and admission test (#13).
- **Real gaps, worth closing**: slow-path outlining/EnterSlowPath (#3b), tag
  register optimization (#6), register pinning (#7), true bytecode quickening
  as instruction rewrite (#8), JIT-side SMC IC stub chain + inline slab
  (#5/#10), hot-cold code splitting (#11), OSR-entry wiring verification
  (#13), stencil region breadth (#9).
- **Intentional, documented divergence, not a gap**: region-fused stencils
  instead of one-per-bytecode (#9's fusion *technique* itself) — keep as is,
  it's this project's own sanctioned extension.

## Sequencing

See `tasks/index.json` theme `deegen_full_jit` (027-037) for the ordered
backlog closing these gaps.
