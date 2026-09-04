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
| 1 | Single-source DSL generating interpreter+JIT+IC bodies (§2-3) | **Partial, by decision, not by gap** | `vm_op!` (`ir.rs:48-262`) generates the opcode catalog, dispatch metadata, and stencil/IC *eligibility facts*. It does not generate handler bodies (correctly — those are irreducible JS semantics, not glue, per this project's own "handwritten code is limited to irreducible behavior" principle; auto-generating them would need a Deegen-scale LLVM-IR-analysis pipeline, not worth the cost). The IC layer is already well-factored and reused (`quickening.rs`'s `GenericInlineCache<K, S, N>`), so the paper's λi/λe scaffolding isn't hand-duplicated per opcode. What *is* real duplication: each new stencil region's `RegionKey` construction + CFG-soundness test (tasks 021-042). Task 044 generates that wiring from a declaration, once 040/041 produce enough regions (3-5) to justify the pattern. Full paper parity stays out of scope permanently; the wiring gap is task 044's, not this row's. |
| 2 | Call inline caching, dual monomorphic/polymorphic (§3) | **Present** | `quickening.rs` `GuardedCallHit`/`InstallCallGuard`, bounded-polymorphic call sites (`quickening.rs:400-404`) |
| 3 | Generic IC λi/λe split (§4, §5.2) | **Partial** | `GenericIcDecision<S>`/`GenericInlineCache` implement the idempotent-probe half (`quickening.rs:32-62`). |
| 3b | Slow-path outlining, `EnterSlowPath`-style CPS transfer, AOT-compiled non-inlined slow path (§4) | **Present** | `enter_slow_path` (`vm_runtime.rs`) is the named one-way gateway: it resolves the cold operation, executes complete fallback semantics, and returns an explicit `DispatchTransition` targeting `Exit`. It is `#[cold]`/`#[inline(never)]`, shared by all stencil misses, and the three-way throw/break/continue transition test proves the CPS boundary. |
| 4 | Type-check elimination algorithm 𝒜 (§5.1) | **Present** | `stencil_fact.rs` `BoxingFact`/`RegionKey::from_facts` over `Certainty::Proven/Guarded` vectors; `docs/copy-and-patch-jit.md` names this explicitly as mirroring 𝒜. |
| 5 | JIT-side λi/λe as self-modifying-code (SMC) IC stub chain (§7.1) | **Present (bounded chain)** | `stencil_lifecycle.rs::IcStubChain` records one key/effect stub per bounded site, walks the chain in insertion order, and returns explicit fallback on a miss or exhaustion. It keeps physical placement disposable and does not duplicate JavaScript semantics. The existing W^X arena remains the publication boundary for rendered bytes. |
| 6 | Tag register optimization (§5.3) | **Rescoped, queued (task 043)** | Task 030's Gate 0 correctly found no reliably pinned physical register *across the Rust-level interpreter dispatch loop* — stable Rust has no portable GHCcc-style fixed-register ABI, and that part stays blocked, permanently. But that's not the only place this technique applies: `build.rs`'s stencil bytes (tasks 039/042) are already hand-encoded machine code with explicit, fully-controlled register arguments (`aarch64_fadd_d(rd, rn, rm)`, etc.) — *stencil-local* register allocation was never blocked, task 031's original scoping just conflated it with the dispatch-loop question. Queued as task 043, sequenced after 040/041 so there are multi-instruction regions (loop-glue sequences, property chains, call sequences) long enough for the win (avoiding repeated tag-constant materialization) to be real rather than negligible. No boxing layout or constant semantics change. |
| 7 | Register pinning for VM state across dispatch (§6.1) | **Present (safe approximation)** | `vm_runtime.rs` `DispatchState` groups `CodeView`/`RegisterFile`/`VmContext`/tier owner behind one stable state pointer across the CPS chain. Gate-0 DWARF/sample + ARM64 disassembly measured dispatch stack-memory operations at 259/1030 (25.15%) before and 242/1006 (24.06%) after; `run_instruction` remained 16.42% vs 16.52%. Rust has no portable GHCcc equivalent, so this is deliberately not a fixed physical-register ABI or unsafe register hack. |
| 8 | Bytecode quickening — opcode/instruction rewrite on IC hit (§6.2) | **Present (bounded logical rewrite)** | `ir.rs` declares generated `GetPropertyQuickened`/`GetNQuickened`/`AGetIQuickened` variants. `machine.rs` stores a fixed-size per-instruction rewrite cell; `CodeView::instruction` exposes the specialized opcode after a confirmed shape hit, and `dequicken_instruction` restores the canonical opcode on shape/key/descriptor mismatch. The immutable canonical stream and complete generic handler remain the fallback; no source/fixture identity is involved. |
| 9 | Baseline JIT stencil granularity: one stencil per bytecode variant (§7) | **Diverges by design; full catalog fallback** | quench fuses multi-opcode **region** stencils instead (`docs/copy-and-patch-jit.md`: "this project's own extension beyond the paper"). The generated `DISPATCH` region admits every `Opcode::ALL` entry (31/31, including the bounded quickened variants; 100% before and after task 035's declaration reconciliation), while the inventory in `docs/deegen-stencil-coverage.md` distinguishes that complete trampoline fallback from the smaller set of specialized leaves (8/31, 25.8%). |
| 10 | Polymorphic IC + IC inline slab in JIT (§7.1) | **Present (bounded placement model)** | `IcStubChain::install` chooses the reserved inline slab when a stub fits and otherwise allocates an outlined placement; capacity is fixed and the (N+1)th key returns ordinary fallback. Unit tests assert both placement classes and the arena-sized state bound. |
| 11 | Hot-cold code splitting by block-frequency analysis, fallthrough branch elimination (§7.2) | **Gate now justified; implementation still deferred** | Task 039 enabled the proven eight-leaf ARM64 backend. A five-second DWARF `sample` of neutral arithmetic captured `StencilArena::render_or_get` (4 rendered-region stack samples); `execute_f64` is inlined on this optimized ARM64 build. The navier-stokes sample remained timeout-bound and showed no rendered symbol, so no layout change is claimed here; task 034 remains the implementation owner after a workload with stable rendered-region samples is selected. |
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

Task 036 validation on this ARM64 macOS host used the same optimized
`target/debug/quench-node` artifact and three-run neutral-corpus command in
both configurations. With normal promotion reachable, the run was 100/100,
Score **253.929**, wall ratio **0.7861**, RSS ratio **0.3051**, and instruction
ratio **1.0860** (`target/micro-neutral-036-enabled.txt`). With
`OPTIMIZATION_WARMUP_MULTIPLIER = u32::MAX` for the measurement (so promotion
was unreachable), it was also 100/100, Score **251.828**, wall ratio **0.8104**,
RSS ratio **0.3056**, and instruction ratio **1.0835**
(`target/micro-neutral-036-disabled.txt`). The +0.83% score delta is within
the task's stated 2–3% noise band, and the executable optimizing view is
x86_64-gated, so this is **not a measured net win on the current ARM64
machine**. The temporary multiplier change was reverted to 8; no behavior
change is being claimed.

## Closed decision: a real optimizing JIT + deoptimization will not be built

The paper itself names a speculative optimizing JIT and deoptimization as
future work it does not build (§1, §3) — this was never a gap to close.
Worth stating the reasoning explicitly rather than leaving it as a bare
"not paper scope" line, since it's the one item someone could reasonably
assume is still on the table:

- **Cost**: a real optimizing tier needs a second IR, actual runtime
  instruction selection/register allocation, profile-guided speculation, and
  a deoptimization/bailout protocol to stay correct when speculation is
  wrong. This crosses this project's own standing line
  (`docs/copy-and-patch-jit.md`: "if instruction selection, register
  allocation, or CFG analysis ever shows up at runtime, that is a real
  optimizing JIT and out of scope") and opens a large, open-ended
  correctness surface (every speculative guard needs its own proven-correct
  bailout path).
- **What it would actually buy**: per this session's own curriculum
  measurements, the remaining slowness (recursion, megamorphic access,
  closures — 13-47x vs. Node) is a **coverage** problem (tasks 040/041
  closing `Call`/property/array stencil coverage) and a **region-size**
  problem (how large a build-time-provable region can span — task 042's
  sequential executor directly grows this), not a need-for-runtime-discovered-
  speculation problem. Both are addressable inside the existing
  build-time-proof discipline, with none of speculation's correctness risk.
- **Decision**: will not build. Any further performance appetite goes into
  widening 042's region-size ceiling and 040/041's coverage breadth instead —
  same mechanism, same soundness bar, no new risk class.

## Summary: what's real work vs. what's misnamed vs. what's done

- **Done**: call IC (#2), type-check elimination algorithm (#4), tier-up
  counting (#12), OSR-entry wiring and admission test (#13), and the shared
  slow-path CPS gateway (#3b), plus bounded logical instruction quickening
  (#8).
- **Real gaps, worth closing**: tag register optimization (#6), stencil region breadth (#9),
  and hot-cold splitting (#11) now that ARM64 rendered-region activity is
  observable; each remains independently gated by evidence.
- Register pinning is represented by the safe one-pointer dispatch-state
  approximation above; a fixed-register ABI remains unavailable in stable Rust.
- **Intentional, documented divergence, not a gap**: region-fused stencils
  instead of one-per-bytecode (#9's fusion *technique* itself) — keep as is,
  it's this project's own sanctioned extension.

## Sequencing

See `tasks/index.json` theme `deegen_full_jit` (027-037) for the ordered
backlog closing these gaps.

## Task 039: native AArch64 stencil execution

Task 039 replaced executable stencil literals with build-time `const fn`
encoders for the eight existing specialized leaves (Move, Add, AddConst,
Return, Sub, Mul, Div, and GetN), with ARM ARM/Intel SDM field comments. The
opt-in `QUENCH_VERIFY_STENCIL_ENCODINGS=1 cargo check -p quench-runtime
--lib --no-default-features` clang/objdump check passed and is not part of
normal builds. The W^X arena and complete Rust fallback remain bounded.

Full gates passed: 585 no-default tests, 595 execution-trace tests, and
`cargo check -p quench-node`. The final bounded default neutral corpus was
100/100, Score **252.757** (1.0065 wall, 0.2935 RSS, 1.0563 instructions,
0.7282 cycles versus Node). On this ARM64 host, setting the explicit
`QUENCH_ENABLE_AARCH64_STENCILS=1` switch exercises the real leaves; its
paired sweep was 100/100 but scored **221.143**, so the default keeps the
complete ordinary baseline path until that ABI/call-overhead cost is improved.
A temporary ARM-reachable quench-only Optimizing view was also 100/100 but
scored **221.143** in the earlier paired run, so the final Optimizing gate
remains x86_64-only. The trace-enabled 38-case
curriculum completed with 33/38 cases passing its optional checks (the five
failures are existing counter/performance ceilings); speed score **171.9**,
memory score **352.5**. With the ARM switch enabled, cases 017/018/019 passed
at 0.41x/0.58x/0.50x wall time; cases 025/026/027 remained constrained
(3.09x, timeout, and 2.60x). These are existing slow-path findings, not
fixture-specific tuning.
