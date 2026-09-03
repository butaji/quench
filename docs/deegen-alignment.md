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
| 3b | Slow-path outlining, `EnterSlowPath`-style CPS transfer, AOT-compiled non-inlined slow path (§4) | **Present** | `enter_slow_path` (`vm_runtime.rs`) is the named one-way gateway: it resolves the cold operation, executes complete fallback semantics, and returns an explicit `DispatchTransition` targeting `Exit`. It is `#[cold]`/`#[inline(never)]`, shared by all stencil misses, and the three-way throw/break/continue transition test proves the CPS boundary. |
| 4 | Type-check elimination algorithm 𝒜 (§5.1) | **Present** | `stencil_fact.rs` `BoxingFact`/`RegionKey::from_facts` over `Certainty::Proven/Guarded` vectors; `docs/copy-and-patch-jit.md` names this explicitly as mirroring 𝒜. |
| 5 | JIT-side λi/λe as self-modifying-code (SMC) IC stub chain (§7.1) | **Present (bounded chain)** | `stencil_lifecycle.rs::IcStubChain` records one key/effect stub per bounded site, walks the chain in insertion order, and returns explicit fallback on a miss or exhaustion. It keeps physical placement disposable and does not duplicate JavaScript semantics. The existing W^X arena remains the publication boundary for rendered bytes. |
| 6 | Tag register optimization (§5.3) | **Blocked (dependency unavailable)** | Task 030's Gate 0 produced a safe grouped `DispatchState` pointer, not a reliably pinned physical register. Because stable Rust exposes no portable GHCcc-style fixed-register ABI, replacing `TAG_PREFIX`/`TAG_SHIFT`/`TAG_MASK` immediates in stencil code would have no sound pinned base. No boxing layout or constant semantics were changed. |
| 7 | Register pinning for VM state across dispatch (§6.1) | **Present (safe approximation)** | `vm_runtime.rs` `DispatchState` groups `CodeView`/`RegisterFile`/`VmContext`/tier owner behind one stable state pointer across the CPS chain. Gate-0 DWARF/sample + ARM64 disassembly measured dispatch stack-memory operations at 259/1030 (25.15%) before and 242/1006 (24.06%) after; `run_instruction` remained 16.42% vs 16.52%. Rust has no portable GHCcc equivalent, so this is deliberately not a fixed physical-register ABI or unsafe register hack. |
| 8 | Bytecode quickening — opcode/instruction rewrite on IC hit (§6.2) | **Present (bounded logical rewrite)** | `ir.rs` declares generated `GetPropertyQuickened`/`GetNQuickened`/`AGetIQuickened` variants. `machine.rs` stores a fixed-size per-instruction rewrite cell; `CodeView::instruction` exposes the specialized opcode after a confirmed shape hit, and `dequicken_instruction` restores the canonical opcode on shape/key/descriptor mismatch. The immutable canonical stream and complete generic handler remain the fallback; no source/fixture identity is involved. |
| 9 | Baseline JIT stencil granularity: one stencil per bytecode variant (§7) | **Diverges by design; full catalog fallback** | quench fuses multi-opcode **region** stencils instead (`docs/copy-and-patch-jit.md`: "this project's own extension beyond the paper"). The generated `DISPATCH` region admits every `Opcode::ALL` entry (31/31, including the bounded quickened variants; 100% before and after task 035's declaration reconciliation), while the inventory in `docs/deegen-stencil-coverage.md` distinguishes that complete trampoline fallback from the smaller set of specialized leaves (8/31, 25.8%). |
| 10 | Polymorphic IC + IC inline slab in JIT (§7.1) | **Present (bounded placement model)** | `IcStubChain::install` chooses the reserved inline slab when a stub fits and otherwise allocates an outlined placement; capacity is fixed and the (N+1)th key returns ordinary fallback. Unit tests assert both placement classes and the arena-sized state bound. |
| 11 | Hot-cold code splitting by block-frequency analysis, fallthrough branch elimination (§7.2) | **Not justified (Gate 0)** | A five-second DWARF-enabled macOS `sample` of a long neutral arithmetic loop showed no `StencilArena`/`render_selected`/`execute_f64` samples: executable rendered regions are x86_64-gated and inactive on this ARM64 host. With no measured rendered-region icache or branch-miss cost, task 034 correctly made no layout change. |
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
  counting (#12), OSR-entry wiring and admission test (#13), and the shared
  slow-path CPS gateway (#3b), plus bounded logical instruction quickening
  (#8).
- **Real gaps, worth closing**: tag register optimization (#6), stencil region breadth (#9).
  Hot-cold splitting remains an explicit not-justified Gate-0 item on ARM64.
- Register pinning is represented by the safe one-pointer dispatch-state
  approximation above; a fixed-register ABI remains unavailable in stable Rust.
- **Intentional, documented divergence, not a gap**: region-fused stencils
  instead of one-per-bytecode (#9's fusion *technique* itself) — keep as is,
  it's this project's own sanctioned extension.

## Sequencing

See `tasks/index.json` theme `deegen_full_jit` (027-037) for the ordered
backlog closing these gaps.
